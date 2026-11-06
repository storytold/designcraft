//! Printer's marks drawn in the slug area: crop marks at the trim corners, bleed marks at the bleed
//! corners, and a page-information line. All marks use `[Registration]` (every plate).

use craft_pdf::write::backend as krilla;
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
        let bleed = if self.opts.bleed { self.doc.settings.bleed.map(|v| v.max(0.0)) } else { [0.0; 4] };
        let start = mark_start(self.opts, bleed);
        let t = sh.trim;
        let mut bp = BezPath::new();
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
}
