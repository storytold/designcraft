//! A PostScript interpreter for placed EPS files: runs the program and writes what it paints as
//! a one-page PDF the size of its bounding box, in vector form with its colours kept (CMYK stays
//! CMYK), so it displays sharply and prints as it was drawn.
//!
//! Written from the PostScript Language Reference (third edition) and the EPS specification.
//! Covered: the language (Level 3 operators EPS files use), paths, painting, clipping, device and
//! Separation/DeviceN/Indexed colour, sampled images with the standard filters, smooth shadings
//! (axial, radial, function-based, triangle and patch meshes), shading and tiling patterns, and
//! text in Type 1, Type 3, Type 42 and CFF fonts (`StartData`), CIDFonts and composite fonts
//! through CMaps, written horizontally or vertically.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod cff;
mod cff_strings;
mod composite;
mod encodings;
mod font;
mod gstate;
mod interp;
mod obj;
mod ops;
mod pdf;
mod scan;
mod source;
mod type1;
mod type42;

pub use interp::Limits;

/// A converted EPS.
#[derive(Debug)]
pub struct Converted {
    /// A one-page PDF of the bounding box.
    pub pdf: Vec<u8>,
    /// The bounding box in points (x0, y0, x1, y1).
    pub bbox: [f64; 4],
    /// Things the program did that weren't drawn (unsupported features).
    pub warnings: Vec<String>,
    /// The work and memory the conversion used, as [`Limits`] counts them.
    pub steps: u64,
    pub memory: usize,
}

fn u32le(b: &[u8], at: usize) -> Option<usize> {
    let s: [u8; 4] = b.get(at..at.checked_add(4)?)?.try_into().ok()?;
    usize::try_from(u32::from_le_bytes(s)).ok()
}

/// The PostScript part of an EPS (inside a DOS EPS binary header, when there is one).
pub fn postscript_section(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6]) {
        return u32le(bytes, 4).zip(u32le(bytes, 8)).and_then(|(o, l)| bytes.get(o..o.checked_add(l)?)).unwrap_or(&[]);
    }
    bytes
}

/// The bounding box from the DSC comments: the high-resolution one when it is valid.
pub fn bounding_box(ps: &[u8]) -> Option<[f64; 4]> {
    let head = String::from_utf8_lossy(ps.get(..ps.len().min(256 * 1024))?);
    let read = |key: &str| {
        head.lines().find_map(|l| {
            let v: Vec<f64> = l.strip_prefix(key)?.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            let [x0, y0, x1, y1] = <[f64; 4]>::try_from(v).ok()?;
            let ok = [x0, y0, x1, y1, x1 - x0, y1 - y0].iter().all(|c| c.is_finite());
            (ok && x1 > x0 && y1 > y0).then_some([x0, y0, x1, y1])
        })
    };
    read("%%HiResBoundingBox:").or_else(|| read("%%BoundingBox:"))
}

/// An EPS file (plain, or DOS EPS with a binary header)?
pub fn is_eps(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6])
        || (bytes.starts_with(b"%!PS-Adobe") && bytes.get(..bytes.len().min(64)).is_some_and(|h| h.windows(4).any(|w| w == b"EPSF")))
}

/// How many converted EPS files are kept.
const CACHE_ENTRIES: usize = 64;

type Cache = std::collections::HashMap<(u64, usize), Option<std::sync::Arc<Vec<u8>>>>;

fn cache() -> &'static std::sync::Mutex<Cache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Cache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(Cache::new()))
}

/// [`eps_to_pdf`] with the default limits, remembered by content: the screen, thumbnails and
/// every export convert an EPS once. None when it can't be converted (callers show its preview).
pub fn cached_pdf(eps: &[u8]) -> Option<std::sync::Arc<Vec<u8>>> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    eps.hash(&mut h);
    let key = (h.finish(), eps.len());
    if let Some(hit) = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
        return hit.clone();
    }
    // Convert outside the lock: other EPS files keep drawing meanwhile.
    let pdf = eps_to_pdf(eps, Limits::default()).ok().map(|c| std::sync::Arc::new(c.pdf));
    let mut c = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if c.len() >= CACHE_ENTRIES {
        c.clear();
    }
    c.insert(key, pdf.clone());
    pdf
}

/// Run an EPS and write what it paints as PDF. `Err` when it isn't an EPS with a bounding box,
/// when the program fails before painting anything, or when it exceeds `limits`.
pub fn eps_to_pdf(bytes: &[u8], limits: Limits) -> Result<Converted, String> {
    let ps = postscript_section(bytes);
    if !ps.starts_with(b"%!") {
        return Err("not PostScript".into());
    }
    let bbox = bounding_box(ps).ok_or("the EPS has no bounding box")?;
    let mut ip = interp::Interp::new(ps.to_vec(), (bbox[0], bbox[1]), limits);
    let result = ip.run();
    let mut warnings = std::mem::take(&mut ip.warnings);
    warnings.dedup();
    if let Err(e) = result {
        // What was painted before an error is kept (the rest of the page is lost, as on a
        // printer); with nothing painted, the caller falls back to the preview.
        if !ip.out.painted {
            return Err(e);
        }
        warnings.push(e);
    }
    let size = (bbox[2] - bbox[0], bbox[3] - bbox[1]);
    let (steps, memory) = (ip.steps, ip.memory);
    Ok(Converted { pdf: ip.out.finish(size), bbox, warnings, steps, memory })
}

#[cfg(test)]
mod tests;
