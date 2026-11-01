//! Layout menu and Pages/Layers panels: pages, spreads, parents, margins & columns, layers.

use designcraft_doc::{LAYER_COLORS, Layer, LayerId, Margins, SpreadId};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, ok, str_param};
use crate::{Result, Session};

mod layers;
mod margins;
mod pages;
mod parents;
mod registry;
mod sections;

pub(crate) use pages::resize_pages;
pub use registry::specs;
pub use sections::chapter_numbering;

#[cfg(test)]
#[path = "parent_delete_tests.rs"]
mod parent_delete_tests;
