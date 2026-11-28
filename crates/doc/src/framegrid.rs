//! Frame grids: stories set in a grid of character cells, as Chinese, Japanese and Korean text
//! is on squared manuscript paper (原稿用紙 / 稿紙 / 원고지; JLREQ §4.2, and the same character grid
//! in CLREQ and KLREQ).
//!
//! The grid belongs to the story (as IDML's `StoryPreference FrameType="FrameGridType"` and the
//! story's `GridDataInformation`), so every frame of a thread shows the same cells. A cell is the
//! grid's character size scaled by its horizontal and vertical scale; cells follow each other
//! along a line `char_aki` apart, and lines (rows) `line_aki` apart.
//!
//! The grid gives the rows a paragraph's Grid Alignment puts its lines on, in place of the baseline
//! grid (see `designcraft-compose`); text whose paragraphs don't align to a grid follows its
//! leading. Lengths are points.

use serde::{Deserialize, Serialize};

/// Largest character size and aki a frame grid may have (hostile files and parameters).
pub const MAX_GRID_SIZE: f64 = 1000.0;

/// The grid of a frame grid story (Object › Frame Grid Options).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FrameGrid {
    /// Character size: the cell is this square, scaled by `h_scale` and `v_scale`.
    pub size: f64,
    /// Horizontal and vertical scale of the cell, 1.0 = 100 %.
    pub h_scale: f64,
    pub v_scale: f64,
    /// Character aki: space between cells along a line (negative overlaps them).
    pub char_aki: f64,
    /// Line aki: space between rows.
    pub line_aki: f64,
}

impl Default for FrameGrid {
    fn default() -> Self {
        FrameGrid { size: 12.0, h_scale: 1.0, v_scale: 1.0, char_aki: 0.0, line_aki: 6.0 }
    }
}

impl FrameGrid {
    /// The grid with every number finite and in range: a hostile file or parameter can't make
    /// cells of no size, rows that overlap backwards, or sizes that overflow.
    pub fn sanitized(&self) -> FrameGrid {
        let fin = |v: f64, d: f64| if v.is_finite() { v } else { d };
        let size = fin(self.size, 12.0).clamp(0.5, MAX_GRID_SIZE);
        let h_scale = fin(self.h_scale, 1.0).clamp(0.01, 10.0);
        let v_scale = fin(self.v_scale, 1.0).clamp(0.01, 10.0);
        // Aki may overlap cells by at most 90 % of their smaller side, so rows and cells always
        // move forward, whichever way the text runs.
        let overlap = -0.9 * size * h_scale.min(v_scale);
        FrameGrid {
            size,
            h_scale,
            v_scale,
            char_aki: fin(self.char_aki, 0.0).clamp(overlap, MAX_GRID_SIZE),
            line_aki: fin(self.line_aki, 0.0).clamp(overlap, MAX_GRID_SIZE),
        }
    }

    /// Cell extent (along the line, across lines). Vertical text runs along the glyphs' height.
    pub fn cell(&self, vertical: bool) -> (f64, f64) {
        let (w, h) = (self.size * self.h_scale, self.size * self.v_scale);
        if vertical { (h, w) } else { (w, h) }
    }

    /// Distance from one row to the next: the cell across lines plus the line aki.
    pub fn row_pitch(&self, vertical: bool) -> f64 {
        self.cell(vertical).1 + self.line_aki
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_grids_are_brought_into_range() {
        let g = FrameGrid { size: f64::NAN, h_scale: 0.0, v_scale: f64::INFINITY, char_aki: -1e9, line_aki: 1e12 }.sanitized();
        assert_eq!(g.size, 12.0);
        assert_eq!((g.h_scale, g.v_scale), (0.01, 1.0));
        assert!(g.char_aki > -g.size && g.line_aki <= MAX_GRID_SIZE, "{g:?}");
        assert!(g.row_pitch(false) > 0.0);
        // A flat cell can't take a line aki that runs the rows backwards.
        let flat = FrameGrid { size: 0.0, v_scale: 0.0, line_aki: -1e9, ..Default::default() }.sanitized();
        assert!(flat.row_pitch(false) > 0.0 && flat.row_pitch(true) > 0.0, "{flat:?}");
    }

    #[test]
    fn rows_follow_the_cell_across_lines() {
        let g = FrameGrid { size: 10.0, h_scale: 0.8, v_scale: 1.2, char_aki: 1.0, line_aki: 6.0 };
        assert_eq!(g.cell(false), (8.0, 12.0));
        assert_eq!(g.row_pitch(false), 18.0);
        assert_eq!(g.cell(true), (12.0, 8.0));
        assert_eq!(g.row_pitch(true), 14.0);
    }
}
