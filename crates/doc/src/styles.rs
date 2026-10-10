//! Paragraph, character and object styles with based-on inheritance.

use serde::{Deserialize, Serialize};

use crate::attrs::{CharAttrs, CharProps, ParaAttrs, ParaProps};
use crate::item::{Fill, Stroke, TextFrameOptions};
use crate::story::{BASIC_PARAGRAPH, CharFormat, NO_CHAR_STYLE, ParaFormat};

pub const NO_PARA_STYLE: &str = "[No Paragraph Style]";
pub const BASIC_GRAPHICS_FRAME: &str = "[Basic Graphics Frame]";
pub const BASIC_TEXT_FRAME: &str = "[Basic Text Frame]";
pub const NO_OBJECT_STYLE: &str = "[None]";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParagraphStyle {
    /// Unique name; groups are expressed as `Group/Name`.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_style: Option<String>,
    #[serde(default)]
    pub para: ParaAttrs,
    #[serde(default)]
    pub chars: CharAttrs,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shortcut: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterStyle {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(default)]
    pub chars: CharAttrs,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shortcut: String,
}

/// Object style: each attribute group is optional ("not included in style" when `None`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ObjectStyle {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<Fill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Stroke>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paragraph_style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_frame: Option<TextFrameOptions>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Styles {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composite_fonts: Vec<crate::cjk::CompositeFont>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mojikumi_tables: Vec<crate::cjk::MojikumiTable>,
    /// Export Tagging per style (`p:Name` or `c:Name`): the HTML tag and class for EPUB/HTML.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub export_tags: std::collections::BTreeMap<String, ExportTag>,
    pub paragraph: Vec<ParagraphStyle>,
    pub character: Vec<CharacterStyle>,
    pub object: Vec<ObjectStyle>,
    /// Cell and table styles (Table › Cell Styles / Table Styles).
    #[serde(default)]
    pub cell: Vec<CellStyle>,
    #[serde(default)]
    pub table: Vec<TableStyle>,
    /// Default styles for new text/frames (the style selected with nothing selected).
    pub default_paragraph: String,
    pub default_character: String,
    pub default_text_frame: String,
    pub default_graphic_frame: String,
}

/// A style's Export Tagging.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportTag {
    /// HTML element (`p`, `h1`…`h6`, `blockquote`, `pre`, `li` for paragraphs; `span`, `em`,
    /// `strong`, `code`, `sup`, `sub` for characters). Empty = automatic.
    pub tag: String,
    /// CSS class (empty = from the style name).
    pub class: String,
}

impl Styles {
    /// Export tagging of a paragraph (`character` false) or character style.
    pub fn export_tag(&self, name: &str, character: bool) -> Option<&ExportTag> {
        self.export_tags.get(&format!("{}:{name}", if character { "c" } else { "p" }))
    }
}

impl Default for Styles {
    fn default() -> Self {
        Styles {
            composite_fonts: Vec::new(),
            mojikumi_tables: Vec::new(),
            export_tags: Default::default(),
            paragraph: vec![
                ParagraphStyle {
                    name: NO_PARA_STYLE.into(),
                    based_on: None,
                    next_style: None,
                    para: ParaAttrs::default(),
                    chars: CharAttrs::default(),
                    shortcut: String::new(),
                },
                ParagraphStyle {
                    name: BASIC_PARAGRAPH.into(),
                    based_on: None,
                    next_style: Some(BASIC_PARAGRAPH.into()),
                    para: ParaAttrs::default(),
                    chars: CharAttrs::default(),
                    shortcut: String::new(),
                },
            ],
            character: vec![CharacterStyle { name: NO_CHAR_STYLE.into(), based_on: None, chars: CharAttrs::default(), shortcut: String::new() }],
            object: vec![
                ObjectStyle { name: NO_OBJECT_STYLE.into(), ..Default::default() },
                ObjectStyle { name: BASIC_GRAPHICS_FRAME.into(), stroke: Some(Stroke::default()), ..Default::default() },
                ObjectStyle {
                    name: BASIC_TEXT_FRAME.into(),
                    fill: Some(Fill::none()),
                    stroke: Some(Stroke::none()),
                    paragraph_style: Some(BASIC_PARAGRAPH.into()),
                    ..Default::default()
                },
            ],
            cell: vec![CellStyle { name: NO_CELL_STYLE.into(), ..Default::default() }],
            table: vec![TableStyle { name: BASIC_TABLE.into(), ..Default::default() }],
            default_paragraph: BASIC_PARAGRAPH.into(),
            default_character: NO_CHAR_STYLE.into(),
            default_text_frame: BASIC_TEXT_FRAME.into(),
            default_graphic_frame: BASIC_GRAPHICS_FRAME.into(),
        }
    }
}

pub const NO_CELL_STYLE: &str = "[None]";
pub const BASIC_TABLE: &str = "[Basic Table]";
pub const NO_TABLE_STYLE: &str = "[No table style]";

/// Partial edge formatting. Missing attributes inherit; zero weight and the None swatch
/// are explicit values, rather than a request to use the parent/default edge.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CellStrokeAttrs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tint: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<crate::item::StrokeType>,
}

impl CellStrokeAttrs {
    pub fn is_empty(&self) -> bool {
        self.weight.is_none() && self.color.is_none() && self.tint.is_none() && self.kind.is_none()
    }

    pub fn apply_to(&self, stroke: &mut crate::table::CellStroke) {
        if let Some(v) = self.weight {
            stroke.weight = v;
        }
        if let Some(v) = &self.color {
            stroke.color = v.clone();
        }
        if let Some(v) = self.tint {
            stroke.tint = v;
        }
        if let Some(v) = &self.kind {
            stroke.kind = v.clone();
        }
    }

    fn merge(&mut self, other: &Self) {
        if other.weight.is_some() {
            self.weight = other.weight;
        }
        if other.color.is_some() {
            self.color = other.color.clone();
        }
        if other.tint.is_some() {
            self.tint = other.tint;
        }
        if other.kind.is_some() {
            self.kind = other.kind.clone();
        }
    }

    fn from_stroke(stroke: &crate::table::CellStroke) -> Self {
        Self { weight: Some(stroke.weight), color: Some(stroke.color.clone()), tint: Some(stroke.tint), kind: Some(stroke.kind.clone()) }
    }
}

fn no_edge_attrs(edges: &[CellStrokeAttrs; 4]) -> bool {
    edges.iter().all(CellStrokeAttrs::is_empty)
}

fn no_inset_attrs(insets: &[Option<f64>; 4]) -> bool {
    insets.iter().all(Option::is_none)
}

/// Partial alternating-fill settings, retained separately from the legacy complete value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AltFillsAttrs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_tint: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_tint: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_first: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_last: Option<u32>,
}

impl AltFillsAttrs {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Overlay supplied attributes, retaining explicit zero counts and None swatches.
    pub fn merge(&mut self, other: &Self) {
        if other.first.is_some() {
            self.first = other.first;
        }
        if other.first_color.is_some() {
            self.first_color = other.first_color.clone();
        }
        if other.first_tint.is_some() {
            self.first_tint = other.first_tint;
        }
        if other.next.is_some() {
            self.next = other.next;
        }
        if other.next_color.is_some() {
            self.next_color = other.next_color.clone();
        }
        if other.next_tint.is_some() {
            self.next_tint = other.next_tint;
        }
        if other.skip_first.is_some() {
            self.skip_first = other.skip_first;
        }
        if other.skip_last.is_some() {
            self.skip_last = other.skip_last;
        }
    }

    fn from_fills(fills: &crate::table::AltFills) -> Self {
        Self {
            first: Some(fills.first),
            first_color: Some(fills.first_color.clone()),
            first_tint: Some(fills.first_tint),
            next: Some(fills.next),
            next_color: Some(fills.next_color.clone()),
            next_tint: Some(fills.next_tint),
            skip_first: Some(fills.skip_first),
            skip_last: Some(fills.skip_last),
        }
    }

    pub fn apply_to(&self, fills: &mut Option<crate::table::AltFills>) {
        if self.is_empty() {
            return;
        }
        let mut value = fills.clone().unwrap_or_default();
        if let Some(v) = self.first {
            value.first = v;
        }
        if let Some(v) = &self.first_color {
            value.first_color = v.clone();
        }
        if let Some(v) = self.first_tint {
            value.first_tint = v;
        }
        if let Some(v) = self.next {
            value.next = v;
        }
        if let Some(v) = &self.next_color {
            value.next_color = v.clone();
        }
        if let Some(v) = self.next_tint {
            value.next_tint = v;
        }
        if let Some(v) = self.skip_first {
            value.skip_first = v;
        }
        if let Some(v) = self.skip_last {
            value.skip_last = v;
        }
        *fills = (value.first > 0).then_some(value);
    }
}

/// Partial table alternating strokes; missing attributes inherit independently.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AltStrokesAttrs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<u32>,
    #[serde(skip_serializing_if = "CellStrokeAttrs::is_empty")]
    pub first_stroke: CellStrokeAttrs,
    #[serde(skip_serializing_if = "CellStrokeAttrs::is_empty")]
    pub next_stroke: CellStrokeAttrs,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_first: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_last: Option<u32>,
}

impl AltStrokesAttrs {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    pub fn merge(&mut self, other: &Self) {
        if other.first.is_some() {
            self.first = other.first;
        }
        if other.next.is_some() {
            self.next = other.next;
        }
        self.first_stroke.merge(&other.first_stroke);
        self.next_stroke.merge(&other.next_stroke);
        if other.skip_first.is_some() {
            self.skip_first = other.skip_first;
        }
        if other.skip_last.is_some() {
            self.skip_last = other.skip_last;
        }
    }

    pub fn apply_to(&self, strokes: &mut Option<crate::table::AltStrokes>) {
        if self.is_empty() {
            return;
        }
        let mut value = strokes.clone().unwrap_or_default();
        if let Some(v) = self.first {
            value.first = v;
        }
        if let Some(v) = self.next {
            value.next = v;
        }
        self.first_stroke.apply_to(&mut value.first_stroke);
        self.next_stroke.apply_to(&mut value.next_stroke);
        if let Some(v) = self.skip_first {
            value.skip_first = v;
        }
        if let Some(v) = self.skip_last {
            value.skip_last = v;
        }
        // Zero counts disable rendering, but the remaining settings still belong to
        // the document (and are reused if the pattern is enabled later).
        *strokes = Some(value);
    }
}

/// A cell style: the cell attributes it sets (unset = left as is) and a paragraph style for the
/// cell's text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CellStyle {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_tint: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insets: Option<[f64; 4]>,
    /// Per-side values override `insets`; top, left, bottom, right.
    #[serde(skip_serializing_if = "no_inset_attrs")]
    pub inset_overrides: [Option<f64>; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vj: Option<crate::item::VerticalJustification>,
    /// All four edges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<crate::table::CellStroke>,
    /// Per-edge attributes override the legacy uniform `stroke`.
    #[serde(skip_serializing_if = "no_edge_attrs")]
    pub strokes: [CellStrokeAttrs; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paragraph_style: Option<String>,
}

/// A table style: cell styles per region, the border, alternating row fills and spacing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TableStyle {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_same_as_body: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer_same_as_body: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_column_same_as_body: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right_column_same_as_body: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_column: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right_column: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<crate::table::CellStroke>,
    #[serde(skip_serializing_if = "no_edge_attrs")]
    pub borders: [CellStrokeAttrs; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_rows: Option<crate::table::AltFills>,
    #[serde(skip_serializing_if = "AltFillsAttrs::is_empty")]
    pub row_fills: AltFillsAttrs,
    #[serde(skip_serializing_if = "AltFillsAttrs::is_empty")]
    pub column_fills: AltFillsAttrs,
    #[serde(skip_serializing_if = "AltStrokesAttrs::is_empty")]
    pub row_strokes: AltStrokesAttrs,
    #[serde(skip_serializing_if = "AltStrokesAttrs::is_empty")]
    pub column_strokes: AltStrokesAttrs,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_before: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_after: Option<f64>,
}

impl CellStyle {
    /// Write this style's attributes into `cell`.
    pub fn apply_to(&self, cell: &mut crate::table::Cell) {
        if let Some(v) = &self.fill {
            cell.fill = v.clone();
        }
        if let Some(v) = self.fill_tint {
            cell.fill_tint = v;
        }
        if let Some(v) = self.insets {
            cell.insets = v;
        }
        for (slot, value) in cell.insets.iter_mut().zip(self.inset_overrides) {
            if let Some(value) = value {
                *slot = value;
            }
        }
        if let Some(v) = self.vj {
            cell.vj = v;
        }
        if let Some(v) = &self.stroke {
            cell.strokes = [v.clone(), v.clone(), v.clone(), v.clone()];
            cell.border_overrides = [true; 4];
            cell.stroke_defined = [true; 4];
        }
        for (i, edge) in self.strokes.iter().enumerate() {
            edge.apply_to(&mut cell.strokes[i]);
            if !edge.is_empty() {
                cell.border_overrides[i] = true;
                cell.stroke_defined[i] = true;
            }
        }
        if let Some(ps) = &self.paragraph_style {
            for p in &mut cell.text.paras {
                p.style = ps.clone();
            }
            cell.text.rev += 1;
        }
        cell.style = if self.name == NO_CELL_STYLE { String::new() } else { self.name.clone() };
    }
}

impl TableStyle {
    /// Write the resolved table options, then region cell styles in precedence order:
    /// body, left/right columns, header/footer. Explicit cell formatting is applied later.
    pub fn apply_to(&self, t: &mut crate::table::Table, cells: &[CellStyle]) {
        if let Some(b) = &self.border {
            t.options.border = b.clone();
            t.options.borders = Default::default();
        }
        for (i, attrs) in self.borders.iter().enumerate() {
            if !attrs.is_empty() {
                let mut edge = t.options.border_for(i).clone();
                attrs.apply_to(&mut edge);
                t.options.borders[i] = Some(edge);
            }
        }
        if self.borders.iter().any(|edge| !edge.is_empty()) {
            // Keep the legacy uniform accessor representative without changing absent sides.
            let borders = std::array::from_fn::<_, 4, _>(|i| t.options.border_for(i).clone());
            t.options.border = borders[0].clone();
            t.options.borders = borders.map(Some);
        }
        if self.alt_rows.is_some() {
            t.options.alt_rows = self.alt_rows.clone();
        }
        self.row_fills.apply_to(&mut t.options.alt_rows);
        self.column_fills.apply_to(&mut t.options.alt_cols);
        self.row_strokes.apply_to(&mut t.options.row_strokes);
        self.column_strokes.apply_to(&mut t.options.column_strokes);
        if let Some(v) = self.space_before {
            t.options.space_before = v;
        }
        if let Some(v) = self.space_after {
            t.options.space_after = v;
        }
        let (nr, nc) = (t.rows.len(), t.columns.len());
        for r in 0..nr {
            let kind = t.rows[r].kind;
            for c in 0..nc {
                let visual_column = if t.options.direction == crate::TextDirection::RightToLeft { nc - 1 - c } else { c };
                let same_or_region = |same, region: &Option<String>| {
                    if same == Some(true) { self.body.clone() } else { region.clone() }
                };
                let left = same_or_region(self.left_column_same_as_body, &self.left_column);
                let column = if visual_column == 0 && left.is_some() {
                    left
                } else if visual_column + 1 == nc {
                    same_or_region(self.right_column_same_as_body, &self.right_column)
                } else {
                    None
                };
                let row = match kind {
                    crate::table::RowKind::Header => same_or_region(self.header_same_as_body, &self.header),
                    crate::table::RowKind::Footer => same_or_region(self.footer_same_as_body, &self.footer),
                    crate::table::RowKind::Body => None,
                };
                if let Some(cell) = t.cells.get_mut(r * nc + c) {
                    for name in [self.body.as_ref(), column.as_ref(), row.as_ref()].into_iter().flatten() {
                        if name != NO_CELL_STYLE {
                            resolve_cell_style(cells, name).apply_to(cell);
                        }
                    }
                }
            }
        }
        t.style = if self.name == BASIC_TABLE { String::new() } else { self.name.clone() };
    }

    fn merge(&mut self, other: &Self) {
        if other.header.is_some() {
            self.header = other.header.clone();
        }
        if other.body.is_some() {
            self.body = other.body.clone();
        }
        if other.footer.is_some() {
            self.footer = other.footer.clone();
        }
        if other.left_column.is_some() {
            self.left_column = other.left_column.clone();
        }
        if other.right_column.is_some() {
            self.right_column = other.right_column.clone();
        }
        if other.header_same_as_body.is_some() {
            self.header_same_as_body = other.header_same_as_body;
        }
        if other.footer_same_as_body.is_some() {
            self.footer_same_as_body = other.footer_same_as_body;
        }
        if other.left_column_same_as_body.is_some() {
            self.left_column_same_as_body = other.left_column_same_as_body;
        }
        if other.right_column_same_as_body.is_some() {
            self.right_column_same_as_body = other.right_column_same_as_body;
        }
        if let Some(border) = &other.border {
            self.borders = std::array::from_fn(|_| CellStrokeAttrs::from_stroke(border));
        }
        for (edge, value) in self.borders.iter_mut().zip(&other.borders) {
            edge.merge(value);
        }
        if let Some(fills) = &other.alt_rows {
            self.row_fills = AltFillsAttrs::from_fills(fills);
        }
        self.row_fills.merge(&other.row_fills);
        self.column_fills.merge(&other.column_fills);
        self.row_strokes.merge(&other.row_strokes);
        self.column_strokes.merge(&other.column_strokes);
        if other.space_before.is_some() {
            self.space_before = other.space_before;
        }
        if other.space_after.is_some() {
            self.space_after = other.space_after;
        }
    }
}

/// Resolve a cell style with the same 32-level/cycle limit as paragraph styles.
pub fn resolve_cell_style(cells: &[CellStyle], name: &str) -> CellStyle {
    let mut chain = Vec::new();
    let mut cur = cells.iter().find(|s| s.name == name);
    while let Some(style) = cur {
        if chain.len() >= MAX_DEPTH || chain.iter().any(|s: &&CellStyle| s.name == style.name) {
            break;
        }
        chain.push(style);
        cur = style.based_on.as_deref().and_then(|base| cells.iter().find(|s| s.name == base));
    }
    let mut out = CellStyle { name: name.into(), ..Default::default() };
    for style in chain.into_iter().rev() {
        if style.fill.is_some() {
            out.fill = style.fill.clone();
        }
        if style.fill_tint.is_some() {
            out.fill_tint = style.fill_tint;
        }
        if let Some(insets) = style.insets {
            out.inset_overrides = insets.map(Some);
        }
        for (slot, value) in out.inset_overrides.iter_mut().zip(style.inset_overrides) {
            if value.is_some() {
                *slot = value;
            }
        }
        if style.vj.is_some() {
            out.vj = style.vj;
        }
        if let Some(stroke) = &style.stroke {
            out.strokes = std::array::from_fn(|_| CellStrokeAttrs::from_stroke(stroke));
        }
        for (slot, attrs) in out.strokes.iter_mut().zip(&style.strokes) {
            slot.merge(attrs);
        }
        if style.paragraph_style.is_some() {
            out.paragraph_style = style.paragraph_style.clone();
        }
    }
    out
}

const MAX_DEPTH: usize = 32;

impl Styles {
    /// Resolve table style inheritance without recursion; repeated/missing bases stop the chain.
    pub fn resolve_table_style(&self, name: &str) -> TableStyle {
        let mut chain = Vec::new();
        let mut cur = self.table.iter().find(|s| s.name == name);
        while let Some(style) = cur {
            if chain.len() >= MAX_DEPTH || chain.iter().any(|s: &&TableStyle| s.name == style.name) {
                break;
            }
            chain.push(style);
            cur = style.based_on.as_deref().and_then(|base| self.table.iter().find(|s| s.name == base));
        }
        let mut out = TableStyle { name: name.into(), ..Default::default() };
        for style in chain.into_iter().rev() {
            out.merge(style);
        }
        out
    }

    pub fn para(&self, name: &str) -> Option<&ParagraphStyle> {
        self.paragraph.iter().find(|s| s.name == name)
    }
    pub fn para_mut(&mut self, name: &str) -> Option<&mut ParagraphStyle> {
        self.paragraph.iter_mut().find(|s| s.name == name)
    }
    pub fn char_style(&self, name: &str) -> Option<&CharacterStyle> {
        self.character.iter().find(|s| s.name == name)
    }
    pub fn char_style_mut(&mut self, name: &str) -> Option<&mut CharacterStyle> {
        self.character.iter_mut().find(|s| s.name == name)
    }
    pub fn object_style(&self, name: &str) -> Option<&ObjectStyle> {
        self.object.iter().find(|s| s.name == name)
    }

    /// The paragraph style chain from root to `name` (cycle-safe).
    fn para_chain(&self, name: &str) -> Vec<&ParagraphStyle> {
        let mut chain = Vec::new();
        let mut cur = self.para(name);
        while let Some(s) = cur {
            if chain.len() >= MAX_DEPTH || chain.iter().any(|c: &&ParagraphStyle| c.name == s.name) {
                break;
            }
            chain.push(s);
            cur = s.based_on.as_deref().and_then(|b| self.para(b));
        }
        chain.reverse();
        chain
    }

    fn char_chain(&self, name: &str) -> Vec<&CharacterStyle> {
        let mut chain = Vec::new();
        let mut cur = self.char_style(name);
        while let Some(s) = cur {
            if chain.len() >= MAX_DEPTH || chain.iter().any(|c: &&CharacterStyle| c.name == s.name) {
                break;
            }
            chain.push(s);
            cur = s.based_on.as_deref().and_then(|b| self.char_style(b));
        }
        chain.reverse();
        chain
    }

    /// Resolved paragraph attributes and the paragraph's base character attributes.
    pub fn resolve_para(&self, p: &ParaFormat) -> (ParaProps, CharProps) {
        let (mut pp, mut cp) = (ParaProps::default(), CharProps::default());
        for s in self.para_chain(&p.style) {
            pp.apply(&s.para);
            cp.apply(&s.chars);
        }
        pp.apply(&p.para);
        cp.apply(&p.chars);
        (pp, cp)
    }

    /// Paragraph-style-only resolution (no local overrides).
    pub fn resolve_para_style(&self, name: &str) -> (ParaProps, CharProps) {
        self.resolve_para(&ParaFormat { style: name.into(), ..Default::default() })
    }

    /// Character attributes for a run: paragraph base ← character style chain ← local overrides.
    pub fn resolve_char(&self, para_chars: &CharProps, f: &CharFormat) -> CharProps {
        let mut cp = para_chars.clone();
        for s in self.char_chain(&f.style) {
            cp.apply(&s.chars);
        }
        cp.apply(&f.over);
        cp
    }

    /// The attributes character style `name` sets, with those of its Based On chain beneath them.
    pub fn char_style_attrs(&self, name: &str) -> CharAttrs {
        let mut a = CharAttrs::default();
        for s in self.char_chain(name) {
            a.merge(&s.chars);
        }
        a
    }

    /// Would making `name` based on `parent` create a cycle?
    pub fn para_based_on_cycles(&self, name: &str, parent: &str) -> bool {
        name == parent || self.para_chain(parent).iter().any(|s| s.name == name)
    }
    pub fn char_based_on_cycles(&self, name: &str, parent: &str) -> bool {
        name == parent || self.char_chain(parent).iter().any(|s| s.name == name)
    }

    /// A free name `base`, `base 2`, `base 3`…
    pub fn unique_name(existing: impl Fn(&str) -> bool, base: &str) -> String {
        if !existing(base) {
            return base.to_string();
        }
        (2..=usize::MAX).map(|i| format!("{base} {i}")).find(|n| !existing(n)).unwrap_or_else(|| format!("{base} copy"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn based_on_chain_resolves_in_order() {
        let mut st = Styles::default();
        st.paragraph.push(ParagraphStyle {
            name: "Body".into(),
            based_on: Some(BASIC_PARAGRAPH.into()),
            next_style: None,
            para: ParaAttrs { space_after: Some(6.0), ..Default::default() },
            chars: CharAttrs { size: Some(10.0), ..Default::default() },
            shortcut: String::new(),
        });
        st.paragraph.push(ParagraphStyle {
            name: "Body First".into(),
            based_on: Some("Body".into()),
            next_style: Some("Body".into()),
            para: ParaAttrs { first_line_indent: Some(0.0), ..Default::default() },
            chars: CharAttrs { size: Some(11.0), ..Default::default() },
            shortcut: String::new(),
        });
        let (pp, cp) = st.resolve_para_style("Body First");
        assert_eq!(pp.space_after, 6.0);
        assert_eq!(cp.size, 11.0);
        let f = CharFormat { style: NO_CHAR_STYLE.into(), over: CharAttrs { tracking: Some(10.0), ..Default::default() } };
        let r = st.resolve_char(&cp, &f);
        assert_eq!((r.size, r.tracking), (11.0, 10.0));
    }

    #[test]
    fn cycles_are_detected_and_safe() {
        let mut st = Styles::default();
        for (n, b) in [("A", "B"), ("B", "A")] {
            st.paragraph.push(ParagraphStyle {
                name: n.into(),
                based_on: Some(b.into()),
                next_style: None,
                para: ParaAttrs::default(),
                chars: CharAttrs::default(),
                shortcut: String::new(),
            });
        }
        let _ = st.resolve_para_style("A"); // terminates
        assert!(st.para_based_on_cycles("A", "B"));
        assert!(!st.para_based_on_cycles(BASIC_PARAGRAPH, NO_PARA_STYLE));
    }

    #[test]
    fn legacy_cell_and_table_style_json_keep_uniform_values() {
        let cell: CellStyle =
            serde_json::from_str(r#"{"name":"Legacy","insets":[1,2,3,4],"stroke":{"weight":2,"color":"Brand","tint":0.5,"kind":{"kind":"solid"}}}"#)
                .unwrap();
        assert!(cell.based_on.is_none());
        assert!(no_edge_attrs(&cell.strokes));
        assert!(no_inset_attrs(&cell.inset_overrides));
        let resolved = resolve_cell_style(std::slice::from_ref(&cell), "Legacy");
        let mut target = crate::table::Cell::default();
        resolved.apply_to(&mut target);
        assert_eq!(target.insets, [1.0, 2.0, 3.0, 4.0]);
        assert!(target.strokes.iter().all(|s| s.weight == 2.0 && s.color == "Brand" && s.tint == 0.5));
        let json = serde_json::to_value(&cell).unwrap();
        assert!(json.get("strokes").is_none());
        assert_eq!(serde_json::from_value::<CellStyle>(json).unwrap(), cell);

        let table: TableStyle =
            serde_json::from_str(r#"{"name":"Legacy","body":"Legacy","border":{"weight":3},"altRows":{"first":2,"firstColor":"Brand"}}"#).unwrap();
        let styles = Styles { cell: vec![cell], table: vec![table.clone()], ..Styles::default() };
        let mut target = crate::table::Table::new(1, 1, 2, 0, 0, 100.0);
        styles.resolve_table_style("Legacy").apply_to(&mut target, &styles.cell);
        assert_eq!(target.options.border_for(3).weight, 3.0);
        assert_eq!(target.options.alt_rows.as_ref().unwrap().first, 2);
        assert_eq!(target.cell(0, 0).unwrap().strokes[0].weight, 2.0);
        assert_eq!(serde_json::from_str::<TableStyle>(&serde_json::to_string(&table).unwrap()).unwrap(), table);
    }

    #[test]
    fn table_region_flags_cover_footer_right_column_and_rtl() {
        let cells = [
            CellStyle { name: "Body".into(), fill: Some("Body".into()), ..Default::default() },
            CellStyle { name: "Footer".into(), fill: Some("Footer".into()), ..Default::default() },
            CellStyle { name: "Right".into(), fill: Some("Right".into()), ..Default::default() },
        ];
        let mut style = TableStyle {
            body: Some("Body".into()),
            footer: Some("Footer".into()),
            right_column: Some("Right".into()),
            footer_same_as_body: Some(false),
            right_column_same_as_body: Some(false),
            ..Default::default()
        };
        for direction in [crate::TextDirection::LeftToRight, crate::TextDirection::RightToLeft] {
            let mut table = crate::table::Table::new(1, 1, 3, 0, 1, 100.0);
            table.options.direction = direction;
            let right = if direction == crate::TextDirection::LeftToRight { 2 } else { 0 };
            style.apply_to(&mut table, &cells);
            assert_eq!(table.cell(0, right).unwrap().fill, "Right");
            assert_eq!(table.cell(1, right).unwrap().fill, "Footer");
        }
        style.footer_same_as_body = Some(true);
        style.right_column_same_as_body = Some(true);
        let mut table = crate::table::Table::new(1, 1, 3, 0, 1, 100.0);
        style.apply_to(&mut table, &cells);
        assert!(table.cells.iter().all(|c| c.fill == "Body"));
    }

    #[test]
    fn single_column_tables_use_a_configured_right_region_when_left_is_absent() {
        let cells = [CellStyle { name: "Right".into(), fill: Some("Right".into()), ..Default::default() }];
        let style = TableStyle { right_column: Some("Right".into()), ..Default::default() };
        let mut table = crate::table::Table::new(1, 1, 1, 0, 0, 100.0);
        style.apply_to(&mut table, &cells);
        assert_eq!(table.cell(0, 0).unwrap().fill, "Right");
    }

    #[test]
    fn cell_and_table_style_resolution_stop_at_the_depth_limit() {
        let cells: Vec<_> = (0..40)
            .map(|i| CellStyle {
                name: i.to_string(),
                based_on: Some((i + 1).to_string()),
                fill: (i == 32).then(|| "Too deep".into()),
                fill_tint: (i == 31).then_some(0.5),
                ..Default::default()
            })
            .collect();
        let resolved = resolve_cell_style(&cells, "0");
        assert!(resolved.fill.is_none());
        assert_eq!(resolved.fill_tint, Some(0.5));
        let styles = Styles {
            table: (0..40)
                .map(|i| TableStyle {
                    name: i.to_string(),
                    based_on: Some((i + 1).to_string()),
                    space_before: (i == 32).then_some(123.0),
                    space_after: (i == 31).then_some(15.0),
                    ..Default::default()
                })
                .collect(),
            ..Styles::default()
        };
        let resolved = styles.resolve_table_style("0");
        assert!(resolved.space_before.is_none());
        assert_eq!(resolved.space_after, Some(15.0));
    }

    #[test]
    fn character_style_attrs_include_its_based_on_chain_only() {
        let mut st = Styles::default();
        let cs = |name: &str, based_on: Option<&str>, chars| CharacterStyle {
            name: name.into(),
            based_on: based_on.map(Into::into),
            chars,
            shortcut: String::new(),
        };
        st.character.push(cs("Base", None, CharAttrs { font_style: Some("Bold".into()), size: Some(9.0), ..Default::default() }));
        st.character.push(cs("Strong", Some("Base"), CharAttrs { size: Some(14.0), ..Default::default() }));
        st.character.push(cs("Loop", Some("Loop"), CharAttrs { tracking: Some(5.0), ..Default::default() }));
        assert_eq!(st.char_style_attrs("Strong"), CharAttrs { font_style: Some("Bold".into()), size: Some(14.0), ..Default::default() });
        assert_eq!(st.char_style_attrs("Loop"), CharAttrs { tracking: Some(5.0), ..Default::default() });
        assert!(st.char_style_attrs(NO_CHAR_STYLE).is_empty());
        assert!(st.char_style_attrs("Missing").is_empty());
    }

    #[test]
    fn unique_names() {
        let names = ["Body", "Body 2"];
        assert_eq!(Styles::unique_name(|n| names.contains(&n), "Body"), "Body 3");
        assert_eq!(Styles::unique_name(|n| names.contains(&n), "Head"), "Head");
    }
}
