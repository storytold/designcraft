//! Imposition: copies of pages laid out n-up on a press sheet (step and repeat), the area each
//! copy's bleed may cover, and the crop marks at its trim edges. Sheet coordinates are points from
//! the sheet's top left, y pointing down.

use kurbo::{Line, Point, Rect, Size};
use serde::{Deserialize, Serialize};

/// The largest sheet side in points (200 in, the PDF page size limit).
pub const MAX_SHEET: f64 = 14_400.0;
/// The most copies one sheet takes.
pub const MAX_PER_SHEET: usize = 10_000;
/// The shortest crop mark drawn where the space is tight.
pub const MIN_MARK: f64 = 2.0;
const TOL: f64 = 1e-6;

/// Whether copies are turned 90° on the sheet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Rotation {
    /// The orientation that fits more copies (upright on a tie).
    #[default]
    Auto,
    Off,
    On,
}

/// Where crop marks go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CropMarks {
    #[default]
    None,
    /// At each copy's corners, where the gap leaves room.
    Corners,
    /// Along the sheet border at every trim line: cut lines for a guillotine.
    Border,
}

/// The press sheet and how copies sit on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub sheet: Size,
    /// Top, bottom, left, right.
    pub margins: [f64; 4],
    /// Between columns, between rows.
    pub gap: [f64; 2],
    pub rotation: Rotation,
    /// The copies centred inside the margins; otherwise from the top left margin corner.
    pub center: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ImposeError {
    #[error("the sheet must be more than 0 and at most 14400 pt on each side")]
    Sheet,
    #[error("margins and gaps must be numbers of 0 or more")]
    Spacing,
    #[error("the page has no size")]
    Page,
    #[error("the page doesn't fit on the sheet inside its margins")]
    NoFit,
    #[error("more than 10000 copies would go on a sheet")]
    TooMany,
}

/// The grid of copies on every sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    /// Copies are turned 90° clockwise.
    pub rotated: bool,
    /// A copy's trim size on the sheet (width and height swapped when rotated).
    pub cell: Size,
    /// The first copy's top left trim corner.
    pub origin: Point,
}

/// One copy on a sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub sheet: usize,
    /// Position on the sheet, row by row from the top left.
    pub slot: usize,
    pub trim: Rect,
    /// What the copy may paint: half the gap towards a neighbour, the sheet edge outside the grid.
    pub clip: Rect,
    pub rotated: bool,
}

impl Grid {
    pub fn per_sheet(&self) -> usize {
        self.cols.saturating_mul(self.rows)
    }

    /// Sheets needed for `copies`.
    pub fn sheets(&self, copies: usize) -> usize {
        copies.div_ceil(self.per_sheet().max(1))
    }

    /// Where copy `index` goes (copies fill sheets in order, each row left to right).
    pub fn place(&self, spec: &Spec, index: usize) -> Placement {
        let per = self.per_sheet().max(1);
        let cols = self.cols.max(1);
        let (sheet, slot) = (index / per, index % per);
        let (row, col) = (slot / cols, slot % cols);
        let [gx, gy] = spec.gap;
        let x0 = self.origin.x + col as f64 * (self.cell.width + gx);
        let y0 = self.origin.y + row as f64 * (self.cell.height + gy);
        let trim = Rect::new(x0, y0, x0 + self.cell.width, y0 + self.cell.height);
        let clip = Rect::new(
            if col == 0 { 0.0 } else { trim.x0 - gx / 2.0 },
            if row == 0 { 0.0 } else { trim.y0 - gy / 2.0 },
            if col + 1 >= cols { spec.sheet.width } else { trim.x1 + gx / 2.0 },
            if row + 1 >= self.rows { spec.sheet.height } else { trim.y1 + gy / 2.0 },
        );
        Placement { sheet, slot, trim, clip, rotated: self.rotated }
    }
}

fn count(avail: f64, cell: f64, gap: f64) -> usize {
    if avail + TOL < cell {
        return 0;
    }
    let n = ((avail - cell + TOL) / (cell + gap)).floor();
    if n >= MAX_PER_SHEET as f64 { MAX_PER_SHEET + 1 } else { n as usize + 1 }
}

fn spacing(v: f64) -> bool {
    v.is_finite() && v >= 0.0
}

/// How copies of a `page`-sized page fit on the sheet.
pub fn grid(spec: &Spec, page: Size) -> Result<Grid, ImposeError> {
    let side = |v: f64| v.is_finite() && v > 0.0 && v <= MAX_SHEET;
    if !side(spec.sheet.width) || !side(spec.sheet.height) {
        return Err(ImposeError::Sheet);
    }
    if !spec.margins.iter().chain(&spec.gap).all(|v| spacing(*v)) {
        return Err(ImposeError::Spacing);
    }
    if !(page.width.is_finite() && page.height.is_finite() && page.width > 0.0 && page.height > 0.0) {
        return Err(ImposeError::Page);
    }
    let [top, bottom, left, right] = spec.margins;
    let [gx, gy] = spec.gap;
    let aw = spec.sheet.width - left - right;
    let ah = spec.sheet.height - top - bottom;
    let upright = (count(aw, page.width, gx), count(ah, page.height, gy));
    let turned = (count(aw, page.height, gx), count(ah, page.width, gy));
    let n = |(c, r): (usize, usize)| c.saturating_mul(r);
    let rotated = match spec.rotation {
        Rotation::Off => false,
        Rotation::On => true,
        Rotation::Auto => n(turned) > n(upright),
    };
    let ((cols, rows), cell) = if rotated { (turned, Size::new(page.height, page.width)) } else { (upright, page) };
    match n((cols, rows)) {
        0 => return Err(ImposeError::NoFit),
        k if k > MAX_PER_SHEET => return Err(ImposeError::TooMany),
        _ => {}
    }
    let gw = cols as f64 * cell.width + (cols - 1) as f64 * gx;
    let gh = rows as f64 * cell.height + (rows - 1) as f64 * gy;
    let (dx, dy) = if spec.center { ((aw - gw).max(0.0) / 2.0, (ah - gh).max(0.0) / 2.0) } else { (0.0, 0.0) };
    Ok(Grid { cols, rows, rotated, cell, origin: Point::new(left + dx, top + dy) })
}

/// The crop marks of the copies `on_sheet` (all on one sheet). Marks start `offset` from the trim
/// edge, never inside the bleed a copy keeps, and run `len` long at most, shorter where they reach
/// a neighbour's half of the gap or the sheet edge; a mark with less than [`MIN_MARK`] of room is
/// left out.
pub fn crop_marks(spec: &Spec, on_sheet: &[Placement], mode: CropMarks, offset: f64, bleed: f64, len: f64) -> Vec<Line> {
    let offset = if spacing(offset) { offset } else { 0.0 };
    let bleed = if spacing(bleed) { bleed } else { 0.0 };
    let len = if spacing(len) { len } else { 0.0 };
    let mut out = Vec::new();
    match mode {
        CropMarks::None => {}
        CropMarks::Corners => {
            for p in on_sheet {
                let (t, c) = (p.trim, p.clip);
                for (x, sx, room_x) in [(t.x0, -1.0, t.x0 - c.x0), (t.x1, 1.0, c.x1 - t.x1)] {
                    for (y, sy, room_y) in [(t.y0, -1.0, t.y0 - c.y0), (t.y1, 1.0, c.y1 - t.y1)] {
                        if let Some((a, b)) = mark_span(room_x, offset, bleed, len) {
                            out.push(Line::new((x + sx * a, y), (x + sx * b, y)));
                        }
                        if let Some((a, b)) = mark_span(room_y, offset, bleed, len) {
                            out.push(Line::new((x, y + sy * a), (x, y + sy * b)));
                        }
                    }
                }
            }
        }
        CropMarks::Border => {
            let Some(g) = on_sheet.iter().map(|p| p.trim).reduce(|a, b| a.union(b)) else { return out };
            let lines = |f: fn(&Rect) -> [f64; 2]| {
                let mut v: Vec<f64> = on_sheet.iter().flat_map(|p| f(&p.trim)).collect();
                v.sort_by(f64::total_cmp);
                v.dedup_by(|a, b| (*a - *b).abs() < TOL);
                v
            };
            let (top, bottom) = (mark_span(g.y0, offset, bleed, len), mark_span(spec.sheet.height - g.y1, offset, bleed, len));
            for x in lines(|r| [r.x0, r.x1]) {
                if let Some((a, b)) = top {
                    out.push(Line::new((x, g.y0 - a), (x, g.y0 - b)));
                }
                if let Some((a, b)) = bottom {
                    out.push(Line::new((x, g.y1 + a), (x, g.y1 + b)));
                }
            }
            let (left, right) = (mark_span(g.x0, offset, bleed, len), mark_span(spec.sheet.width - g.x1, offset, bleed, len));
            for y in lines(|r| [r.y0, r.y1]) {
                if let Some((a, b)) = left {
                    out.push(Line::new((g.x0 - a, y), (g.x0 - b, y)));
                }
                if let Some((a, b)) = right {
                    out.push(Line::new((g.x1 + a, y), (g.x1 + b, y)));
                }
            }
        }
    }
    out
}

/// Start and end distance from the trim edge of a mark with `room` up to the clip edge.
fn mark_span(room: f64, offset: f64, bleed: f64, len: f64) -> Option<(f64, f64)> {
    let start = offset.max(bleed.min(room));
    let end = (start + len).min(room);
    (end - start >= MIN_MARK).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::PT_PER_MM;

    fn mm(w: f64, h: f64) -> Size {
        Size::new(w * PT_PER_MM, h * PT_PER_MM)
    }

    fn spec(sheet: Size) -> Spec {
        Spec { sheet, margins: [0.0; 4], gap: [0.0; 2], rotation: Rotation::Auto, center: true }
    }

    #[test]
    fn a6_on_a4_is_four_up_and_eight_copies_take_two_sheets() {
        let s = spec(mm(210.0, 297.0));
        let g = grid(&s, mm(105.0, 148.0)).unwrap();
        assert_eq!((g.cols, g.rows, g.rotated, g.per_sheet()), (2, 2, false, 4));
        assert_eq!(g.sheets(8), 2);
        assert_eq!(g.sheets(9), 3);
        let p = g.place(&s, 5);
        assert_eq!((p.sheet, p.slot), (1, 1));
        // Two A6 are 1 mm shorter than A4: centred, half a millimetre above and below.
        assert!((p.trim.x0 - 105.0 * PT_PER_MM).abs() < 1e-9 && (p.trim.y0 - 0.5 * PT_PER_MM).abs() < 1e-9, "{:?}", p.trim);
    }

    #[test]
    fn auto_rotation_takes_the_orientation_that_fits_more() {
        // 88 × 60 mm on A4 without gaps: 8 upright (2 × 4), 9 rotated (3 × 3).
        let mut s = spec(mm(210.0, 297.0));
        let page = mm(88.0, 60.0);
        let g = grid(&s, page).unwrap();
        assert_eq!((g.cols, g.rows, g.rotated), (3, 3, true));
        assert!((g.cell.width - 60.0 * PT_PER_MM).abs() < 1e-9);
        s.rotation = Rotation::Off;
        let g = grid(&s, page).unwrap();
        assert_eq!((g.cols, g.rows, g.rotated), (2, 4, false));
        s.rotation = Rotation::On;
        assert_eq!(grid(&s, page).unwrap().per_sheet(), 9);
        // A tie stays upright.
        s.rotation = Rotation::Auto;
        assert!(!grid(&s, mm(100.0, 100.0)).unwrap().rotated);
    }

    #[test]
    fn margins_gaps_and_centring_place_the_grid() {
        let s = Spec { sheet: Size::new(300.0, 200.0), margins: [10.0, 10.0, 20.0, 20.0], gap: [10.0, 4.0], rotation: Rotation::Off, center: true };
        let g = grid(&s, Size::new(80.0, 50.0)).unwrap();
        // 260 wide: 80 + 90 + 90 = 260 → 3 columns; 180 high: 50 + 54 + 54 = 158 → 3 rows.
        assert_eq!((g.cols, g.rows), (3, 3));
        assert_eq!(g.origin, Point::new(20.0, 10.0 + (180.0 - 158.0) / 2.0));
        let left = Spec { center: false, ..s };
        assert_eq!(grid(&left, Size::new(80.0, 50.0)).unwrap().origin, Point::new(20.0, 10.0));
        // The middle copy's clip is half the gap around its trim; corner copies reach the sheet edge.
        let mid = g.place(&s, 4);
        assert_eq!(mid.clip, Rect::new(mid.trim.x0 - 5.0, mid.trim.y0 - 2.0, mid.trim.x1 + 5.0, mid.trim.y1 + 2.0));
        let last = g.place(&s, 8);
        assert_eq!((last.clip.x1, last.clip.y1, last.clip.x0), (300.0, 200.0, last.trim.x0 - 5.0));
        assert_eq!(g.place(&s, 0).clip.origin(), Point::ZERO);
    }

    #[test]
    fn bad_specs_and_pages_are_refused() {
        let s = spec(Size::new(100.0, 100.0));
        assert_eq!(grid(&s, Size::new(120.0, 50.0)), Err(ImposeError::NoFit));
        assert_eq!(grid(&s, Size::new(0.0, 50.0)), Err(ImposeError::Page));
        assert_eq!(grid(&s, Size::new(f64::NAN, 50.0)), Err(ImposeError::Page));
        assert_eq!(grid(&spec(Size::new(f64::INFINITY, 100.0)), Size::new(1.0, 1.0)), Err(ImposeError::Sheet));
        assert_eq!(grid(&spec(Size::new(20_000.0, 100.0)), Size::new(1.0, 1.0)), Err(ImposeError::Sheet));
        assert_eq!(grid(&Spec { gap: [-1.0, 0.0], ..s }, Size::new(10.0, 10.0)), Err(ImposeError::Spacing));
        assert_eq!(grid(&Spec { margins: [f64::NAN, 0.0, 0.0, 0.0], ..s }, Size::new(10.0, 10.0)), Err(ImposeError::Spacing));
        assert_eq!(grid(&spec(Size::new(14_400.0, 14_400.0)), Size::new(1e-9, 1e-9)), Err(ImposeError::TooMany));
        // Margins wider than the sheet leave no room.
        assert_eq!(grid(&Spec { margins: [0.0, 0.0, 60.0, 60.0], ..s }, Size::new(10.0, 10.0)), Err(ImposeError::NoFit));
    }

    #[test]
    fn corner_marks_fit_in_the_gap_and_border_marks_run_along_the_sheet_edge() {
        let s = Spec { sheet: Size::new(240.0, 140.0), margins: [30.0; 4], gap: [40.0, 0.0], rotation: Rotation::Off, center: false };
        let g = grid(&s, Size::new(70.0, 80.0)).unwrap();
        assert_eq!((g.cols, g.rows), (2, 1));
        let on: Vec<Placement> = (0..2).map(|i| g.place(&s, i)).collect();
        // Copy 0's trim is 30..100 × 30..110, copy 1's 140..210 × 30..110.
        let marks = crop_marks(&s, &on, CropMarks::Corners, 6.0, 3.0, 18.0);
        // Into the 40 pt gap: from 6 pt off the trim to the gap's middle (14 pt long).
        assert!(marks.contains(&Line::new((106.0, 30.0), (120.0, 30.0))), "{marks:?}");
        assert!(marks.contains(&Line::new((134.0, 110.0), (120.0, 110.0))), "{marks:?}");
        // Into the 30 pt margin, full length.
        assert!(marks.contains(&Line::new((30.0, 24.0), (30.0, 6.0))), "{marks:?}");
        assert!(marks.contains(&Line::new((24.0, 30.0), (6.0, 30.0))), "{marks:?}");
        assert_eq!(marks.len(), 16);
        // Without a gap there's no room between copies: only the marks into the margins.
        let tight = Spec { gap: [0.0, 0.0], ..s };
        let g2 = grid(&tight, Size::new(70.0, 80.0)).unwrap();
        let on2: Vec<Placement> = (0..2).map(|i| g2.place(&tight, i)).collect();
        assert_eq!(crop_marks(&tight, &on2, CropMarks::Corners, 6.0, 3.0, 18.0).len(), 12);
        // A bleed wider than the offset pushes the marks out.
        let wide = crop_marks(&s, &on, CropMarks::Corners, 6.0, 9.0, 18.0);
        assert!(wide.contains(&Line::new((30.0, 21.0), (30.0, 3.0))), "{wide:?}");
        assert!(wide.contains(&Line::new((109.0, 30.0), (120.0, 30.0))), "{wide:?}");
        // Border: a vertical mark above and below the grid at each of the four trim x lines, a
        // horizontal one left and right at each of the two y lines.
        let border = crop_marks(&s, &on, CropMarks::Border, 6.0, 3.0, 18.0);
        assert_eq!(border.len(), 4 * 2 + 2 * 2);
        assert!(border.contains(&Line::new((140.0, 24.0), (140.0, 6.0))), "{border:?}");
        assert!(border.contains(&Line::new((140.0, 116.0), (140.0, 134.0))), "{border:?}");
        assert!(border.contains(&Line::new((216.0, 110.0), (234.0, 110.0))), "{border:?}");
        assert!(crop_marks(&s, &on, CropMarks::None, 6.0, 3.0, 18.0).is_empty());
        // Copies filling the sheet edge to edge leave no room for marks.
        let full = Spec { sheet: Size::new(240.0, 140.0), margins: [0.0; 4], gap: [0.0; 2], rotation: Rotation::Off, center: false };
        let g3 = grid(&full, Size::new(80.0, 70.0)).unwrap();
        assert_eq!(g3.per_sheet(), 6);
        let on3: Vec<Placement> = (0..6).map(|i| g3.place(&full, i)).collect();
        assert!(crop_marks(&full, &on3, CropMarks::Border, 6.0, 0.0, 18.0).is_empty());
        assert!(crop_marks(&full, &on3, CropMarks::Corners, 6.0, 0.0, 18.0).is_empty());
    }
}
