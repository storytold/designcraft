//! Object Layer Options for placed PDFs: the file's optional-content layers, and a variant of the
//! file with some layers off (an appended catalog whose default configuration turns them off,
//! which the PDF renderer honours).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use craft_pdf::syntax::Pdf;
use craft_pdf::syntax::object::{Array, Dict, Name, ObjectIdentifier};

/// (object number, name, on by default) of each layer.
fn ocgs(pdf: &Pdf) -> Vec<(i32, String, bool)> {
    let xref = pdf.xref();
    let Some(cat) = xref.get::<Dict<'_>>(xref.root_id()) else { return vec![] };
    let Some(ocp) = cat.get::<Dict<'_>>(b"OCProperties") else { return vec![] };
    let Some(list) = ocp.get::<Array<'_>>(b"OCGs") else { return vec![] };
    let cfg = ocp.get::<Dict<'_>>(b"D");
    let ids_in = |key: &[u8]| -> Vec<i32> {
        cfg.as_ref()
            .and_then(|c| c.get::<Array<'_>>(key))
            .map(|a| a.raw_iter().filter_map(|o| o.as_obj_ref()).map(|r| r.obj_number).collect())
            .unwrap_or_default()
    };
    let base_off = cfg.as_ref().and_then(|c| c.get::<Name<'_>>(b"BaseState")).is_some_and(|n| n.as_ref() == b"OFF");
    let (on, off) = (ids_in(b"ON"), ids_in(b"OFF"));
    list.raw_iter()
        .filter_map(|o| o.as_obj_ref())
        .filter_map(|r| {
            let d = xref.get::<Dict<'_>>(ObjectIdentifier::from(r))?;
            let name = d
                .get::<craft_pdf::syntax::object::String<'_>>(b"Name")
                .map(|s| String::from_utf8_lossy(s.as_bytes()).to_string())
                .unwrap_or_default();
            let visible = if base_off { on.contains(&r.obj_number) } else { !off.contains(&r.obj_number) };
            Some((r.obj_number, name, visible))
        })
        .collect()
}

/// The layers of a placed PDF: (name, visible by default).
pub fn pdf_layers(bytes: &[u8]) -> Vec<(String, bool)> {
    craft_pdf::open_slice(bytes, None).map(|p| ocgs(&p).into_iter().map(|(_, n, v)| (n, v)).collect()).unwrap_or_default()
}

/// `bytes` with exactly the layers named in `hidden` off (the others on). `None` when the file
/// has no such layers or can't be read.
pub fn pdf_hide_layers(bytes: &[u8], hidden: &[String]) -> Option<Vec<u8>> {
    let pdf = craft_pdf::open_slice(bytes, None).ok()?;
    let layers = ocgs(&pdf);
    if layers.is_empty() {
        return None;
    }
    let xref = pdf.xref();
    let cat = xref.get::<Dict<'_>>(xref.root_id())?;
    let pages = cat.get_ref(b"Pages")?;
    let all: Vec<String> = layers.iter().map(|(n, _, _)| format!("{n} 0 R")).collect();
    let off: Vec<String> = layers.iter().filter(|(_, name, _)| hidden.contains(name)).map(|(n, _, _)| format!("{n} 0 R")).collect();
    // A new catalog under a fresh object number, with the original's page tree.
    let root = 9_999_000;
    let mut out = bytes.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let at = out.len();
    out.extend_from_slice(
        format!(
            "{root} 0 obj\n<</Type/Catalog/Pages {} {} R/OCProperties<</OCGs[{}]/D<</BaseState/ON/OFF[{}]>>>>>>\nendobj\n",
            pages.obj_number,
            pages.gen_number,
            all.join(" "),
            off.join(" ")
        )
        .as_bytes(),
    );
    let xref_at = out.len();
    let prev = {
        let tail = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(1024)..]).to_string();
        tail.rsplit_once("startxref").and_then(|(_, t)| t.split_whitespace().next()).and_then(|n| n.parse::<usize>().ok())?
    };
    out.extend_from_slice(
        format!("xref\n{root} 1\n{at:010} 00000 n\r\ntrailer\n<</Size {}/Root {root} 0 R/Prev {prev}>>\nstartxref\n{xref_at}\n%%EOF\n", root + 1)
            .as_bytes(),
    );
    Some(out)
}

/// Shared copies of layer variants (by source address and hidden set).
pub(crate) fn layered(data: &Arc<Vec<u8>>, hidden: &[String]) -> Arc<Vec<u8>> {
    type Key = (usize, Vec<String>);
    static CACHE: Mutex<Option<HashMap<Key, Arc<Vec<u8>>>>> = Mutex::new(None);
    let key = (Arc::as_ptr(data) as usize, hidden.to_vec());
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = c.get_or_insert_with(HashMap::new);
    if let Some(v) = map.get(&key) {
        return v.clone();
    }
    if map.len() > 64 {
        map.clear();
    }
    let v = pdf_hide_layers(data, hidden).map(Arc::new).unwrap_or_else(|| data.clone());
    map.insert(key, v.clone());
    v
}
