//! PageMaker import (File › Open of `.pmd`, `.p65`, `.pm6`, `.pm5`, `.pm4`, `.pm3` and their
//! templates): Aldus/Adobe PageMaker documents, PC and Mac, into a [`Document`].
//!
//! Clean-room: the format was worked out from sample files (see `FORMAT.md`); libpagemaker was
//! used only as a black box to compare results, never read.
//!
//! PageMaker 6–7 files are an OLE compound file with one `PageMaker` stream; that stream, like a
//! whole PageMaker 4–5 file, starts with a header whose bytes 6–7 are `FF 99` (PC, little-endian)
//! or `99 FF` (Mac, big-endian) and whose version code says how its records are laid out.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod build;
mod bytes;
mod cfb;
mod images;
mod model;
mod parse;
mod records;
#[cfg(any(test, feature = "testing"))]
#[doc(hidden)]
pub mod testfile;
#[cfg(test)]
mod tests;

use designcraft_doc::Document;

/// File name extensions of PageMaker documents and templates.
pub const EXTENSIONS: &[&str] = &["pmd", "pmt", "p65", "t65", "pm6", "pt6", "pm5", "pt5", "pm4", "pt4", "pm3", "pt3", "pagemaker"];

/// The largest file imported.
const MAX_FILE: usize = 512 << 20;

/// A PageMaker version, as its file format tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    Pm3,
    Pm4,
    Pm5,
    /// PageMaker 6.0.
    Pm6,
    /// PageMaker 6.5 and 7.0 (one format).
    Pm65,
}

impl Version {
    pub fn label(self) -> &'static str {
        match self {
            Version::Pm3 => "PageMaker 3",
            Version::Pm4 => "PageMaker 4",
            Version::Pm5 => "PageMaker 5",
            Version::Pm6 => "PageMaker 6",
            Version::Pm65 => "PageMaker 6.5/7",
        }
    }
}

/// What kind of PageMaker file some bytes are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    pub version: Version,
    /// Written on the Mac (big-endian).
    pub mac: bool,
    /// Wrapped in an OLE compound file (PageMaker 6 and later).
    pub ole: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PmError {
    NotPageMaker,
    Unsupported(String),
    Corrupt(String),
}

impl std::fmt::Display for PmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PmError::NotPageMaker => write!(f, "not a PageMaker document"),
            PmError::Unsupported(m) => write!(f, "unsupported PageMaker document: {m}"),
            PmError::Corrupt(m) => write!(f, "damaged PageMaker document: {m}"),
        }
    }
}

impl std::error::Error for PmError {}

/// An imported document and what didn't come across.
pub struct Imported {
    pub document: Document,
    pub warnings: Vec<String>,
    pub kind: Kind,
}

/// The PageMaker stream: the `PageMaker` stream of a compound file, or the bytes themselves.
fn stream(bytes: &[u8]) -> Option<(std::borrow::Cow<'_, [u8]>, bool)> {
    if cfb::is_cfb(bytes) {
        return cfb::stream(bytes, "PageMaker").map(|s| (std::borrow::Cow::Owned(s), true));
    }
    Some((std::borrow::Cow::Borrowed(bytes), false))
}

/// The version of a PageMaker stream from its header.
fn version_of(s: &[u8]) -> Option<(Version, bool)> {
    let mac = match s.get(6..8)? {
        [0xFF, 0x99] => false,
        [0x99, 0xFF] => true,
        _ => return None,
    };
    let b = bytes::Bytes::new(s, !mac);
    let code = b.u16(10)?;
    let version = match code {
        0x0201..=0x0204 => Version::Pm4,
        0x0205..=0x0208 => Version::Pm5,
        0x0209..=0x020F => {
            if b.u16(42)? <= 50 {
                Version::Pm6
            } else {
                Version::Pm65
            }
        }
        // PageMaker 3 files carry a small version number here.
        3..=0x00FF => Version::Pm3,
        // Anything else that happens to have the marker isn't a PageMaker file we know.
        _ => return None,
    };
    Some((version, mac))
}

/// What PageMaker file `bytes` is, if it is one.
pub fn detect(bytes: &[u8]) -> Option<Kind> {
    let (s, ole) = stream(bytes)?;
    let (version, mac) = version_of(&s)?;
    Some(Kind { version, mac, ole })
}

/// Whether `bytes` are a PageMaker document (by their content, whatever the name).
pub fn is_pagemaker(bytes: &[u8]) -> bool {
    detect(bytes).is_some()
}

/// Whether a file name has a PageMaker extension.
pub fn has_pagemaker_extension(name: &str) -> bool {
    std::path::Path::new(name).extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Import a PageMaker document.
pub fn import_pagemaker(bytes: &[u8]) -> Result<Imported, PmError> {
    if bytes.len() > MAX_FILE {
        return Err(PmError::Unsupported(format!("the file is larger than {} MB", MAX_FILE >> 20)));
    }
    let (s, ole) = stream(bytes).ok_or(PmError::NotPageMaker)?;
    let (version, mac) = version_of(&s).ok_or(PmError::NotPageMaker)?;
    let pm = match version {
        Version::Pm3 => return Err(PmError::Unsupported(format!("{} documents can't be opened yet", version.label()))),
        _ => parse::parse(&s, version, mac)?,
    };
    let (document, warnings) = build::build(&pm)?;
    Ok(Imported { document, warnings, kind: Kind { version, mac, ole } })
}

/// A plain-text summary of what the importer read (pages, objects, stories), for working on the
/// format; not part of the API.
#[doc(hidden)]
pub fn dump(bytes: &[u8]) -> Result<String, PmError> {
    use std::fmt::Write as _;
    let (s, _) = stream(bytes).ok_or(PmError::NotPageMaker)?;
    let (version, mac) = version_of(&s).ok_or(PmError::NotPageMaker)?;
    let pm = parse::parse(&s, version, mac)?;
    let mut out = String::new();
    let _ = writeln!(out, "{version:?} mac={mac} page {}x{} facing={} margins={:?}", pm.width, pm.height, pm.facing, pm.margins);
    let masters = [pm.masters.clone()];
    for (label, lists) in [("master", &masters[..]), ("page", &pm.pages[..])] {
        for (i, objs) in lists.iter().enumerate() {
            let _ = writeln!(out, "{label} {}: {} objects", i + 1, objs.len());
            for o in objs {
                let _ = writeln!(out, "   {:?} {:?} rot={:?}", o.kind, o.rect.map(|v| (v * 10.0).round() / 10.0), o.rotation);
            }
        }
    }
    for (id, st) in &pm.stories {
        let preview: String = st.text.chars().take(60).collect();
        let _ =
            writeln!(out, "story {id}: {} chars, {} char runs, {} para runs: {preview:?}", st.text.chars().count(), st.chars.len(), st.paras.len());
    }
    Ok(out)
}
