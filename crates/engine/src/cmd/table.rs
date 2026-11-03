//! Table menu: create, convert, insert/delete rows and columns, merge, cell/row/column options,
//! table options and cell selection.
//!
//! Commands target the table at the text selection (a caret in a cell or next to a table anchor),
//! the selected cells, or an explicit `{story, table}`; `rows: [a, b]` / `cols: [a, b]` (or `row` /
//! `col`) narrow the cell range (default: the selected cells, else the caret's cell, else the
//! whole table).

use designcraft_compose as compose;
use designcraft_doc::story::floor_char_boundary;
use designcraft_doc::{
    AltFills, CellAddr, CellRange, CellStroke, Document, RowHeightMode, RowKind, Selection, StoryId, StrokeType, Table, TableSel, TextSel,
    VerticalJustification,
};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, f64_or, has_doc, has_text, ok, str_param};
use crate::{Result, Session};

mod cell;
mod content;
mod options;
mod registry;
mod selection;
mod sizing;
mod structure;
mod styles;
mod target;

pub use registry::specs;
pub(crate) use selection::{one_selected_cell_to_text, step_cell};
pub use structure::{MAX_BODY_ROWS, MAX_COLUMNS, MAX_CONVERTED_CELLS, MAX_HEADER_FOOTER_ROWS};
