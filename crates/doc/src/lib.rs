//! The DesignCraft document model (pure data + serde).
//!
//! A [`Document`] holds spreads of pages, parent ("master") spreads, layers, stories, styles,
//! swatches, sections and embedded assets. Spreads, items and stories are `Arc`-shared so an
//! edit clones only what it touches and undo snapshots are O(1).
//!
//! Coordinates are points, y down. Each spread has its own space: pages sit side by side from
//! x = 0 with their tops at y = 0. Items live in spread space via `Item::xf`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod anchored;
pub mod arabic;
pub mod arrow;
pub mod attrs;
pub mod build;
pub mod cjk;
pub mod datamerge;
mod edit;
pub mod endnotes;
pub mod ids;
pub mod index;
pub mod item;
pub mod notes;
pub mod otf;
pub mod page;
pub mod selection;
mod slice;
pub mod story;
pub mod styles;
pub mod table;
pub mod vars;
pub mod xref;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use anchored::{AnchorPosition, AnchoredObject, OBJECT_MARK};
pub use attrs::*;
pub use datamerge::{
    DataField, DataFieldKind, DataMerge, DataSource, Delimiter, Fingerprint, MergeOptions, Placeholder, PlaceholderAnchor, PlaceholderRole,
    SourceStatus,
};
pub use designcraft_color as color;
pub use designcraft_geom as geom;
pub use edit::{ItemLoc, ItemPath, SpreadRef, item_hit as edit_hit};
pub use endnotes::{ENDNOTE_REF, EditorialNote, EndnoteOptions, NOTE_MARK};
pub use ids::*;
pub use index::{INDEX_MARK, IndexRef};
pub use item::*;
pub use notes::{FOOTNOTE_REF, FOOTNOTE_TABLE, Footnote, FootnoteOptions};
pub use page::*;
pub use selection::*;
pub use story::*;
pub use styles::*;
pub use table::*;
pub use xref::{ANCHOR_MARK, CrossRef, TextAnchor, XREF_MARK, XrefFormat};

use designcraft_color::Swatch;
use designcraft_geom::Unit;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DocError {
    #[error("no such item {0}")]
    NoItem(ItemId),
    #[error("no such story {0}")]
    NoStory(StoryId),
    #[error("no such page {0}")]
    NoPage(usize),
    #[error("no such layer {0}")]
    NoLayer(LayerId),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, DocError>;

/// Largest `next_id` a valid document has (ids count up from it; 2^53 also stays exact in JSON
/// readers that use doubles).
pub const MAX_NEXT_ID: u64 = 1 << 53;

/// Edit › Transparency Blend Space: the colour space transparency is flattened in (CMYK for
/// print documents, RGB for screen ones, as InDesign defaults).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlendSpace {
    #[default]
    Cmyk,
    Rgb,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Intent {
    #[default]
    Print,
    Web,
    Mobile,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridRelative {
    #[default]
    TopOfPage,
    TopMargin,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BaselineGrid {
    pub start: f64,
    pub increment: f64,
    pub relative_to: GridRelative,
    pub color: [u8; 3],
    /// Hidden below this zoom (1.0 = 100%).
    pub view_threshold: f64,
}

impl Default for BaselineGrid {
    fn default() -> Self {
        BaselineGrid { start: 36.0, increment: 12.0, relative_to: GridRelative::TopOfPage, color: [140, 205, 230], view_threshold: 0.75 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DocumentGrid {
    pub horizontal: f64,
    pub vertical: f64,
    pub subdivisions: u32,
    pub color: [u8; 3],
    pub in_back: bool,
}

impl Default for DocumentGrid {
    fn default() -> Self {
        DocumentGrid { horizontal: 72.0, vertical: 72.0, subdivisions: 8, color: [200, 200, 200], in_back: true }
    }
}

/// A named numbered list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NumberedList {
    pub name: String,
    /// Numbering continues from story to story (in page order).
    pub continue_across_stories: bool,
}

impl Default for NumberedList {
    fn default() -> Self {
        NumberedList { name: String::new(), continue_across_stories: true }
    }
}

/// A conditional-text condition: its indicator colour (screen only) and whether text with it shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    pub name: String,
    pub color: [u8; 3],
    pub visible: bool,
}

impl Document {
    /// A stroke type with named styles resolved (unknown names draw solid).
    pub fn stroke_kind(&self, t: &StrokeType) -> StrokeType {
        match t {
            StrokeType::Style { name } => self
                .stroke_styles
                .iter()
                .find(|s| s.name == *name)
                .map(|s| s.kind.clone())
                .filter(|k| !matches!(k, StrokeType::Style { .. }))
                .unwrap_or(StrokeType::Solid),
            t => t.clone(),
        }
    }

    /// Hyphenation exceptions by lowercase word: the break positions (char indices; empty = never
    /// hyphenate).
    pub fn hyphenation_exception_map(&self) -> std::collections::HashMap<String, Vec<usize>> {
        self.hyphenation_exceptions
            .iter()
            .map(|e| {
                let mut word = String::new();
                let mut breaks = Vec::new();
                for c in e.trim().chars() {
                    if c == '~' {
                        if breaks.last() != Some(&word.chars().count()) && !word.is_empty() {
                            breaks.push(word.chars().count());
                        }
                    } else {
                        word.extend(c.to_lowercase());
                    }
                }
                breaks.retain(|b| *b < word.chars().count());
                (word, breaks)
            })
            .collect()
    }

    /// Is text with these conditions hidden? (Conditioned text shows while any of its conditions
    /// is visible.)
    pub fn conditions_hide(&self, conds: &[String]) -> bool {
        !conds.is_empty() && conds.iter().all(|c| self.conditions.iter().any(|x| x.name == *c && !x.visible))
    }
}

/// Ink Manager settings for output.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InkManager {
    /// All Spots to Process.
    pub all_to_process: bool,
    /// Spot inks converted to process one by one.
    pub to_process: Vec<String>,
    /// Ink Alias: (spot ink, the ink it prints on).
    pub aliases: Vec<(String, String)>,
}

impl InkManager {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The ink a spot swatch prints on (after aliases; at most a few hops) and whether it's
    /// converted to process.
    pub fn resolve<'a>(&'a self, ink: &'a str) -> (&'a str, bool) {
        let mut n = ink;
        for _ in 0..8 {
            match self.aliases.iter().find(|(a, _)| a == n) {
                Some((_, to)) if to != n => n = to,
                _ => break,
            }
        }
        (n, self.all_to_process || self.to_process.iter().any(|x| x == n))
    }
}

/// XML tags and mappings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct XmlSettings {
    pub root: String,
    pub tags: Vec<XmlTag>,
    /// Paragraph style → tag: tagged frames' paragraphs become elements of that name.
    pub style_map: Vec<(String, String)>,
    /// The loaded Document Type Definition (its text), for validation.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub dtd: String,
}

impl XmlSettings {
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.style_map.is_empty() && self.root.is_empty() && self.dtd.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XmlTag {
    pub name: String,
    pub color: [u8; 3],
}

/// An article: objects in reading order; `export` includes it when exporting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Article {
    pub name: String,
    pub items: Vec<ItemId>,
    #[serde(default = "yes")]
    pub export: bool,
}

/// A Swatches panel colour group: a named folder of swatches (by name).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorGroup {
    pub name: String,
    pub swatches: Vec<String>,
}

/// Preferences › Advanced Type (stored with the document, like InDesign).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AdvancedType {
    pub superscript_size: f64,
    pub superscript_position: f64,
    pub subscript_size: f64,
    pub subscript_position: f64,
}

impl Default for AdvancedType {
    fn default() -> Self {
        AdvancedType { superscript_size: 58.3, superscript_position: 33.3, subscript_size: 58.3, subscript_position: 33.3 }
    }
}

/// File → Document Setup + the guide/grid/unit preferences stored with the document.
/// Where a document's chapter number comes from (Numbering & Section Options › Document Chapter
/// Numbering). A book's Update Numbering resolves it in book order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChapterSource {
    /// Automatic Chapter Numbering: one more than the previous document in the book (1 first).
    #[default]
    Automatic,
    /// Start Chapter Numbering at `chapter_number`.
    UserDefined,
    /// Same as Previous Document in the Book: the document continues its chapter.
    SameAsPrevious,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DocSettings {
    pub intent: Intent,
    pub page_width: f64,
    pub page_height: f64,
    pub facing_pages: bool,
    /// Binding: Right to Left — page 1 is a left page and spreads read from right to left.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub right_to_left_binding: bool,
    pub primary_text_frame: bool,
    /// Bleed: top, bottom, inside, outside.
    pub bleed: [f64; 4],
    pub slug: [f64; 4],
    pub horizontal_units: Unit,
    pub vertical_units: Unit,
    pub baseline_grid: BaselineGrid,
    pub grid: DocumentGrid,
    /// Pasteboard extent around spreads (horizontal, vertical).
    pub pasteboard: (f64, f64),
    pub margin_color: [u8; 3],
    pub column_color: [u8; 3],
    pub bleed_color: [u8; 3],
    pub slug_color: [u8; 3],
    pub keyboard_increment: f64,
    /// Named lists (Type › Bulleted and Numbered Lists › Define Lists).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lists: Vec<NumberedList>,
    /// The story in the primary text frames (Smart Text Reflow adds and removes pages for it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_story: Option<StoryId>,
    /// Chapter number (Numbering & Section Options › Document Chapter Numbering): the one set by
    /// Start Chapter Numbering at, or the one a book last gave the document.
    pub chapter_number: u32,
    /// The chapter number's style (Chapter Number variables, cross-references).
    pub chapter_style: NumberStyle,
    /// Where the chapter number comes from.
    pub chapter_source: ChapterSource,
    /// Object › Effects › Global Light: the angle (degrees) shadows that use it share.
    pub global_light: f64,
    /// Preferences › Advanced Type: superscript and subscript size and position (percent of the
    /// font size).
    pub advanced_type: AdvancedType,
    /// Edit › Transparency Blend Space.
    pub blend_space: BlendSpace,
    /// Preferences › Appearance of Black: [Black] at 100% overprints (Overprint Preview, output).
    pub overprint_black: bool,
    /// Type › Track Changes: edits are recorded as inserted / deleted text.
    pub track_changes: bool,
    /// Preferences › Composition › Draw Missing Glyphs from Fallback Fonts: characters the
    /// applied font lacks are drawn from other fonts. Off (InDesign's behaviour, and new
    /// documents'), they are drawn as the font's missing-glyph box and Preflight lists them.
    /// Documents saved before the setting existed read as on, the way they were drawn.
    #[serde(default = "yes")]
    pub glyph_fallback: bool,
}

impl Default for DocSettings {
    fn default() -> Self {
        DocSettings {
            intent: Intent::Print,
            page_width: 612.0,
            page_height: 792.0,
            facing_pages: true,
            right_to_left_binding: false,
            primary_text_frame: false,
            bleed: [0.0; 4],
            slug: [0.0; 4],
            horizontal_units: Unit::Picas,
            vertical_units: Unit::Picas,
            baseline_grid: BaselineGrid::default(),
            grid: DocumentGrid::default(),
            pasteboard: (72.0, 72.0),
            margin_color: [255, 56, 255],
            column_color: [166, 40, 255],
            bleed_color: [255, 72, 103],
            slug_color: [100, 188, 221],
            keyboard_increment: 1.0,
            primary_story: None,
            lists: Vec::new(),
            chapter_number: 1,
            chapter_style: NumberStyle::Arabic,
            chapter_source: ChapterSource::Automatic,
            global_light: 120.0,
            advanced_type: AdvancedType::default(),
            blend_space: BlendSpace::Cmyk,
            overprint_black: true,
            track_changes: false,
            glyph_fallback: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    /// Selection / frame-edge colour.
    pub color: [u8; 3],
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "yes")]
    pub printable: bool,
    #[serde(default = "yes")]
    pub show_guides: bool,
    #[serde(default)]
    pub suppress_wrap_when_hidden: bool,
}

fn yes() -> bool {
    true
}

/// InDesign's layer colour sequence (names are the conventional colour names).
pub const LAYER_COLORS: &[(&str, [u8; 3])] = &[
    ("Light Blue", [43, 155, 255]),
    ("Red", [255, 0, 0]),
    ("Green", [79, 255, 79]),
    ("Blue", [0, 0, 255]),
    ("Yellow", [255, 255, 79]),
    ("Magenta", [255, 79, 255]),
    ("Cyan", [0, 255, 255]),
    ("Gray", [128, 128, 128]),
    ("Black", [0, 0, 0]),
    ("Orange", [255, 102, 0]),
    ("Dark Green", [0, 84, 0]),
    ("Teal", [0, 153, 153]),
    ("Tan", [204, 153, 102]),
    ("Brown", [153, 51, 0]),
    ("Violet", [153, 51, 255]),
    ("Gold", [255, 153, 0]),
];

/// An embedded file (placed image / PDF). Bytes are stored next to the JSON in the native format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: AssetId,
    pub name: String,
    pub mime: String,
    /// Original location on disk (Links panel), if linked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(skip)]
    pub data: Arc<Vec<u8>>,
    /// Pixel size for raster images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixels: Option<(u32, u32)>,
    /// The page shown from a multi-page PDF (0-based; Image Import Options).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub page: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub title: String,
    pub settings: DocSettings,
    pub spreads: Vec<Arc<Spread>>,
    pub parents: Vec<Arc<Spread>>,
    /// Top of the list = frontmost layer (Layers panel order).
    pub layers: Vec<Layer>,
    pub stories: BTreeMap<StoryId, Arc<Story>>,
    pub styles: Arc<Styles>,
    pub swatches: Vec<Swatch>,
    /// Swatches panel colour groups (folders), in panel order; swatches not in a group sit at
    /// the top level.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub color_groups: Vec<ColorGroup>,
    /// XML: the tags (Tags panel) and the paragraph style → tag map (Map Styles to Tags).
    #[serde(default, skip_serializing_if = "XmlSettings::is_empty")]
    pub xml: XmlSettings,
    /// Articles panel: named reading-order lists of objects (EPUB / HTML export order).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub articles: Vec<Article>,
    /// Custom stroke styles (Object › Stroke Styles).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stroke_styles: Vec<StrokeStyleDef>,
    /// Ink Manager: spot inks printed as process, and spot inks aliased to others.
    #[serde(default, skip_serializing_if = "InkManager::is_default")]
    pub inks: InkManager,
    /// Document Endnote Options, and the story shown in the endnote frame (generated).
    #[serde(default)]
    pub endnote_options: EndnoteOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endnote_story: Option<StoryId>,
    /// Conditional text conditions; text whose conditions are all hidden isn't composed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    pub sections: Vec<Section>,
    #[serde(default)]
    pub assets: BTreeMap<AssetId, Arc<Asset>>,
    /// Hyperlinks (Window → Interactive → Hyperlinks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hyperlinks: Vec<Hyperlink>,
    /// Data merge: the linked table, placeholders, and the last merge options.
    #[serde(default, skip_serializing_if = "DataMerge::is_empty")]
    pub data_merge: DataMerge,
    /// PDF bookmarks (Window → Interactive → Bookmarks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bookmarks: Vec<Bookmark>,
    /// Words added to the document's user dictionary (spelling).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_words: Vec<String>,
    /// User dictionary hyphenation exceptions: `ex~am~ple` (breaks only at `~`), or a word with
    /// no `~` that never hyphenates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hyphenation_exceptions: Vec<String>,
    /// The generated table of contents (Layout → Table of Contents), kept for Update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toc: Option<Toc>,
    /// Text variable definitions (instances in stories are `vars::var_char(index)`).
    #[serde(default = "vars::defaults")]
    pub text_variables: Vec<vars::TextVariable>,
    /// Type › Document Footnote Options.
    #[serde(default)]
    pub footnote_options: FootnoteOptions,
    /// Cross-reference formats (Type › Hyperlinks & Cross-References › Cross-Reference Formats).
    #[serde(default = "xref::default_formats")]
    pub xref_formats: Vec<XrefFormat>,
    /// The generated index (kept for Update Index).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<index::IndexSpec>,
    /// Creation / last save time (Unix seconds, UTC).
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub modified: i64,
    pub next_id: u64,
    /// While the document is open, the font scope of the fonts it brought (its `Document Fonts`
    /// folder): composition, export and the font menus look its fonts up there. 0: none. Not
    /// saved.
    #[serde(skip)]
    pub font_scope: u32,
}

/// What a hyperlink is attached to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum HyperlinkSource {
    Item { id: ItemId },
    Text { story: StoryId, start: usize, end: usize },
}

/// Where a hyperlink goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum HyperlinkDest {
    Url(String),
    Email(String),
    /// Absolute page index.
    Page(usize),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hyperlink {
    pub id: u64,
    pub name: String,
    pub source: HyperlinkSource,
    pub dest: HyperlinkDest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TocEntry {
    /// Paragraph style whose paragraphs are listed.
    pub style: String,
    /// 1-based nesting level.
    #[serde(default = "one")]
    pub level: u8,
}

fn one() -> u8 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Toc {
    pub story: StoryId,
    pub title: String,
    pub entries: Vec<TocEntry>,
    #[serde(default)]
    pub page_numbers: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub name: String,
    /// Absolute page index.
    pub page: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Bookmark>,
}

impl Document {
    /// Allocate a fresh id.
    pub fn alloc(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn page_count(&self) -> usize {
        self.spreads.iter().map(|s| s.pages.len()).sum()
    }

    /// (spread index, page index within spread) for an absolute page index.
    pub fn page_loc(&self, abs: usize) -> Option<(usize, usize)> {
        let mut n = 0;
        for (si, s) in self.spreads.iter().enumerate() {
            if abs < n + s.pages.len() {
                return Some((si, abs - n));
            }
            n += s.pages.len();
        }
        None
    }

    /// Absolute index of the first page of spread `si`.
    pub fn first_page_of_spread(&self, si: usize) -> usize {
        self.spreads[..si.min(self.spreads.len())].iter().map(|s| s.pages.len()).sum()
    }

    pub fn page(&self, abs: usize) -> Option<&Page> {
        let (s, p) = self.page_loc(abs)?;
        self.spreads[s].pages.get(p)
    }

    /// Iterate all pages with their absolute index and spread index.
    pub fn pages(&self) -> impl Iterator<Item = (usize, usize, &Page)> {
        self.spreads.iter().enumerate().flat_map(|(si, s)| s.pages.iter().map(move |p| (si, p))).enumerate().map(|(i, (si, p))| (i, si, p))
    }

    /// The section containing absolute page `abs`.
    pub fn section_of(&self, abs: usize) -> Option<&Section> {
        self.sections.iter().filter(|s| s.start <= abs).max_by_key(|s| s.start)
    }

    /// The page number (before formatting) of absolute page `abs`, per sections. A section without
    /// a start number continues from the previous section.
    pub fn page_number(&self, abs: usize) -> u32 {
        let Some(sec) = self.section_of(abs) else { return abs as u32 + 1 };
        let start = match sec.start_number {
            Some(n) => n,
            None if sec.start == 0 => 1,
            None => self.page_number(sec.start - 1) + 1,
        };
        start + (abs - sec.start) as u32
    }

    /// The displayed page name ("1", "iv", "A-3"…) for an absolute page index, per sections.
    /// The chapter number in its style (`3`, `III`, `c`, `03`).
    pub fn chapter_label(&self) -> String {
        self.settings.chapter_style.format(self.settings.chapter_number.max(1))
    }

    pub fn page_name(&self, abs: usize) -> String {
        let Some(sec) = self.section_of(abs) else { return (abs + 1).to_string() };
        let num = sec.style.format(self.page_number(abs));
        if sec.include_prefix { format!("{}{}", sec.prefix, num) } else { num }
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }
    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }
    /// Stacking rank of a layer: 0 = backmost.
    pub fn layer_rank(&self, id: LayerId) -> usize {
        self.layers.iter().position(|l| l.id == id).map(|i| self.layers.len() - 1 - i).unwrap_or(0)
    }
    pub fn default_layer(&self) -> LayerId {
        self.layers.first().map(|l| l.id).unwrap_or_default()
    }

    pub fn story(&self, id: StoryId) -> Option<&Story> {
        self.stories.get(&id).map(|s| &**s)
    }
    pub fn story_mut(&mut self, id: StoryId) -> Option<&mut Story> {
        self.stories.get_mut(&id).map(Arc::make_mut)
    }

    pub fn swatch(&self, name: &str) -> Option<&Swatch> {
        self.swatches.iter().find(|s| s.name == name)
    }

    /// Resolve a swatch reference to a display colour.
    /// A shadow's light angle: its own, or the document's Global Light.
    pub fn light_angle(&self, angle: f64, global: bool) -> f64 {
        if global { self.settings.global_light } else { angle }
    }

    pub fn resolve_color(&self, swatch: &str, tint: f32) -> Option<designcraft_color::Color> {
        designcraft_color::swatch::resolve(&self.swatches, swatch, tint)
    }

    /// Validate structural invariants (ids unique, threads consistent, stories well-formed).
    pub fn check(&self) -> Result<()> {
        for f in &self.styles.composite_fonts {
            if f.entries.iter().any(|e| {
                ![e.relative_size, e.horizontal_scale, e.vertical_scale].iter().all(|v| v.is_finite() && *v > 0.0) || !e.baseline_shift.is_finite()
            }) {
                return Err(DocError::Invalid(format!("invalid composite font metrics: {}", f.name)));
            }
        }
        for t in &self.styles.mojikumi_tables {
            if t.overrides.iter().any(|r| ![r.minimum, r.desired, r.maximum].iter().all(|v| v.is_finite())) {
                return Err(DocError::Invalid(format!("non-finite mojikumi spacing: {}", t.name)));
            }
        }
        let mut ids = std::collections::HashSet::new();
        for sp in self.spreads.iter().chain(self.parents.iter()) {
            for it in &sp.items {
                let mut dup = None;
                it.walk(&mut |i| {
                    if !ids.insert(i.id.0) {
                        dup = Some(i.id);
                    }
                });
                if let Some(d) = dup {
                    return Err(DocError::Invalid(format!("duplicate item id {d}")));
                }
            }
        }
        for (sid, st) in &self.stories {
            st.check().map_err(DocError::Invalid)?;
            if st.id != *sid {
                return Err(DocError::Invalid(format!("story key {sid} != id {}", st.id)));
            }
            for f in &st.frames {
                let item = self.find(*f).ok_or(DocError::NoItem(*f))?;
                match self.item_at(&item).and_then(|i| i.text_frame().map(|t| t.story)) {
                    Some(s) if s == *sid => {}
                    other => return Err(DocError::Invalid(format!("frame {f} in story {sid} points at {other:?}"))),
                }
            }
        }
        // Every text frame is in its story's thread.
        for sp in self.spreads.iter().chain(self.parents.iter()) {
            for it in &sp.items {
                let mut bad = None;
                it.walk(&mut |i| {
                    if let Some(t) = i.text_frame()
                        && !self.stories.get(&t.story).is_some_and(|s| s.frames.contains(&i.id))
                    {
                        bad = Some(i.id);
                    }
                });
                if let Some(b) = bad {
                    return Err(DocError::Invalid(format!("text frame {b} not in its story's thread")));
                }
            }
        }
        if self.next_id < ids.iter().copied().max().unwrap_or(0) {
            return Err(DocError::Invalid("next_id behind existing ids".into()));
        }
        // Ids are allocated by counting up from `next_id`: leave room so that can't overflow.
        if self.next_id > MAX_NEXT_ID {
            return Err(DocError::Invalid(format!("next_id {} out of range", self.next_id)));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

impl Document {
    /// Mutable styles (copy-on-write).
    pub fn styles_mut(&mut self) -> &mut Styles {
        Arc::make_mut(&mut self.styles)
    }
}
