//! Data merge: one linked table, placeholders, and the last merge options.
//!
//! Phase 1 stores at most one source. The list is the seam for a later multi-source manager.
//! Preview is session state and is not stored here.

use std::collections::BTreeMap;

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
    /// A QR placeholder built from several fields: the `object.qrCode` params (`type`, `name`,
    /// `phone`, `mobile`…), each of which may hold `<<Field>>` references. Empty: the code is
    /// `field`'s cell.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub qr: BTreeMap<String, String>,
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
}

impl DataMerge {
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty() && self.placeholders.is_empty() && self.options.is_default()
    }
}
