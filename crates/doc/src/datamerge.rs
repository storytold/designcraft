//! Data merge: linked tables, placeholders, and the last merge options.
//!
//! Each source keeps its own rows, filter, and sort. A drawn grid is stored on the item, not here.
//! Preview is session state and is not stored here.

use designcraft_geom::{BezPath, Point, Rect};
use serde::{Deserialize, Serialize};

use crate::{ItemId, StoryId};

/// How a column is read when a placeholder does not say otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DataFieldKind {
    #[default]
    Text,
    Image,
    Qr,
}

impl DataFieldKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DataFieldKind::Text => "text",
            DataFieldKind::Image => "image",
            DataFieldKind::Qr => "qr",
        }
    }
}

/// A column in a data source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataField {
    pub name: String,
    pub kind: DataFieldKind,
}

/// Text delimiter. Excel sources leave this unused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Delimiter {
    #[default]
    Comma,
    Tab,
    Semicolon,
}

impl Delimiter {
    pub fn as_str(self) -> &'static str {
        match self {
            Delimiter::Comma => "comma",
            Delimiter::Tab => "tab",
            Delimiter::Semicolon => "semicolon",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "comma" => Some(Delimiter::Comma),
            "tab" => Some(Delimiter::Tab),
            "semicolon" => Some(Delimiter::Semicolon),
            _ => None,
        }
    }

    pub fn char(self) -> char {
        match self {
            Delimiter::Comma => ',',
            Delimiter::Tab => '\t',
            Delimiter::Semicolon => ';',
        }
    }
}

/// File size and modification time of the last successful read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub size: u64,
    pub mtime: i64,
}

/// Whether the file on disk still matches the cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceStatus {
    #[default]
    Ok,
    Missing,
    Modified,
}

/// One rule in a source filter. `empty` and `notEmpty` ignore `value`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterRule {
    pub field: String,
    pub op: String,
    #[serde(default)]
    pub value: String,
}

/// Which rows of one source survive. Default keeps every row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFilter {
    /// `all` (every rule) or `any` (one rule).
    #[serde(default = "match_all", rename = "match")]
    pub match_mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<FilterRule>,
}

impl Default for SourceFilter {
    fn default() -> Self {
        SourceFilter { match_mode: match_all(), rules: Vec::new() }
    }
}

impl SourceFilter {
    pub fn is_default(&self) -> bool {
        self.rules.is_empty() && self.match_mode == "all"
    }
}

/// One sort key. `direction` is `asc` or `desc`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortKey {
    pub field: String,
    #[serde(default = "asc_dir")]
    pub direction: String,
}

fn match_all() -> String {
    "all".into()
}
fn asc_dir() -> String {
    "asc".into()
}
fn enabled_default() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
}

/// One linked table. Rows are the cache. Each row is the same length as `fields`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSource {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
    pub name: String,
    #[serde(default)]
    pub delimiter: Delimiter,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    pub fields: Vec<DataField>,
    pub rows: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<Fingerprint>,
    #[serde(default)]
    pub status: SourceStatus,
    /// Extra-cell notes from the last read, repeated on a later merge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// A disabled source stays linked and is skipped at merge time.
    #[serde(default = "enabled_default", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "SourceFilter::is_default")]
    pub filter: SourceFilter,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<SortKey>,
}

/// A drawn record grid. Children are the prototype, stored relative to the origin cell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataGrid {
    pub rows: u32,
    pub columns: u32,
    #[serde(default)]
    pub gutter: f64,
    #[serde(default)]
    pub record_offset: u32,
    #[serde(default = "one_u32")]
    pub record_advance: u32,
    #[serde(default = "top_left")]
    pub origin: String,
    #[serde(default = "arrange_rows")]
    pub arrange: String,
}

fn top_left() -> String {
    "topLeft".into()
}

impl Default for DataGrid {
    fn default() -> Self {
        DataGrid { rows: 2, columns: 2, gutter: 0.0, record_offset: 0, record_advance: 1, origin: top_left(), arrange: arrange_rows() }
    }
}

impl DataGrid {
    /// Cell rectangle in `bounds` space. Flow starts at the origin and walks `arrange`, reversing
    /// an axis when the corner is on the right or the bottom. `None` when the cell is not positive.
    pub fn cell_rect(&self, bounds: Rect, index: usize) -> Option<Rect> {
        if self.rows < 1 || self.columns < 1 {
            return None;
        }
        let (from_right, from_bottom) = match self.origin.as_str() {
            "topLeft" => (false, false),
            "topRight" => (true, false),
            "bottomLeft" => (false, true),
            "bottomRight" => (true, true),
            _ => return None,
        };
        if self.arrange != "rows" && self.arrange != "columns" {
            return None;
        }
        let columns = f64::from(self.columns);
        let rows = f64::from(self.rows);
        let width = (bounds.width() - self.gutter * (columns - 1.0)) / columns;
        let height = (bounds.height() - self.gutter * (rows - 1.0)) / rows;
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return None;
        }
        let primary = if self.arrange == "columns" { self.rows } else { self.columns } as usize;
        if primary == 0 {
            return None;
        }
        let a = index % primary;
        let b = index / primary;
        let (mut col, mut row) = if self.arrange == "columns" { (b, a) } else { (a, b) };
        if from_right {
            col = (self.columns as usize).saturating_sub(1).saturating_sub(col);
        }
        if from_bottom {
            row = (self.rows as usize).saturating_sub(1).saturating_sub(row);
        }
        let x = bounds.x0 + col as f64 * (width + self.gutter);
        let y = bounds.y0 + row as f64 * (height + self.gutter);
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        Some(Rect::new(x, y, x + width, y + height))
    }

    /// Top-left of the origin cell in `bounds` space.
    pub fn origin_point(&self, bounds: Rect) -> Option<Point> {
        self.cell_rect(bounds, 0).map(|rect| Point::new(rect.x0, rect.y0))
    }

    /// Interior cell edges in `bounds` space. The outer rectangle is the item stroke.
    pub fn divider_path(&self, bounds: Rect) -> BezPath {
        let mut path = BezPath::new();
        let Some(n) = self.rows.checked_mul(self.columns) else { return path };
        let n = (n as usize).min(500);
        for index in 0..n {
            let Some(cell) = self.cell_rect(bounds, index) else { continue };
            for (a, b) in cell_edges(cell) {
                if !edge_on_bounds(bounds, a, b) {
                    path.move_to(a);
                    path.line_to(b);
                }
            }
        }
        path
    }
}

fn cell_edges(cell: Rect) -> [(Point, Point); 4] {
    [
        (Point::new(cell.x0, cell.y0), Point::new(cell.x1, cell.y0)),
        (Point::new(cell.x1, cell.y0), Point::new(cell.x1, cell.y1)),
        (Point::new(cell.x0, cell.y1), Point::new(cell.x1, cell.y1)),
        (Point::new(cell.x0, cell.y0), Point::new(cell.x0, cell.y1)),
    ]
}

/// True when the segment lies on the outer edge of `bounds`.
fn edge_on_bounds(bounds: Rect, a: Point, b: Point) -> bool {
    const EPS: f64 = 0.05;
    let same_y = (a.y - b.y).abs() <= EPS;
    let same_x = (a.x - b.x).abs() <= EPS;
    if same_y && ((a.y - bounds.y0).abs() <= EPS || (a.y - bounds.y1).abs() <= EPS) {
        return true;
    }
    same_x && ((a.x - bounds.x0).abs() <= EPS || (a.x - bounds.x1).abs() <= EPS)
}

/// What a placeholder does at fill time. This wins over the column kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaceholderRole {
    Text,
    Image,
    Qr,
    Hyperlink,
}

impl PlaceholderRole {
    pub fn as_str(self) -> &'static str {
        match self {
            PlaceholderRole::Text => "text",
            PlaceholderRole::Image => "image",
            PlaceholderRole::Qr => "qr",
            PlaceholderRole::Hyperlink => "hyperlink",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "text" => Some(PlaceholderRole::Text),
            "image" => Some(PlaceholderRole::Image),
            "qr" => Some(PlaceholderRole::Qr),
            "hyperlink" => Some(PlaceholderRole::Hyperlink),
            _ => None,
        }
    }
}

/// Where the placeholder sits. Offsets match `HyperlinkSource::Text` (story byte offsets).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PlaceholderAnchor {
    Text { story: StoryId, start: usize, end: usize },
    Item { id: ItemId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placeholder {
    pub id: u64,
    pub source_id: u64,
    pub field: String,
    pub role: PlaceholderRole,
    pub anchor: PlaceholderAnchor,
}

/// Last merge options, saved with the document. The merge command reads these and does not
/// write them back, so a one-off run leaves the template's undo stack alone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeOptions {
    /// `all`, `one`, or `range`.
    #[serde(default = "all_records")]
    pub records: String,
    /// 1-based record used when `records` is `one`.
    #[serde(default = "one_u32")]
    pub one: u32,
    /// Range string used when `records` is `range`.
    #[serde(default)]
    pub range: String,
    /// `single` or `multiple`.
    #[serde(default = "single_page")]
    pub per_page: String,
    /// `rows` or `columns`. Used when tiling.
    #[serde(default = "arrange_rows")]
    pub arrange: String,
    /// Top, right, bottom, left, in points.
    #[serde(default = "default_insets")]
    pub insets: [f64; 4],
    #[serde(default)]
    pub column_spacing: f64,
    #[serde(default)]
    pub row_spacing: f64,
    /// `fitProportionally`, `fillProportionally`, `fitContentToFrame`, or `none`.
    #[serde(default = "fit_prop")]
    pub fitting: String,
    #[serde(default)]
    pub center: bool,
    #[serde(default = "yes")]
    pub link_images: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

fn all_records() -> String {
    "all".into()
}
fn one_u32() -> u32 {
    1
}
fn single_page() -> String {
    "single".into()
}
fn arrange_rows() -> String {
    "rows".into()
}
fn default_insets() -> [f64; 4] {
    [36.0, 36.0, 36.0, 36.0]
}
fn fit_prop() -> String {
    "fitProportionally".into()
}
fn yes() -> bool {
    true
}

impl Default for MergeOptions {
    fn default() -> Self {
        MergeOptions {
            records: all_records(),
            one: 1,
            range: String::new(),
            per_page: single_page(),
            arrange: arrange_rows(),
            insets: default_insets(),
            column_spacing: 0.0,
            row_spacing: 0.0,
            fitting: fit_prop(),
            center: false,
            link_images: true,
            limit: None,
        }
    }
}

impl MergeOptions {
    pub fn is_default(&self) -> bool {
        self == &MergeOptions::default()
    }
}

/// Document data-merge state. Empty state is omitted from the save.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataMerge {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<DataSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placeholders: Vec<Placeholder>,
    #[serde(default, skip_serializing_if = "MergeOptions::is_default")]
    pub options: MergeOptions,
    /// Session id of the template this document was merged from. Not saved.
    #[serde(skip)]
    pub template_uid: Option<u64>,
}

impl DataMerge {
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty() && self.placeholders.is_empty() && self.options.is_default()
    }
}
