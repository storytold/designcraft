use super::{Deserialize, FirstBaseline, ParaFormat, Serialize, Story, StoryId, StrokeType, VerticalJustification};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RowHeightMode {
    /// Grows with the content (the height is a minimum).
    #[default]
    AtLeast,
    Exactly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RowKind {
    Header,
    #[default]
    Body,
    Footer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Row {
    pub height: f64,
    pub mode: RowHeightMode,
    pub kind: RowKind,
}

impl Default for Row {
    fn default() -> Self {
        Row { height: 3.0, mode: RowHeightMode::AtLeast, kind: RowKind::Body }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub width: f64,
}

/// One cell edge or the table border.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CellStroke {
    pub weight: f64,
    pub color: String,
    pub tint: f32,
    pub kind: StrokeType,
    /// Painted in the gaps of dashed, dotted and striped types.
    #[serde(skip_serializing_if = "is_no_swatch")]
    pub gap_color: String,
    #[serde(skip_serializing_if = "is_full")]
    pub gap_tint: f32,
}

fn is_no_swatch(s: &str) -> bool {
    s == designcraft_color::swatch::NONE
}

fn is_full(t: &f32) -> bool {
    *t == 1.0
}

impl Default for CellStroke {
    fn default() -> Self {
        CellStroke {
            weight: 1.0,
            color: designcraft_color::swatch::BLACK.into(),
            tint: 1.0,
            kind: StrokeType::Solid,
            gap_color: designcraft_color::swatch::NONE.into(),
            gap_tint: 1.0,
        }
    }
}

impl CellStroke {
    pub fn none() -> Self {
        CellStroke { weight: 0.0, ..Default::default() }
    }
    pub fn is_visible(&self) -> bool {
        self.weight > 0.0 && self.color != designcraft_color::swatch::NONE
    }
}

/// Table-level alternating row/column strokes. Retained independently of cell formatting
/// so an explicit cell style or local edge continues to take precedence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AltStrokes {
    pub first: u32,
    pub next: u32,
    pub first_stroke: CellStroke,
    pub next_stroke: CellStroke,
    pub skip_first: u32,
    pub skip_last: u32,
}

impl Default for AltStrokes {
    fn default() -> Self {
        Self {
            first: 0,
            next: 0,
            first_stroke: CellStroke::default(),
            next_stroke: CellStroke { weight: 0.25, ..Default::default() },
            skip_first: 0,
            skip_last: 0,
        }
    }
}

impl AltStrokes {
    /// A phase-independent stroke for unskipped, visually uniform patterns. Unequal
    /// patterns and skip settings are preserved for round trips until their boundary
    /// phase is supported; they must not be rendered with a guessed phase.
    pub fn uniform_stroke(&self) -> Option<&CellStroke> {
        if self.skip_first > 0 || self.skip_last > 0 {
            return None;
        }
        match (self.first > 0, self.next > 0) {
            (true, false) => Some(&self.first_stroke),
            (false, true) => Some(&self.next_stroke),
            (true, true) if self.first_stroke == self.next_stroke || (!self.first_stroke.is_visible() && !self.next_stroke.is_visible()) => {
                Some(&self.first_stroke)
            }
            _ => None,
        }
    }
}

fn no_flags(flags: &[bool; 4]) -> bool {
    !flags.iter().any(|v| *v)
}

fn no_priorities(priorities: &[i32; 4]) -> bool {
    priorities.iter().all(|v| *v == 0)
}

fn one() -> u32 {
    1
}

fn full() -> f32 {
    1.0
}

fn no_fill() -> String {
    designcraft_color::swatch::NONE.into()
}

fn is_ascent(f: &FirstBaseline) -> bool {
    *f == FirstBaseline::Ascent
}

fn is_zero_min(v: &f64) -> bool {
    *v == 0.0
}

fn default_insets() -> [f64; 4] {
    [4.0; 4]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    #[serde(default = "one")]
    pub row_span: u32,
    #[serde(default = "one")]
    pub col_span: u32,
    /// Cell content (a frameless story).
    pub text: Story,
    #[serde(default = "no_fill")]
    pub fill: String,
    #[serde(default = "full")]
    pub fill_tint: f32,
    /// Top, left, bottom, right.
    #[serde(default = "default_insets")]
    pub insets: [f64; 4],
    #[serde(default)]
    pub vj: VerticalJustification,
    /// Cell Options › Text › First Baseline: how far the first line's baseline sits below the
    /// top inset (IDML `FirstBaselineOffset`), and its minimum (`MinimumFirstBaselineOffset`).
    #[serde(default, skip_serializing_if = "is_ascent")]
    pub first_baseline: FirstBaseline,
    #[serde(default, skip_serializing_if = "is_zero_min")]
    pub first_baseline_min: f64,
    /// Text rotation in degrees (only 0 is composed for now).
    #[serde(default)]
    pub rotation: f64,
    /// Edge strokes: top, left, bottom, right.
    #[serde(default)]
    pub strokes: [CellStroke; 4],
    /// Explicit cell-edge formatting takes precedence over the table border, in the same
    /// top, left, bottom, right order as `strokes`. Older documents keep their table border.
    #[serde(default)]
    pub border_overrides: [bool; 4],
    /// Whether an edge has supplied cell/style attributes, independently of perimeter priority.
    #[serde(default, skip_serializing_if = "no_flags")]
    pub stroke_defined: [bool; 4],
    /// IDML edge-conflict priorities, in top, left, bottom, right order.
    #[serde(default, skip_serializing_if = "no_priorities")]
    pub stroke_priorities: [i32; 4],
    /// Applied cell style ("" = [None]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style: String,
    /// Graphic cell: an image in the cell instead of text (its `xf` maps into the cell's
    /// content box, origin at the box's top left).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphic: Option<crate::Graphic>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            row_span: 1,
            col_span: 1,
            text: Story::new(StoryId(0)),
            fill: no_fill(),
            fill_tint: 1.0,
            insets: default_insets(),
            vj: VerticalJustification::Top,
            first_baseline: FirstBaseline::Ascent,
            first_baseline_min: 0.0,
            rotation: 0.0,
            strokes: Default::default(),
            border_overrides: [false; 4],
            stroke_priorities: [0; 4],
            stroke_defined: [false; 4],
            style: String::new(),
            graphic: None,
        }
    }
}

impl Cell {
    pub fn with_text(text: &str, para: ParaFormat) -> Cell {
        Cell { text: Story::with_text(StoryId(0), text, para), ..Default::default() }
    }
    /// A cell with this cell's formatting and no text (new rows / columns, unmerged cells).
    pub fn blank_like(&self) -> Cell {
        let mut c = Cell { row_span: 1, col_span: 1, ..self.clone() };
        let para = self.text.paras.first().cloned().map(|p| ParaFormat { table: None, ..p }).unwrap_or_default();
        let fmt = self.text.chars.first().map(|r| r.format.clone()).unwrap_or_default();
        c.text = Story::new(StoryId(0));
        c.text.paras[0] = para;
        c.text.chars[0].format = fmt;
        c
    }
    pub fn has_fill(&self) -> bool {
        self.fill != designcraft_color::swatch::NONE
    }
}

/// Table → Table Options → Fills (alternating pattern).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AltFills {
    pub first: u32,
    pub first_color: String,
    pub first_tint: f32,
    pub next: u32,
    pub next_color: String,
    pub next_tint: f32,
    pub skip_first: u32,
    pub skip_last: u32,
}

impl Default for AltFills {
    fn default() -> Self {
        AltFills {
            first: 1,
            first_color: designcraft_color::swatch::BLACK.into(),
            first_tint: 0.2,
            next: 1,
            next_color: designcraft_color::swatch::NONE.into(),
            next_tint: 1.0,
            skip_first: 0,
            skip_last: 0,
        }
    }
}

impl AltFills {
    /// Fill for the `i`-th of `n` body rows/columns, if the pattern paints it.
    pub fn fill_for(&self, i: usize, n: usize) -> Option<(&str, f32)> {
        let (sf, sl) = (self.skip_first as usize, self.skip_last as usize);
        if i < sf || i >= n.saturating_sub(sl) {
            return None;
        }
        let period = (u64::from(self.first) + u64::from(self.next)).max(1);
        let k = (i - sf) as u64 % period;
        let (c, t) = if k < u64::from(self.first) { (&self.first_color, self.first_tint) } else { (&self.next_color, self.next_tint) };
        (c != designcraft_color::swatch::NONE).then_some((c.as_str(), t))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TableOptions {
    /// Logical column zero is drawn at the right edge for RTL tables.
    pub direction: crate::TextDirection,
    /// Outer border (overrides the outer cell edges).
    pub border: CellStroke,
    /// Per-side borders; absent sides use the legacy uniform `border`.
    #[serde(skip_serializing_if = "no_borders")]
    pub borders: [Option<CellStroke>; 4],
    pub space_before: f64,
    pub space_after: f64,
    /// Header rows repeat at the top of every text column / frame the table continues into.
    pub repeat_header: bool,
    pub repeat_footer: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_rows: Option<AltFills>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_cols: Option<AltFills>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_strokes: Option<AltStrokes>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_strokes: Option<AltStrokes>,
}

fn no_borders(borders: &[Option<CellStroke>; 4]) -> bool {
    borders.iter().all(Option::is_none)
}

impl TableOptions {
    pub fn border_for(&self, side: usize) -> &CellStroke {
        self.borders.get(side).and_then(Option::as_ref).unwrap_or(&self.border)
    }
}

impl Default for TableOptions {
    fn default() -> Self {
        TableOptions {
            direction: crate::TextDirection::LeftToRight,
            border: CellStroke::default(),
            borders: Default::default(),
            space_before: 4.0,
            space_after: -4.0,
            repeat_header: true,
            repeat_footer: true,
            alt_rows: None,
            alt_cols: None,
            row_strokes: None,
            column_strokes: None,
        }
    }
}
