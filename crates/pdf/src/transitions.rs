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

/// Byte range of object `id`'s body (between `obj` and `endobj`).
pub(crate) fn object(pdf: &[u8], id: usize) -> Option<std::ops::Range<usize>> {
    // The latest copy (incremental updates re-state objects further on).
    let head = format!("\n{id} 0 obj");
    let start = rfind(pdf, head.as_bytes())? + head.len();
    let end = find(pdf, b"endobj", start)?;
    Some(start..end)
}

// Bound optional indexing metadata. Larger requests retain the original lookup path.
const MAX_INDEXED_PAGE_OBJECTS: usize = 4096;

/// Latest literal object headers for the requested IDs, found in a single reverse pass.
/// Match `object` exactly, including embedded headers and the lack of a boundary after `obj`.
/// Only requested IDs are retained; neither PDF size nor the largest ID sizes this map.
/// If the metadata limit or an allocation failure prevents indexing, use `object` instead.
fn object_starts(pdf: &[u8], ids: impl Iterator<Item = usize>) -> Option<HashMap<usize, Option<usize>>> {
    let mut starts = HashMap::new();
    for id in ids {
        if starts.contains_key(&id) {
            continue;
        }
        if starts.len() == MAX_INDEXED_PAGE_OBJECTS || starts.try_reserve(1).is_err() {
            return None;
        }
        starts.insert(id, None);
    }
    let mut missing = starts.len();
    if missing == 0 {
        return Some(starts);
    }
    let mut end = pdf.len();
    while let Some(at) = pdf.get(..end).and_then(|bytes| bytes.iter().rposition(|b| *b == b'\n')) {
        let Some(start) = at.checked_add(1) else { break };
        let Some(line) = pdf.get(start..end) else { break };
        end = at;
        let digits = line.iter().position(|b| !b.is_ascii_digit()).unwrap_or(line.len());
        let Some(number) = line.get(..digits) else { continue };
        if number.is_empty() || (number.len() > 1 && number.first() == Some(&b'0')) {
            continue;
        }
        if !line.get(digits..).is_some_and(|tail| tail.starts_with(b" 0 obj")) {
            continue;
        }
        let Some(id) = number.iter().try_fold(0usize, |n, b| n.checked_mul(10)?.checked_add(usize::from(*b - b'0'))) else { continue };
        let Some(slot) = starts.get_mut(&id) else { continue };
        if slot.is_some() {
            continue;
        }
        let Some(body) = start.checked_add(digits).and_then(|n| n.checked_add(6)) else { continue };
        *slot = Some(body);
        missing -= 1;
        if missing == 0 {
            break;
        }
    }
    Some(starts)
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
    let cat = String::from_utf8_lossy(&pdf[object(pdf, root)?]).to_string();
    let pages_id = int_after(&cat, "/Pages")?;
    let pages = String::from_utf8_lossy(&pdf[object(pdf, pages_id)?]).to_string();
    let kids_at = pages.find("/Kids")?;
    let kids_str = &pages[kids_at + 5..];
    let kids_str = &kids_str[kids_str.find('[')? + 1..kids_str.find(']')?];
    let nums: Vec<usize> = kids_str.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    let kids: Vec<usize> = nums.chunks(2).map(|c| c[0]).collect();
    let starts = object_starts(pdf, extra.iter().zip(&kids).filter_map(|(t, &kid)| t.as_ref().map(|_| kid)));
    let mut out = pdf.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let mut entries: Vec<(usize, usize)> = Vec::new();
    for (i, t) in extra.iter().enumerate() {
        let (Some(t), Some(&kid)) = (t, kids.get(i)) else { continue };
        let range = if let Some(starts) = &starts {
            let start = starts.get(&kid).copied().flatten()?;
            start..find(pdf, b"endobj", start)?
        } else {
            object(pdf, kid)?
        };
        let body = String::from_utf8_lossy(&pdf[range]).to_string();
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
    let cat = String::from_utf8_lossy(&pdf[object(pdf, root)?]).to_string();
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
mod object_index_tests {
    use super::{MAX_INDEXED_PAGE_OBJECTS, object_starts};

    #[test]
    fn metadata_limit_disables_only_the_optional_index() {
        let at_limit = object_starts(b"", 0..MAX_INDEXED_PAGE_OBJECTS).unwrap();
        assert_eq!(at_limit.len(), MAX_INDEXED_PAGE_OBJECTS);
        assert!(object_starts(b"", 0..=MAX_INDEXED_PAGE_OBJECTS).is_none());
        // Duplicate requests do not consume the unique-ID budget.
        let starts = object_starts(b"\n0 0 objendobj", std::iter::repeat_n(0, MAX_INDEXED_PAGE_OBJECTS + 1)).unwrap();
        assert_eq!(starts.len(), 1);
        assert_eq!(starts.get(&0), Some(&Some(8)));
    }
}
