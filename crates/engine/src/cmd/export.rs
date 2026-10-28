//! File › Export PDF (Print).

use designcraft_pdf::{Marks, PdfOptions, Standard};
use serde_json::{Value, json};
use std::{borrow::Cow, sync::Arc};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{EngineError, Result, Session};

mod flatten;
mod formats;
mod pdf;
mod registry;

#[cfg(test)]
use flatten::{rasterize_effects, shadow_coverage};
pub use pdf::export_pdf_document;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use pdf::pdf_export;
pub(crate) use pdf::{flatten_ppi, options, print_pdf};
pub use registry::specs;

#[cfg(test)]
mod text_tests;
