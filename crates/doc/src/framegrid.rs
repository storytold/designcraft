//! Frame grids: text frames whose text is set in a grid of character cells (Japanese
//! *genkō yōshi*-style composition, JLREQ §4.2 *hanmen* and frame grids).
//!
//! A frame grid is an ordinary text frame with a [`FrameGrid`] in its options. The grid gives the
//! cell (character size × scale), the character aki (space between cells along the line) and the
//! line aki (space between lines); the frame's columns and gutter divide it as usual. Composition
//! (in `designcraft-compose`) puts full-width characters of the grid's size one per cell and snaps
//! every line to the grid's rows. Character and line counts are not stored: they follow from the
//! frame's size, so resizing a frame grid adds or removes cells.
//!
//! Lengths are points. "Along" is the line direction (left to right in horizontal frames, top to
//! bottom in vertical ones), "across" the direction lines follow each other in.

use designcraft_geom::Rect;
use serde::{Deserialize, Serialize};

use crate::Align;

/// Where a line sits in the grid rows it occupies (Grid Alignment), and where a character smaller
/// than the cell sits in it (Character Alignment).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridAlignment {
    /// Lines follow each other at the grid's line pitch without snapping to rows.
    None,
    /// The Roman baseline of the line on the Roman baseline of the cell.
    RomanBaseline,
    /// The top (right, in vertical text) of the em box on the top of the cell.
    EmTop,
    /// The middle of the em box on the middle of the cell.
    #[default]
    EmCenter,
    /// The bottom (left) of the em box on the bottom of the cell.
    EmBottom,
    /// The ideographic character face (ICF) top on the cell's ICF top.
    IcfTop,
    /// The ICF bottom on the cell's ICF bottom.
    IcfBottom,
}

impl GridAlignment {
    pub const ALL: [GridAlignment; 7] = [
        GridAlignment::None,
        GridAlignment::RomanBaseline,
        GridAlignment::EmTop,
        GridAlignment::EmCenter,
        GridAlignment::EmBottom,
        GridAlignment::IcfTop,
        GridAlignment::IcfBottom,
    ];
    pub fn label(self) -> &'static str {
        match self {
            GridAlignment::None => "None",
            GridAlignment::RomanBaseline => "Roman Baseline",
            GridAlignment::EmTop => "Em Box Top",
            GridAlignment::EmCenter => "Em Box Center",
            GridAlignment::EmBottom => "Em Box Bottom",
            GridAlignment::IcfTop => "ICF Top",
            GridAlignment::IcfBottom => "ICF Bottom",
        }
    }
}

/// Where the character count of a frame grid is shown (on screen only).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridCount {
    None,
    Top,
    #[default]
    Bottom,
    Left,
    Right,
}

impl GridCount {
    pub const ALL: [GridCount; 5] = [GridCount::None, GridCount::Top, GridCount::Bottom, GridCount::Left, GridCount::Right];
}

/// How a frame grid shows on screen (never printed).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridView {
    /// Every cell.
    #[default]
    Grid,
    /// The N/Z view: only the frame, with a diagonal showing the text direction.
    NZ,
    /// The cells of the first and last line and column only (a light view for large grids).
    Outline,
}

impl GridView {
    pub const ALL: [GridView; 3] = [GridView::Grid, GridView::NZ, GridView::Outline];
}

/// The grid of a frame grid (Object › Frame Grid Options).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FrameGrid {
    /// Grid font (the font Apply Grid Format gives the text). Empty keeps the text's font.
    pub font_family: String,
    pub font_style: String,
    /// Character size: the cell is this square, scaled by `h_scale` and `v_scale`.
    pub size: f64,
    /// Horizontal and vertical scale of the cell and of the grid font, 1.0 = 100 %.
    pub h_scale: f64,
    pub v_scale: f64,
    /// Character aki: space between cells along the line (negative overlaps them).
    pub char_aki: f64,
    /// Line aki: space between lines.
    pub line_aki: f64,
    /// Line alignment of the frame's text; `None` follows each paragraph's alignment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_align: Option<Align>,
    /// Grid alignment: where lines sit in their rows.
    pub grid_align: GridAlignment,
    /// Character alignment: where characters smaller than the line's largest sit.
    pub char_align: GridAlignment,
    /// Character count position and its type size.
    pub count: GridCount,
    pub count_size: f64,
    pub view: GridView,
    /// Mojikumi set for text in the grid (a paragraph's own set wins). Empty: none, every
    /// character and punctuation mark takes a whole cell. New grids use `DEFAULT_GRID_MOJIKUMI`.
    /// Always written: a missing value reads as the default set, an empty one as none.
    pub mojikumi: String,
}

/// The grid font new documents give their frame grids (a Mincho, as Japanese body text is set in).
pub const DEFAULT_GRID_FONT: (&str, &str) = ("Hiragino Mincho ProN", "W3");

/// Mincho faces that stand in for [`DEFAULT_GRID_FONT`] where it isn't installed (Windows,
/// Linux): (family, style), in order of preference.
pub const GRID_FONT_FALLBACKS: &[(&str, &str)] = &[
    ("Hiragino Mincho ProN", "W3"),
    ("Yu Mincho", "Regular"),
    ("YuMincho", "Regular"),
    ("MS Mincho", "Regular"),
    ("Noto Serif CJK JP", "Regular"),
    ("Source Han Serif JP", "Regular"),
    ("Noto Serif JP", "Regular"),
    ("IPAexMincho", "Regular"),
];

/// The mojikumi set of new frame grids: full-width punctuation that closes up between
/// consecutive marks, half-width at the line end (JIS X 4051 and JLREQ §3.1's basic setting).
pub const DEFAULT_GRID_MOJIKUMI: &str = "lineEndHalf";

impl Default for FrameGrid {
    fn default() -> Self {
        FrameGrid {
            font_family: DEFAULT_GRID_FONT.0.into(),
            font_style: DEFAULT_GRID_FONT.1.into(),
            size: 12.0,
            h_scale: 1.0,
            v_scale: 1.0,
            char_aki: 0.0,
            line_aki: 6.0,
            line_align: None,
            grid_align: GridAlignment::EmCenter,
            char_align: GridAlignment::EmCenter,
            count: GridCount::Bottom,
            count_size: 9.0,
            view: GridView::Grid,
            mojikumi: DEFAULT_GRID_MOJIKUMI.into(),
        }
    }
}

/// Largest character size, aki and count a grid may have (hostile files and parameters).
pub const MAX_GRID_SIZE: f64 = 1000.0;
pub const MAX_GRID_COUNT: u32 = 1000;

impl FrameGrid {
    /// The grid with every number finite and in range (sizes positive, scales 1 %–1000 %, aki
    /// no more negative than the cell).
    pub fn sanitized(&self) -> FrameGrid {
        let fin = |v: f64, d: f64| if v.is_finite() { v } else { d };
        let size = fin(self.size, 12.0).clamp(0.5, MAX_GRID_SIZE);
        let h_scale = fin(self.h_scale, 1.0).clamp(0.01, 10.0);
        let v_scale = fin(self.v_scale, 1.0).clamp(0.01, 10.0);
        let along = size * h_scale.max(v_scale);
        FrameGrid {
            size,
            h_scale,
            v_scale,
            char_aki: fin(self.char_aki, 0.0).clamp(-0.9 * along, MAX_GRID_SIZE),
            line_aki: fin(self.line_aki, 0.0).clamp(-0.9 * along, MAX_GRID_SIZE),
            count_size: fin(self.count_size, 9.0).clamp(1.0, 200.0),
            ..self.clone()
        }
    }

    /// Is every number finite (document validation)?
    pub fn is_finite(&self) -> bool {
        [self.size, self.h_scale, self.v_scale, self.char_aki, self.line_aki, self.count_size].iter().all(|v| v.is_finite())
    }

    /// Cell extent (along the line, across lines). Vertical text runs along the glyphs' height.
    pub fn cell(&self, vertical: bool) -> (f64, f64) {
        let (w, h) = (self.size * self.h_scale, self.size * self.v_scale);
        if vertical { (h, w) } else { (w, h) }
    }

    /// Distance from one cell to the next along the line.
    pub fn char_pitch(&self, vertical: bool) -> f64 {
        (self.cell(vertical).0 + self.char_aki).max(0.01)
    }

    /// Distance from one line to the next.
    pub fn line_pitch(&self, vertical: bool) -> f64 {
        (self.cell(vertical).1 + self.line_aki).max(0.01)
    }

    /// Length of a line of `chars` cells (no aki after the last).
    pub fn measure(&self, chars: u32, vertical: bool) -> f64 {
        f64::from(chars.max(1)) * self.char_pitch(vertical) - self.char_aki
    }

    /// Depth of `lines` lines (no aki after the last).
    pub fn depth(&self, lines: u32, vertical: bool) -> f64 {
        f64::from(lines.max(1)) * self.line_pitch(vertical) - self.line_aki
    }

    /// How many whole cells fit along `measure` and how many lines across `depth`.
    pub fn counts(&self, measure: f64, depth: f64, vertical: bool) -> (u32, u32) {
        let n = |len: f64, pitch: f64, aki: f64| {
            let v = ((len + aki) / pitch + 1e-6).floor();
            if v.is_finite() { v.clamp(0.0, f64::from(MAX_GRID_COUNT)) as u32 } else { 0 }
        };
        (n(measure, self.char_pitch(vertical), self.char_aki), n(depth, self.line_pitch(vertical), self.line_aki))
    }

    /// Text area (width, height) of a grid of `chars` × `lines` per column in `columns` columns
    /// `gutter` apart. Columns divide the line direction: side by side in horizontal frames,
    /// stacked in vertical ones.
    pub fn area_size(&self, chars: u32, lines: u32, columns: u32, gutter: f64, vertical: bool) -> (f64, f64) {
        let cols = f64::from(columns.clamp(1, 40));
        let measure = self.measure(chars, vertical) * cols + gutter.max(0.0) * (cols - 1.0);
        let depth = self.depth(lines, vertical);
        if vertical { (depth, measure) } else { (measure, depth) }
    }

    /// Characters per line and lines of a grid in the text `area` (turned box for vertical
    /// frames: lines along x) with `columns` columns `gutter` apart.
    pub fn counts_in(&self, area: Rect, columns: u32, gutter: f64, vertical: bool) -> (u32, u32) {
        let cols = f64::from(columns.clamp(1, 40));
        let col = (area.width() - gutter.max(0.0) * (cols - 1.0)) / cols;
        self.counts(col, area.height(), vertical)
    }

    /// The cell rectangles of one column, in composition space (lines along x, following each
    /// other down y), at most `limit` of them.
    pub fn cells(&self, col: Rect, vertical: bool, limit: usize) -> Vec<Rect> {
        let (along, across) = self.cell(vertical);
        let (chars, lines) = self.counts(col.width(), col.height(), vertical);
        let (cp, lp) = (self.char_pitch(vertical), self.line_pitch(vertical));
        let mut out = Vec::new();
        'rows: for l in 0..lines {
            let y = col.y0 + f64::from(l) * lp;
            for c in 0..chars {
                if out.len() >= limit {
                    break 'rows;
                }
                let x = col.x0 + f64::from(c) * cp;
                out.push(Rect::new(x, y, x + along, y + across));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_sizes_follow_counts_and_back() {
        let g = FrameGrid { size: 10.0, char_aki: 1.0, line_aki: 5.0, ..Default::default() };
        assert_eq!(g.measure(20, false), 20.0 * 11.0 - 1.0);
        assert_eq!(g.depth(3, false), 3.0 * 15.0 - 5.0);
        let (w, h) = g.area_size(20, 3, 2, 12.0, false);
        assert_eq!((w, h), (2.0 * 219.0 + 12.0, 40.0));
        assert_eq!(g.counts_in(Rect::new(0.0, 0.0, w, h), 2, 12.0, false), (20, 3));
        // Vertical: columns stack down the page, lines follow each other leftwards.
        let (w, h) = g.area_size(20, 3, 2, 12.0, true);
        assert_eq!((w, h), (40.0, 2.0 * 219.0 + 12.0));
        assert_eq!(g.counts_in(Rect::new(0.0, 0.0, h, w), 2, 12.0, true), (20, 3));
    }

    #[test]
    fn scales_shape_the_cell() {
        let g = FrameGrid { size: 10.0, h_scale: 0.8, ..Default::default() };
        assert_eq!(g.cell(false), (8.0, 10.0));
        assert_eq!(g.cell(true), (10.0, 8.0));
    }

    #[test]
    fn hostile_grids_are_sanitized() {
        let g = FrameGrid { size: f64::NAN, h_scale: 0.0, char_aki: -1e9, line_aki: f64::INFINITY, ..Default::default() }.sanitized();
        assert!(g.is_finite());
        assert!(g.char_pitch(false) > 0.0 && g.line_pitch(true) > 0.0);
        let (c, l) = FrameGrid::default().counts(1e300, f64::NAN, false);
        assert_eq!((c, l), (MAX_GRID_COUNT, 0));
        assert!(FrameGrid::default().cells(Rect::new(0.0, 0.0, 1e9, 1e9), false, 100).len() <= 100);
    }
}
