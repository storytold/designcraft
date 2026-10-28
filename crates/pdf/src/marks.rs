//! Printer's marks drawn in the slug area: crop marks at the trim corners, bleed marks at the bleed
//! corners, registration targets beside each edge, colour bars above the page and a
//! page-information line. The marks use `[Registration]` (every plate); the colour bars are
//! process inks.

use craft_pdf::write::backend as krilla;
use designcraft_geom::kurbo::Shape as _;
use designcraft_geom::{BezPath, Point};
use krilla::num::NormalizedF32;
use krilla::paint::{LineCap, LineJoin, Stroke};
use krilla::surface::Surface;

use crate::export::{Exporter, MARK_LEN, Sheet, civil, mark_start, registration, to_path};

impl Exporter<'_> {
    pub(crate) fn marks(&mut self, s: &mut Surface, sh: &Sheet, title: &str, created: Option<i64>) {
        let m = self.opts.marks;
        if !m.any() {
            return;
        }
        let bleed = crate::export::bleed_values(self.doc, self.opts);
        let start = mark_start(self.opts, bleed);
        let t = sh.trim;
        let mut bp = BezPath::new();
        let mut circles = BezPath::new();
        let mut seg = |a: (f64, f64), b: (f64, f64)| {
            bp.move_to(Point::new(a.0, a.1));
            bp.line_to(Point::new(b.0, b.1));
        };
        if m.crop {
            // Each trim corner: a horizontal mark beside it and a vertical one above/below it.
            for (x, sx) in [(t.x0, -1.0), (t.x1, 1.0)] {
                for (y, sy) in [(t.y0, -1.0), (t.y1, 1.0)] {
                    seg((x + sx * start, y), (x + sx * (start + MARK_LEN), y));
                    seg((x, y + sy * start), (x, y + sy * (start + MARK_LEN)));
                }
            }
        }
        let b = sh.bleed;
        if m.bleed && (b.x0 < t.x0 || b.y0 < t.y0 || b.x1 > t.x1 || b.y1 > t.y1) {
            // Shorter marks on the bleed lines, outside the crop-mark start.
            let len = MARK_LEN / 2.0;
            for (x, bx, sx) in [(t.x0, b.x0, -1.0), (t.x1, b.x1, 1.0)] {
                for (y, by, sy) in [(t.y0, b.y0, -1.0), (t.y1, b.y1, 1.0)] {
                    seg((x + sx * start, by), (x + sx * (start + len), by));
                    seg((bx, y + sy * start), (bx, y + sy * (start + len)));
                }
            }
        }
        if m.registration {
            // A target (circle and cross) centred outside the middle of each trim edge.
            let r = (MARK_LEN / 2.0 - 2.0).max(3.0);
            let d = start + MARK_LEN / 2.0;
            let (cx, cy) = ((t.x0 + t.x1) / 2.0, (t.y0 + t.y1) / 2.0);
            // The page information runs along the bottom from the left: the bottom target moves
            // to the last quarter, clear of it.
            let bx = if m.page_info { t.x0 + t.width() * 0.75 } else { cx };
            for (x, y) in [(cx, t.y0 - d), (bx, t.y1 + d), (t.x0 - d, cy), (t.x1 + d, cy)] {
                seg((x - r - 2.0, y), (x + r + 2.0, y));
                seg((x, y - r - 2.0), (x, y + r + 2.0));
                circles.extend(designcraft_geom::kurbo::Circle::new((x, y), r).path_elements(0.05));
                circles.extend(designcraft_geom::kurbo::Circle::new((x, y), r / 2.0).path_elements(0.05));
            }
        }
        bp.extend(circles);
        if m.color_bars {
            self.color_bars(s, sh, start);
        }
        if let Some(p) = to_path(&bp) {
            s.set_fill(None);
            s.set_stroke(Some(Stroke {
                paint: registration(1.0, self.rgb_only).into(),
                width: m.weight.max(0.05) as f32,
                miter_limit: 4.0,
                line_cap: LineCap::Butt,
                line_join: LineJoin::Miter,
                opacity: NormalizedF32::ONE,
                dash: None,
            }));
            s.draw_path(&p);
            s.set_stroke(None);
        }
        if m.page_info {
            let mut info = format!("{title}    Page {}", sh.label);
            if let Some(c) = created.map(civil) {
                info.push_str(&format!("    {:04}-{:02}-{:02} {:02}:{:02} UTC", c.0, c.1, c.2, c.3, c.4));
            }
            let at = (t.x0 + start + 6.0, t.y1 + start + 10.0);
            self.plain_text(s, &info, at, 6.0, registration(1.0, self.rgb_only));
        }
    }

    /// Colour bars above the page: solid process inks and their overprints, then black tints.
    fn color_bars(&mut self, s: &mut Surface, sh: &Sheet, start: f64) {
        use designcraft_color::Color;
        let size = (MARK_LEN - 4.0).max(6.0);
        let t = sh.trim;
        let y = t.y0 - start - size - 2.0;
        let solids = [
            Color::cmyk(1.0, 0.0, 0.0, 0.0),
            Color::cmyk(0.0, 1.0, 0.0, 0.0),
            Color::cmyk(0.0, 0.0, 1.0, 0.0),
            Color::cmyk(0.0, 0.0, 0.0, 1.0),
            Color::cmyk(1.0, 1.0, 0.0, 0.0),
            Color::cmyk(1.0, 0.0, 1.0, 0.0),
            Color::cmyk(0.0, 1.0, 1.0, 0.0),
        ];
        let tints = (1..=10).map(|i| Color::cmyk(0.0, 0.0, 0.0, i as f32 / 10.0));
        let patches: Vec<Color> = solids.into_iter().chain(tints).collect();
        s.set_stroke(None);
        for (c, x) in patches.iter().zip(bar_xs(t, size, patches.len(), self.opts.marks.registration)) {
            if let Some(p) = to_path(&designcraft_geom::Rect::new(x, y, x + size, y + size).to_path(0.1)) {
                s.set_fill(Some(krilla::paint::Fill { paint: crate::export::device(c, self.rgb_only).into(), ..Default::default() }));
                s.draw_path(&p);
            }
        }
        s.set_fill(None);
    }
}

/// Where the colour-bar patches (`size` wide, at most `n`) start along the top: from the left of
/// the trim `t`, stopping short of the top registration target in the middle when there is one,
/// fewer when the page is narrow.
fn bar_xs(t: designcraft_geom::Rect, size: f64, n: usize, registration: bool) -> Vec<f64> {
    let x0 = t.x0 + 6.0;
    let room = if registration { (t.x0 + t.x1) / 2.0 - MARK_LEN / 2.0 - 4.0 } else { t.x1 - 6.0 };
    let fit = if size > 0.0 { ((room - x0) / size).floor().max(0.0) as usize } else { 0 };
    (0..fit.min(n)).map(|i| x0 + size * i as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The patches end before the top registration target (centred, `MARK_LEN` / 2 − 2 in
    /// radius plus its cross arms) and never pass the trim.
    #[test]
    fn colour_bars_stay_clear_of_the_registration_target() {
        let t = designcraft_geom::Rect::new(0.0, 0.0, 612.0, 792.0);
        let target_left = 306.0 - (MARK_LEN / 2.0 - 2.0) - 2.0;
        let xs = bar_xs(t, 14.0, 17, true);
        assert_eq!(xs.len(), 17, "a Letter page has room for all");
        assert!(xs.iter().all(|x| x + 14.0 <= target_left), "{xs:?}");
        // A narrow page takes fewer, still clear of the target.
        let narrow = designcraft_geom::Rect::new(0.0, 0.0, 144.0, 200.0);
        let xs = bar_xs(narrow, 14.0, 17, true);
        assert!(!xs.is_empty() && xs.len() < 17);
        assert!(xs.iter().all(|x| x + 14.0 <= 72.0 - (MARK_LEN / 2.0 - 2.0) - 2.0), "{xs:?}");
        // Without registration marks they may run across the page, inside the trim.
        let xs = bar_xs(narrow, 14.0, 17, false);
        assert!(xs.iter().all(|x| x + 14.0 <= 144.0));
        assert!(bar_xs(narrow, 0.0, 17, true).is_empty());
    }
}
