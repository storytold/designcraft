//! Interactive PDF forms (Buttons and Forms: text fields, check boxes, combo and list boxes,
//! signature fields), media (placed video and sound) and state buttons (Go To State, Next and
//! Previous State on a multi-state object whose states export as layers). krilla writes none of
//! these, so they're added as an incremental update: widget, screen and link annotations on
//! their pages, the page `/Annots`, the catalog `/AcroForm`, and the media files embedded for
//! rendition actions.

use std::collections::HashMap;
use std::fmt::Write as _;

use designcraft_doc::{ButtonAction, Document, FieldKind, FormField, Item, ItemId, MediaOptions};

use crate::export::Sheet;
use crate::transitions::{bracketed, int_after, object, rfind};

/// A button on an output page whose action switches a multi-state object's layers: (page index,
/// PDF rectangle, name, the object, what to show).
pub(crate) type PlacedStateButton = (usize, [f64; 4], String, ItemId, StateSwitch);

/// What a state button's `SetOCGState` action does (#1798). PDF has no "advance" action, so Next
/// and Previous State are exact only on a two-state object, where they toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StateSwitch {
    /// Show this state (0-based), hiding the others.
    Show(usize),
    /// Swap the two states of a two-state object.
    Toggle,
}

/// The name of the optional content group holding state `index` of multi-state object `id`;
/// [`add_fields`] finds the group by it.
pub(crate) fn state_layer_name(id: ItemId, index: usize) -> String {
    format!("dc-state:{}:{index}", id.0)
}

/// A field placed on an output page: (page index, PDF rectangle, field, border width, 0 when the
/// item has no stroke).
pub(crate) type Placed = (usize, [f64; 4], FormField, f64);

/// A placed video or sound on an output page: (page index, PDF rectangle, file name, MIME type,
/// bytes, options).
pub(crate) type PlacedMedia = (usize, [f64; 4], String, String, std::sync::Arc<Vec<u8>>, MediaOptions);

/// Every item of the exported pages that goes into the PDF, with its PDF rectangle (origin bottom
/// left).
fn visit(doc: &Document, sheets: &[Sheet], mut f: impl FnMut(usize, [f64; 4], &Item)) {
    for (pi, sh) in sheets.iter().enumerate() {
        for (xf, it) in crate::export::exported_items(doc, sh.spread) {
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
            let bw = if it.stroke.is_none() || !it.stroke.weight.is_finite() { 0.0 } else { it.stroke.weight };
            out.push((pi, r, f.clone(), bw));
        }
    });
    out
}

/// The state buttons of every exported page, and a warning for each button a PDF can't express:
/// Next or Previous State on an object with three or more states, or an action on an object that
/// isn't exported or is no longer multi-state (`items` lists what [`visit`] reaches).
pub(crate) fn collect_state_buttons(doc: &Document, sheets: &[Sheet]) -> (Vec<PlacedStateButton>, Vec<String>) {
    let mut states: HashMap<ItemId, usize> = HashMap::new();
    visit(doc, sheets, |_, _, it| {
        if !it.states.is_empty() {
            states.insert(it.id, it.states.len());
        }
    });
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    visit(doc, sheets, |pi, r, it| {
        let (object, switch) = match &it.button {
            Some(ButtonAction::GoToState { object, state }) => (*object, Some(StateSwitch::Show(*state))),
            Some(ButtonAction::GoToNextState { object, .. } | ButtonAction::GoToPreviousState { object, .. }) => (*object, None),
            _ => return,
        };
        let name = if it.name.is_empty() { format!("Button {}", it.id) } else { it.name.clone() };
        let Some(&count) = states.get(&object) else {
            warnings.push(format!("{name}: its multi-state object isn't on an exported page, so the button was left out"));
            return;
        };
        let switch = match switch {
            Some(StateSwitch::Show(i)) if i < count => StateSwitch::Show(i),
            Some(StateSwitch::Show(i)) => {
                warnings.push(format!("{name}: state {} doesn't exist, so the button was left out", i.saturating_add(1)));
                return;
            }
            None if count == 2 => StateSwitch::Toggle,
            _ => {
                warnings.push(format!(
                    "{name}: Go To Next/Previous State on an object with {count} states can't be expressed in PDF (only two states toggle), so the button was left out"
                ));
                return;
            }
        };
        out.push((pi, r, name, object, switch));
    });
    (out, warnings)
}

/// The optional content groups of the written PDF by name, as `n g R` references.
fn groups_by_name(pdf: &[u8]) -> HashMap<String, String> {
    use hayro_syntax::object::{Name, Object};
    let mut out = HashMap::new();
    let Ok(parsed) = hayro_syntax::Pdf::new(pdf.to_vec()) else { return out };
    for object in parsed.objects() {
        let Object::Dict(dict) = object else { continue };
        if dict.get::<Name<'_>>(b"Type").as_ref().map(AsRef::as_ref) != Some(b"OCG") {
            continue;
        }
        let (Some(name), Some(id)) = (dict.get::<hayro_syntax::object::String<'_>>(b"Name"), dict.obj_id()) else { continue };
        out.insert(String::from_utf8_lossy(name.as_bytes()).into_owned(), format!("{} {} R", id.obj_number, id.gen_number));
    }
    out
}

/// How many states a multi-state object may have in a PDF before the lookup stops.
const MAX_STATES: usize = 4096;

/// The objects that make page `page_ref` list the annotations `ids` after its own, `(object
/// number, body)`: the page itself with a new or longer inline `/Annots`, or its indirect
/// `/Annots` array re-stated with the ids added. Nothing for an empty `ids`.
fn page_annots(pdf: &[u8], page_ref: usize, ids: &[usize]) -> Option<Vec<(usize, String)>> {
    if ids.is_empty() {
        return Some(Vec::new());
    }
    let refs: String = ids.iter().map(|i| format!(" {i} 0 R")).collect();
    let body = String::from_utf8_lossy(&pdf[object(pdf, page_ref)?]).to_string();
    if let Some(a) = body.find("/Annots[") {
        let close = a + body[a..].find(']')?;
        return Some(vec![(page_ref, format!("{}{refs}{}", &body[..close], &body[close..]).trim().to_string())]);
    }
    if let Some(arr_id) = int_after(&body, "/Annots") {
        // An indirect array: re-state it with the annotations added.
        let arr = String::from_utf8_lossy(&pdf[object(pdf, arr_id)?]).to_string();
        let close = arr.rfind(']')?;
        return Some(vec![(arr_id, format!("{}{refs}{}", arr[..close].trim(), &arr[close..]).trim().to_string())]);
    }
    let close = body.rfind(">>")?;
    Some(vec![(page_ref, format!("{}/Annots[{}]{}", &body[..close], refs.trim(), &body[close..]).trim().to_string())])
}

/// The link annotation of each state button, `(page index, dictionary)`: its `SetOCGState`
/// action turns the object's state layers off and the chosen one on, or toggles the two of a
/// two-state object. Buttons whose layers aren't in `pdf` are skipped.
fn state_button_annotations(pdf: &[u8], buttons: &[PlacedStateButton], kids: &[usize]) -> Vec<(usize, String)> {
    if buttons.is_empty() {
        return Vec::new();
    }
    let groups = groups_by_name(pdf);
    let mut out = Vec::new();
    for (page, r, name, object, switch) in buttons {
        let Some(&page_ref) = kids.get(*page) else { continue };
        let refs: Vec<&str> = (0..MAX_STATES).map_while(|i| groups.get(&state_layer_name(*object, i)).map(String::as_str)).collect();
        let state = match switch {
            StateSwitch::Show(i) => {
                let Some(on) = refs.get(*i) else { continue };
                format!("/OFF {} /ON {on}", refs.join(" "))
            }
            StateSwitch::Toggle if refs.len() == 2 => format!("/Toggle {}", refs.join(" ")),
            StateSwitch::Toggle => continue,
        };
        out.push((
            *page,
            format!(
                "<</Type/Annot/Subtype/Link/Rect[{:.2} {:.2} {:.2} {:.2}]/P {page_ref} 0 R/F 4/Border[0 0 0]/Contents{}/A<</S/SetOCGState/State[{state}]/PreserveRB false>>>>",
                r[0],
                r[1],
                r[2],
                r[3],
                pdf_string(name)
            ),
        ));
    }
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

/// A PDF name's characters, with everything outside the regular set written `#xx`.
///
/// ISO 32000-1 7.3.5: a name may hold only the regular characters, and `#` introduces a two-digit
/// hex escape. A media type comes from a placed file's own metadata, and only `/` was escaped, so
/// a type holding a space, a delimiter or `#` -- `video/mp4;x=(a)` -- wrote a name that ends
/// early and leaves the rest as stray tokens in the PDF's structure (#1582).
fn pdf_name(s: &str) -> String {
    let regular = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.' | b'_' | b'~' | b'*' | b'@' | b'$' | b'&' | b'!' | b'^' | b'|');
    let mut o = String::new();
    for b in s.bytes() {
        if regular(b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("#{b:02X}"));
        }
    }
    o
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

/// A checkbox appearance's border: the item's stroke weight, inset by half of it so it stays
/// inside the box; nothing for an unstroked item.
fn checkbox_border(bw: f64, w: f64, h: f64) -> String {
    if bw > 0.0 {
        format!("0.45 G {bw:.2} w {i:.2} {i:.2} {:.2} {:.2} re S", (w - bw).max(0.0), (h - bw).max(0.0), i = bw / 2.0)
    } else {
        String::new()
    }
}

/// Add `fields` and `media` to `pdf` (page indices in output order). `None` when the file's
/// structure isn't the simple kind this understands.
pub(crate) fn add_fields(pdf: &[u8], fields: &[Placed], media: &[PlacedMedia], buttons: &[PlacedStateButton]) -> Option<Vec<u8>> {
    if fields.is_empty() && media.is_empty() && buttons.is_empty() {
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
    let kids_str = bracketed(kids_str)?;
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
    let mut embedded: std::collections::HashMap<(*const Vec<u8>, &str), usize> = std::collections::HashMap::new();
    for (page, r, name, mime, data, m) in media {
        let Some(&page_ref) = kids.get(*page) else { continue };
        // One embedded file per asset: placements of the same asset share it (a copy of the whole
        // asset per placement multiplied a large one by its placements).
        let file = *embedded.entry((std::sync::Arc::as_ptr(data), mime.as_str())).or_insert_with(|| {
            let file = next;
            next += 1;
            let mut body = format!("<</Type/EmbeddedFile/Subtype/{}/Length {}>>\nstream\n", pdf_name(mime), data.len()).into_bytes();
            body.extend_from_slice(data);
            body.extend_from_slice(b"\nendstream");
            put(&mut out, file, &body);
            file
        });
        let (spec, rendition, screen) = (next, next + 1, next + 2);
        next += 3;
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
    // State buttons: a link annotation whose action sets the layers of the object's states.
    for (page, body) in state_button_annotations(pdf, buttons, &kids) {
        let link = next;
        next += 1;
        put(&mut out, link, body.as_bytes());
        per_page[page].push(link);
    }
    let font = next;
    if !fields.is_empty() {
        next += 1;
        put(&mut out, font, b"<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>");
    }
    let mut all = Vec::new();
    for (page, r, f, bw) in fields {
        let Some(&page_ref) = kids.get(*page) else { continue };
        let rect = format!("[{:.2} {:.2} {:.2} {:.2}]", r[0], r[1], r[2], r[3]);
        let (w, h) = (r[2] - r[0], r[3] - r[1]);
        let fs = if f.font_size > 0.0 { f.font_size } else { 0.0 };
        let (bc, bs) = if *bw > 0.0 { ("/BC[0.45 0.45 0.45]", format!("/BS<</W {bw:.2}>>")) } else { ("", "/BS<</W 0>>".to_string()) };
        let common = format!(
            "/Type/Annot/Subtype/Widget/T{}/Rect{rect}/F 4/P {page_ref} 0 R/MK<<{bc}/BG[1 1 1]>>{bs}/DA(/Helv {fs:.1} Tf 0 g)",
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
                let border = checkbox_border(*bw, w, h);
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
    // Pages list their annotations.
    for (k, ids) in per_page.iter().enumerate() {
        for (id, body) in page_annots(pdf, kids[k], ids)? {
            put(&mut out, id, body.as_bytes());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A media type comes from a placed file's own metadata and was written into a PDF name with
    /// only `/` escaped, so a type holding a delimiter ended the name early and left the rest as
    /// tokens in the file's structure (#1582).
    #[test]
    fn a_pdf_name_escapes_every_irregular_byte() {
        assert_eq!(pdf_name("video/mp4"), "video#2Fmp4");
        assert_eq!(pdf_name("a b"), "a#20b");
        assert_eq!(pdf_name("x<</Type/Catalog>>"), "x#3C#3C#2FType#2FCatalog#3E#3E");
        assert_eq!(pdf_name("a%comment"), "a#25comment");
        assert_eq!(pdf_name("a#1"), "a#231");
        assert_eq!(pdf_name("a(b)"), "a#28b#29");
        assert_eq!(pdf_name("[1 0 R]"), "#5B1#200#20R#5D");
        assert_eq!(pdf_name("application-x_test.v2"), "application-x_test.v2");
        assert_eq!(pdf_name("\u{e9}"), "#C3#A9");
    }

    /// Placements of one asset share one embedded file (`/EmbeddedFile`), each with its own screen annotation.
    #[test]
    fn placements_of_one_asset_embed_it_once() {
        let mut pdf = b"%PDF-1.4\n1 0 obj\n<</Type/Catalog/Pages 2 0 R>>\nendobj\n2 0 obj\n<</Type/Pages/Kids[3 0 R]/Count 1>>\nendobj\n3 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]>>\nendobj\n".to_vec();
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 4\n0000000000 65535 f \ntrailer\n<</Size 4/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        let data = std::sync::Arc::new(vec![7u8; 5000]);
        let other = std::sync::Arc::new(vec![9u8; 3000]);
        let m = |d: &std::sync::Arc<Vec<u8>>| {
            (0, [0.0, 0.0, 10.0, 10.0], "a.mp4".to_string(), "video/mp4".to_string(), d.clone(), MediaOptions::default())
        };
        let media = [m(&data), m(&data), m(&data), m(&other)];
        let out = add_fields(&pdf, &[], &media).unwrap();
        let count = |needle: &[u8]| out.windows(needle.len()).filter(|w| *w == needle).count();
        assert_eq!(count(b"/Type/EmbeddedFile"), 2);
        assert_eq!(count(b"/Subtype/Screen"), 4);
        assert_eq!(count(&[7u8; 5000]), 1);
    }
}
