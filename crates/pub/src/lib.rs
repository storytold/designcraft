// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Microsoft Publisher (`.pub`) reading: [`read_publication`] decodes a Publisher 2002–2021
//! publication's structure, text store and drawing records with bounded readers and reports what
//! it could not decode. Turning it into a document follows in a later change.
//!
//! ## Format
//! A `.pub` file is an OLE2 compound file (MS-CFB). Publisher 2002 and later (all one storage
//! family, `Contents` header `E8 AC 2C 00`) keep:
//! - `Contents`: chunks found through a trailer directory by sequence number — the document
//!   (page size, page order), pages (their shapes, master names) and shapes (story ids).
//! - `Quill/QuillSub/CONTENTS`: the stories' UTF-16 text and character/paragraph formatting.
//! - `Escher/EscherStm` and `Escher/EscherDelayStm`: Office Drawing records (MS-ODRAW) with
//!   every shape's geometry, fill and line, and the pictures.
//!
//! Publisher 97/98/2000 files (header `E8 AC 22 00`) are recognised and refused with an
//! explanation.
//!
//! ## Provenance
//! The Contents, Quill and bounded-reader modules (`contents.rs`, `quill.rs`, `read.rs`) were
//! rewritten from scratch, without access to the earlier reader, from the format notes in
//! `docs/pub-format/` and the allowed sample files (the controlled Publisher samples, never any
//! other reader's source code). The notes reference Microsoft's public MS-CFB and MS-ODRAW
//! specifications. The drawing reader (`escher.rs`) follows MS-ODRAW. See
//! `docs/publisher-import.md` (Provenance). Publisher `truth/*.json` exports are used only to
//! score imports, never to infer format encoding.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

// The readers decode every field the conversion uses; until the conversion lands, only the
// summary below reads them.
#[allow(dead_code)]
mod contents;
#[allow(dead_code)]
mod escher;
#[allow(dead_code)]
mod quill;
#[allow(dead_code)]
mod read;
#[allow(dead_code)]
mod tables;

use std::io::Read;

#[derive(Debug, thiserror::Error)]
pub enum PubError {
    #[error("not a Microsoft Publisher file: {0}")]
    NotPub(String),
    #[error("unsupported Publisher file: {0}")]
    Unsupported(String),
    #[error("damaged Publisher file: data ends early")]
    Truncated,
    #[error("damaged Publisher file: {0}")]
    Malformed(String),
}

pub type Result<T> = std::result::Result<T, PubError>;

const CFB_SIGNATURE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const MAX_FILE: usize = 512 * 1024 * 1024;
const MAX_STREAM: u64 = 256 * 1024 * 1024;
const EMU_PER_PT: f64 = 12_700.0;

/// True for an OLE2 compound file holding a Publisher `Contents` stream.
pub fn is_pub(bytes: &[u8]) -> bool {
    bytes.starts_with(&CFB_SIGNATURE)
        && cfb::CompoundFile::open(std::io::Cursor::new(bytes)).is_ok_and(|mut cf| {
            let mut head = [0u8; 2];
            cf.open_stream("/Contents").is_ok_and(|mut s| s.read_exact(&mut head).is_ok() && head == [0xe8, 0xac])
        })
}

fn stream(cf: &mut cfb::CompoundFile<std::io::Cursor<&[u8]>>, path: &str) -> Result<Option<Vec<u8>>> {
    if !cf.is_stream(path) {
        return Ok(None);
    }
    let s = cf.open_stream(path).map_err(|e| PubError::Malformed(format!("{path}: {e}")))?;
    let mut out = Vec::new();
    s.take(MAX_STREAM + 1).read_to_end(&mut out).map_err(|e| PubError::Malformed(format!("{path}: {e}")))?;
    if out.len() as u64 > MAX_STREAM {
        return Err(PubError::Unsupported(format!("{path} is larger than {MAX_STREAM} bytes")));
    }
    Ok(Some(out))
}

fn pt(emu: f64) -> f64 {
    emu / EMU_PER_PT
}

/// A decoded publication: its structure, text store and drawing records.
pub struct Publication {
    contents: contents::Contents,
    quill: quill::Quill,
    drawing: escher::Drawing,
}

impl Publication {
    /// Page width and height in points, as stored.
    pub fn page_size(&self) -> (f64, f64) {
        (pt(f64::from(self.contents.width_emu)), pt(f64::from(self.contents.height_emu)))
    }
    /// Ordinary pages (masters and the scratch-area placeholders are not counted).
    pub fn page_count(&self) -> usize {
        self.contents.pages.len()
    }
    /// Master pages.
    pub fn master_count(&self) -> usize {
        self.contents.masters.len()
    }
    /// Stories in the text store.
    pub fn story_count(&self) -> usize {
        self.quill.stories.len()
    }
    /// A story's text, UTF-16 decoded as stored (paragraphs end with CR).
    pub fn story_text(&self, id: u32) -> Option<String> {
        self.quill.stories.get(&id).map(|s| String::from_utf16_lossy(&s.units))
    }
    /// Story ids, in ascending order.
    pub fn story_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.quill.stories.keys().copied().collect();
        ids.sort_unstable();
        ids
    }
    /// Top-level drawing records (groups count once).
    pub fn shape_count(&self) -> usize {
        self.drawing.shapes.len()
    }
    /// Pictures in the drawing's picture store.
    pub fn picture_count(&self) -> usize {
        self.drawing.pictures.len()
    }
    /// What the readers could not decode, one line per kind.
    pub fn warnings(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for w in self.contents.warnings.iter().chain(&self.quill.warnings).chain(&self.drawing.warnings) {
            if !out.contains(w) {
                out.push(w.clone());
            }
        }
        out
    }
}

/// Read a Publisher publication's streams with bounded readers.
pub fn read_publication(bytes: &[u8]) -> Result<Publication> {
    if bytes.len() > MAX_FILE {
        return Err(PubError::Unsupported("the file is larger than 512 MB".into()));
    }
    if !bytes.starts_with(&CFB_SIGNATURE) {
        return Err(PubError::NotPub("not an OLE2 compound file".into()));
    }
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(bytes)).map_err(|e| PubError::Malformed(format!("compound file: {e}")))?;
    let contents = stream(&mut cf, "/Contents")?.ok_or_else(|| PubError::NotPub("no Contents stream".into()))?;
    match contents.get(..4) {
        Some([0xe8, 0xac, 0x2c, 0x00]) => {}
        Some([0xe8, 0xac, 0x22, 0x00]) => {
            return Err(PubError::Unsupported(
                "Publisher 97/98/2000 files are not supported yet; open and save the file in Publisher 2002 or later".into(),
            ));
        }
        _ => return Err(PubError::NotPub("unrecognised Contents header".into())),
    }
    let quill = stream(&mut cf, "/Quill/QuillSub/CONTENTS")?.ok_or_else(|| PubError::Malformed("no Quill text stream".into()))?;
    let escher = stream(&mut cf, "/Escher/EscherStm")?.ok_or_else(|| PubError::Malformed("no Escher drawing stream".into()))?;
    let delay = stream(&mut cf, "/Escher/EscherDelayStm")?.unwrap_or_default();

    let c = contents::parse(&contents)?;
    let mut q = quill::parse(&quill)?;
    tables::recover_single_table_text(&c, &mut q, &quill)?;
    let dr = escher::parse(&escher, &delay)?;
    Ok(Publication { contents: c, quill: q, drawing: dr })
}
