//! Page transitions (`/Trans`) for interactive PDF. krilla doesn't write them, so they're added
//! as an incremental update: each page with a transition is re-stated with a `/Trans` entry, with
//! a new cross-reference section pointing back at the original.

use designcraft_doc::{PageTransition, TransitionKind};
use std::collections::HashMap;

/// The `/Trans` dictionary for a transition.
fn trans_dict(t: &PageTransition) -> String {
    use TransitionKind as K;
    let dm = if t.horizontal { "/H" } else { "/V" };
    let m = if t.horizontal { "/I" } else { "/O" };
    let body = match t.kind {
        K::Blinds => format!("/S/Blinds/Dm{dm}"),
        K::Box => format!("/S/Box/M{m}"),
        K::Comb => "/S/Glitter/Di 0".into(),
        K::Cover => "/S/Cover/Di 0".into(),
        K::Dissolve => "/S/Dissolve".into(),
        K::Fade => "/S/Fade".into(),
        K::Push => "/S/Push/Di 0".into(),
        K::Split => format!("/S/Split/Dm{dm}/M{m}"),
        K::Uncover => "/S/Uncover/Di 0".into(),
        K::Wipe => "/S/Wipe/Di 0".into(),
        K::ZoomIn => "/S/Fly/M/I/SS 0.01".into(),
        K::ZoomOut => "/S/Fly/M/O/SS 0.01".into(),
    };
    format!("/Trans<</Type/Trans{body}/D {:.2}>>", t.duration.clamp(0.0, 60.0))
}

pub(crate) fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

pub(crate) fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

/// The integer after `key` in `s` (e.g. `/Size 14`, `/Root 13 0 R`).
pub(crate) fn int_after(s: &str, key: &str) -> Option<usize> {
    let i = s.find(key)? + key.len();
    let rest = s[i..].trim_start();
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// The text between the first `[` and the first `]` after it. `None` when there is no such pair (a `]`
/// before the `[` included).
pub(crate) fn bracketed(s: &str) -> Option<&str> {
    let open = s.find('[')?;
    let close = s[open..].find(']')? + open;
    s.get(open + 1..close)
}

/// Cross-reference sections followed at most (`/Prev`): the file krilla writes, then the updates
/// this crate appends.
const MAX_XREF_SECTIONS: usize = 64;

/// Where the objects of `pdf` start, as its classic cross-reference sections say (the newest
/// first, following `/Prev`; the first entry seen for an object wins, a free entry included).
/// This is where a reader finds an object, so bytes inside a stream or a string that look like an
/// object header (an attached file, a form value) can't be taken for one.
pub(crate) struct Xref(HashMap<usize, Option<usize>>);

impl Xref {
    /// `None` when the file has no classic table at `startxref` (`xref`, entries, `trailer`).
    pub(crate) fn read(pdf: &[u8]) -> Option<Self> {
        let sx = rfind(pdf, b"startxref")?;
        let mut next = int_after(&String::from_utf8_lossy(pdf.get(sx..)?), "startxref");
        let mut offsets: HashMap<usize, Option<usize>> = HashMap::new();
        for _ in 0..MAX_XREF_SECTIONS {
            let Some(at) = next else { return Some(Self(offsets)) };
            let section = pdf.get(at..)?;
            let t = find(section, b"trailer", 0)?;
            let mut tokens = std::str::from_utf8(section.get(..t)?).ok()?.split_whitespace();
            if tokens.next()? != "xref" {
                return None;
            }
            while let Some(first) = tokens.next() {
                let first: usize = first.parse().ok()?;
                let count: usize = tokens.next()?.parse().ok()?;
                for i in 0..count {
                    let (offset, _generation, kind) = (tokens.next()?, tokens.next()?, tokens.next()?);
                    let n = first.checked_add(i)?;
                    let offset = match kind {
                        "n" => Some(offset.parse().ok()?),
                        "f" => None,
                        _ => return None,
                    };
                    offsets.entry(n).or_insert(offset);
                }
            }
            let end = find(section, b"startxref", t)?;
            next = int_after(&String::from_utf8_lossy(section.get(t..end)?), "/Prev");
        }
        None
    }
}

/// Byte range of object `id`'s body (between `obj` and `endobj`) where `xref` places it; `None`
/// when the object is free, unknown, or its header isn't there.
pub(crate) fn object(pdf: &[u8], xref: &Xref, id: usize) -> Option<std::ops::Range<usize>> {
    let start = (*xref.0.get(&id)?)?;
    let head = format!("{id} 0 obj");
    let body = start.checked_add(head.len())?;
    if pdf.get(start..body)? != head.as_bytes() {
        return None;
    }
    let end = find(pdf, b"endobj", body)?;
    Some(body..end)
}

/// The page objects of `pdf` in order (a flat page tree under the catalog `root`).
pub(crate) fn page_kids(pdf: &[u8], xref: &Xref, root: usize) -> Option<Vec<usize>> {
    let cat = String::from_utf8_lossy(pdf.get(object(pdf, xref, root)?)?).to_string();
    let pages_id = int_after(&cat, "/Pages")?;
    let pages = String::from_utf8_lossy(pdf.get(object(pdf, xref, pages_id)?)?).to_string();
    let kids_str = pages.get(pages.find("/Kids")? + 5..)?;
    let kids_str = bracketed(kids_str)?;
    let nums: Vec<usize> = kids_str.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    Some(nums.chunks(2).map(|c| c[0]).collect())
}

/// The object number of the first page of `pdf` (the first `/Kids` entry of its page tree).
pub fn first_page_ref(pdf: &[u8]) -> Option<usize> {
    let xref = Xref::read(pdf)?;
    let trailer = String::from_utf8_lossy(pdf.get(rfind(pdf, b"trailer")?..)?).to_string();
    page_kids(pdf, &xref, int_after(&trailer, "/Root")?)?.first().copied()
}

/// Add transitions to the pages of `pdf` (index = PDF page). `None` when the file's structure
/// isn't the simple kind this understands (classic xref table, a flat page tree).
pub fn add_transitions(pdf: &[u8], trans: &[Option<PageTransition>]) -> Option<Vec<u8>> {
    add_page_entries(pdf, &trans.iter().map(|t| t.as_ref().map(trans_dict)).collect::<Vec<_>>())
}

/// Transparency blend space: pages with transparency get a page group in DeviceCMYK or DeviceRGB
/// (`pages[i]` true = PDF page i).
pub fn add_blend_space(pdf: &[u8], pages: &[bool], cmyk: bool) -> Option<Vec<u8>> {
    let cs = if cmyk { "DeviceCMYK" } else { "DeviceRGB" };
    add_page_entries(pdf, &pages.iter().map(|on| on.then(|| format!("/Group<</Type/Group/S/Transparency/CS/{cs}>>"))).collect::<Vec<_>>())
}

/// Append dictionary entries to pages (index = PDF page) as an incremental update.
pub fn add_page_entries(pdf: &[u8], extra: &[Option<String>]) -> Option<Vec<u8>> {
    if extra.iter().all(Option::is_none) {
        return Some(pdf.to_vec());
    }
    let t_at = rfind(pdf, b"trailer")?;
    let trailer = String::from_utf8_lossy(&pdf[t_at..]).to_string();
    let size = int_after(&trailer, "/Size")?;
    let root = int_after(&trailer, "/Root")?;
    let prev = int_after(&trailer, "startxref")?;
    let info = int_after(&trailer, "/Info");
    let id = trailer.find("/ID").and_then(|i| trailer[i..].find(']').map(|j| trailer[i..i + j + 1].to_string()));
    let xref = Xref::read(pdf)?;
    let kids = page_kids(pdf, &xref, root)?;
    let mut out = pdf.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let mut entries: Vec<(usize, usize)> = Vec::new();
    // A page listed twice is re-stated once: a second copy would only grow the file.
    let mut done = std::collections::HashSet::new();
    for (i, t) in extra.iter().enumerate() {
        let (Some(t), Some(&kid)) = (t, kids.get(i)) else { continue };
        if !done.insert(kid) {
            continue;
        }
        let body = String::from_utf8_lossy(&pdf[object(pdf, &xref, kid)?]).to_string();
        if !body.contains("/Type/Page") && !body.contains("/Type /Page") {
            return None;
        }
        let close = body.rfind(">>")?;
        let new_body = format!("{}{}{}", &body[..close], t, &body[close..]);
        entries.push((kid, out.len()));
        out.extend_from_slice(format!("{kid} 0 obj{new_body}endobj\n").as_bytes());
    }
    let xref_at = out.len();
    let mut x = String::from("xref\n");
    for (kid, off) in &entries {
        x.push_str(&format!("{kid} 1\n{off:010} 00000 n\r\n"));
    }
    let info = info.map(|i| format!("/Info {i} 0 R")).unwrap_or_default();
    let id = id.unwrap_or_default();
    x.push_str(&format!("trailer\n<</Size {size}/Root {root} 0 R{info}{id}/Prev {prev}>>\nstartxref\n{xref_at}\n%%EOF\n"));
    out.extend_from_slice(x.as_bytes());
    Some(out)
}

/// Add `entries` (raw dictionary entries, e.g. `/PageMode/FullScreen`) to the catalog as an
/// incremental update.
pub fn add_catalog_entries(pdf: &[u8], entries: &str) -> Option<Vec<u8>> {
    if entries.is_empty() {
        return Some(pdf.to_vec());
    }
    let t_at = rfind(pdf, b"trailer")?;
    let trailer = String::from_utf8_lossy(&pdf[t_at..]).to_string();
    let size = int_after(&trailer, "/Size")?;
    let root = int_after(&trailer, "/Root")?;
    let prev = int_after(&trailer, "startxref")?;
    let info = int_after(&trailer, "/Info").map(|i| format!("/Info {i} 0 R")).unwrap_or_default();
    let id = trailer.find("/ID").and_then(|i| trailer[i..].find(']').map(|j| trailer[i..i + j + 1].to_string())).unwrap_or_default();
    let xref = Xref::read(pdf)?;
    let cat = String::from_utf8_lossy(&pdf[object(pdf, &xref, root)?]).to_string();
    let close = cat.rfind(">>")?;
    let mut out = pdf.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let at = out.len();
    out.extend_from_slice(format!("{root} 0 obj\n{}{entries}{}\nendobj\n", cat[..close].trim(), cat[close..].trim()).as_bytes());
    let xref_at = out.len();
    out.extend_from_slice(
        format!("xref\n{root} 1\n{at:010} 00000 n\r\ntrailer\n<</Size {size}/Root {root} 0 R{info}{id}/Prev {prev}>>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    Some(out)
}

#[cfg(test)]
mod bracket_tests {
    use super::{add_page_entries, bracketed};

    #[test]
    fn bracketed_ignores_a_close_before_the_open() {
        assert_eq!(bracketed(" [3 0 R 4 0 R] "), Some("3 0 R 4 0 R"));
        assert_eq!(bracketed("] ["), None);
        assert_eq!(bracketed("] [3 0 R]"), Some("3 0 R"));
        assert_eq!(bracketed("[]"), Some(""));
    }

    /// `]` before the `[` of /Kids (a spoofed page tree) is refused, not a slice panic.
    #[test]
    fn kids_closed_before_opened_is_refused() {
        let objects = ["<</Type/Catalog/Pages 2 0 R>>", "<</Type/Pages/Kids ] [3 0 R/Count 1>>"];
        let mut out = b"%PDF-1.6\n".to_vec();
        for (i, o) in objects.iter().enumerate() {
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        out.extend_from_slice(b"trailer\n<</Size 3/Root 1 0 R>>\nstartxref\n0\n%%EOF\n");
        assert!(add_page_entries(&out, &[Some("/Rotate 0".to_string())]).is_none());
    }
}
