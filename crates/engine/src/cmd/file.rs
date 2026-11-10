//! File menu: new, open, save, place, export, close.

use std::sync::Arc;

use designcraft_doc::build::{NewDocument, PRESETS};
use designcraft_doc::{Asset, AssetId, Content, Document, Graphic, Item, ItemId, PdfCrop, Selection, Shape};
use designcraft_geom::{Affine, Rect, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, f64_or, has_doc, ok, str_param};
use crate::{DocState, EngineError, Result, Session};

mod bytes;
mod new;
mod open;
mod place;
mod registry;
mod save;
mod snippets;

pub use bytes::{DOCUMENT_SNIFF_BYTES, base64_decode, base64_encode, from_bytes, is_document, to_bytes, with_document_extension};
pub(crate) use open::load_document_fonts;
pub use open::{RecentPreview, recent_preview};
pub(crate) use place::{crop_pdf, read_source, validate_placement};
pub use registry::specs;
pub(crate) use snippets::{snippet_bytes, snippet_place};

#[cfg(test)]
thread_local! {
    /// Makes [`to_bytes`] fail on this thread, standing in for writer errors that need gigabytes to reach.
    pub(crate) static FAIL_TO_BYTES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) mod eps_place_tests;
