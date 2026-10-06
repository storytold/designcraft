//! DesignCraft's icon set, drawn in code (original artwork; no external icon assets).
//!
//! Icons are designed on a 20×20 grid and scaled to the target rect.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    c: Color32,
    w: f32,
}

impl Pen<'_> {
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        let s = self.r.width().min(self.r.height()) / 20.0;
        let o = self.r.center() - vec2(10.0 * s, 10.0 * s);
        pos2(o.x + x * s, o.y + y * s)
    }
    fn s(&self) -> f32 {
        self.r.width().min(self.r.height()) / 20.0
    }
    fn line(&self, pts: &[(f32, f32)]) {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pt(x, y)).collect();
        self.p.add(Shape::line(v, Stroke::new(self.w * self.s(), self.c)));
    }
    fn closed(&self, pts: &[(f32, f32)]) {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pt(x, y)).collect();
        self.p.add(Shape::closed_line(v, Stroke::new(self.w * self.s(), self.c)));
    }
    fn fill(&self, pts: &[(f32, f32)]) {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pt(x, y)).collect();
        self.p.add(Shape::convex_polygon(v, self.c, Stroke::NONE));
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.p.rect_stroke(
            Rect::from_min_max(self.pt(x0, y0), self.pt(x1, y1)),
            0.0,
            Stroke::new(self.w * self.s(), self.c),
            egui::StrokeKind::Middle,
        );
    }
    fn frect(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.p.rect_filled(Rect::from_min_max(self.pt(x0, y0), self.pt(x1, y1)), 0.0, self.c);
    }
    fn circle(&self, x: f32, y: f32, r: f32) {
        self.p.circle_stroke(self.pt(x, y), r * self.s(), Stroke::new(self.w * self.s(), self.c));
    }
    fn fcircle(&self, x: f32, y: f32, r: f32) {
        self.p.circle_filled(self.pt(x, y), r * self.s(), self.c);
    }
    fn ellipse(&self, cx: f32, cy: f32, rx: f32, ry: f32) {
        let n = 32;
        let pts: Vec<(f32, f32)> = (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                (cx + rx * a.cos(), cy + ry * a.sin())
            })
            .collect();
        self.closed(&pts);
    }
    fn text(&self, x: f32, y: f32, t: &str, size: f32) {
        self.p.text(self.pt(x, y), egui::Align2::CENTER_CENTER, t, crate::theme::semibold(size * self.s()), self.c);
    }
}

fn arrow(pen: &Pen, hollow: bool) {
    let pts = [(5.0, 2.5), (5.0, 16.0), (8.4, 12.8), (10.8, 18.0), (13.0, 17.0), (10.6, 11.9), (15.0, 11.6)];
    if hollow {
        pen.closed(&pts);
    } else {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| pen.pt(x, y)).collect();
        pen.p.add(Shape::Path(egui::epaint::PathShape { points: v, closed: true, fill: pen.c, stroke: egui::epaint::PathStroke::NONE }));
    }
}

/// Paint icon `name` into `r`.
pub fn paint(p: &Painter, r: Rect, name: &str, c: Color32) {
    let mut pen = Pen { p, r, c, w: 1.3 };
    match name {
        "tool-selection" => arrow(&pen, true),
        "tool-direct" => arrow(&pen, false),
        "tool-page" => {
            pen.closed(&[(5.0, 3.0), (12.0, 3.0), (15.0, 6.0), (15.0, 17.0), (5.0, 17.0)]);
            pen.line(&[(12.0, 3.0), (12.0, 6.0), (15.0, 6.0)]);
            pen.line(&[(2.0, 17.0), (2.0, 3.0)]);
        }
        "tool-gap" => {
            pen.rect(2.0, 4.0, 7.0, 16.0);
            pen.rect(13.0, 4.0, 18.0, 16.0);
            pen.line(&[(8.5, 10.0), (11.5, 10.0)]);
            pen.line(&[(9.5, 8.5), (8.0, 10.0), (9.5, 11.5)]);
            pen.line(&[(10.5, 8.5), (12.0, 10.0), (10.5, 11.5)]);
        }
        "tool-collector" | "tool-placer" => {
            pen.rect(3.0, 6.0, 17.0, 16.0);
            pen.line(&[(10.0, 2.0), (10.0, 11.0)]);
            pen.line(&[(7.0, 8.0), (10.0, 11.0), (13.0, 8.0)]);
        }
        "tool-type" => {
            pen.line(&[(4.0, 4.0), (16.0, 4.0)]);
            pen.line(&[(10.0, 4.0), (10.0, 17.0)]);
            pen.line(&[(7.5, 17.0), (12.5, 17.0)]);
            pen.line(&[(4.0, 4.0), (4.0, 6.0)]);
            pen.line(&[(16.0, 4.0), (16.0, 6.0)]);
        }
        "tool-type-path" | "tool-type-vertical" => {
            pen.line(&[(3.0, 15.0), (7.0, 11.0), (13.0, 14.0), (17.0, 9.0)]);
            pen.line(&[(6.0, 3.0), (14.0, 3.0)]);
            pen.line(&[(10.0, 3.0), (10.0, 10.0)]);
        }
        "tool-line" => pen.line(&[(4.0, 16.0), (16.0, 4.0)]),
        "tool-pen" | "tool-pen-add" | "tool-pen-delete" | "tool-anchor" => {
            pen.closed(&[(10.0, 2.5), (14.5, 11.0), (12.0, 15.0), (8.0, 15.0), (5.5, 11.0)]);
            pen.line(&[(10.0, 2.5), (10.0, 9.5)]);
            pen.fcircle(10.0, 10.5, 1.1);
            pen.line(&[(8.0, 15.0), (8.0, 18.0), (12.0, 18.0), (12.0, 15.0)]);
            if name == "tool-pen-add" {
                pen.line(&[(15.0, 3.0), (19.0, 3.0)]);
                pen.line(&[(17.0, 1.0), (17.0, 5.0)]);
            } else if name == "tool-pen-delete" {
                pen.line(&[(15.0, 3.0), (19.0, 3.0)]);
            }
        }
        "tool-pencil" | "tool-smooth" | "tool-erase" => {
            pen.closed(&[(4.0, 16.0), (5.0, 12.0), (14.0, 3.0), (17.0, 6.0), (8.0, 15.0)]);
            pen.line(&[(12.0, 5.0), (15.0, 8.0)]);
        }
        "tool-rect-frame" | "tool-ellipse-frame" | "tool-polygon-frame" => {
            if name == "tool-ellipse-frame" {
                pen.ellipse(10.0, 10.0, 7.0, 6.0);
            } else if name == "tool-polygon-frame" {
                pen.closed(&[(10.0, 3.0), (17.0, 8.0), (14.5, 16.5), (5.5, 16.5), (3.0, 8.0)]);
            } else {
                pen.rect(3.0, 4.0, 17.0, 16.0);
            }
            pen.w = 1.0;
            pen.line(&[(5.0, 6.0), (15.0, 14.0)]);
            pen.line(&[(15.0, 6.0), (5.0, 14.0)]);
        }
        "tool-rect" => pen.rect(3.0, 4.0, 17.0, 16.0),
        "tool-ellipse" => pen.ellipse(10.0, 10.0, 7.0, 6.0),
        "tool-polygon" => pen.closed(&[(10.0, 3.0), (17.0, 8.0), (14.5, 16.5), (5.5, 16.5), (3.0, 8.0)]),
        "tool-scissors" => {
            pen.circle(6.0, 14.5, 2.5);
            pen.circle(14.0, 14.5, 2.5);
            pen.line(&[(7.5, 12.5), (14.0, 3.0)]);
            pen.line(&[(12.5, 12.5), (6.0, 3.0)]);
        }
        "tool-free-transform" | "tool-scale" | "tool-shear" => {
            pen.rect(4.0, 4.0, 13.0, 13.0);
            pen.frect(2.8, 2.8, 5.2, 5.2);
            pen.frect(11.8, 11.8, 14.2, 14.2);
            pen.line(&[(14.0, 14.0), (18.0, 18.0)]);
            pen.line(&[(15.0, 18.0), (18.0, 18.0), (18.0, 15.0)]);
        }
        "tool-rotate" => {
            let pts: Vec<(f32, f32)> = (0..24)
                .map(|i| {
                    let a = (i as f32 / 23.0) * 4.6 - 0.6;
                    (10.0 + 6.5 * a.cos(), 10.0 - 6.5 * a.sin())
                })
                .collect();
            pen.line(&pts);
            pen.line(&[(13.0, 4.0), (15.5, 6.0), (16.5, 2.8)]);
        }
        "tool-gradient" | "tool-gradient-feather" => {
            for i in 0..6 {
                let a = 30 + i * 35;
                let pc = Pen { p, r, c: c.gamma_multiply(a as f32 / 255.0 * 1.2), w: 1.0 };
                pc.frect(3.0 + i as f32 * 2.33, 4.0, 3.0 + (i + 1) as f32 * 2.33, 16.0);
            }
            pen.rect(3.0, 4.0, 17.0, 16.0);
        }
        "tool-note" => {
            pen.closed(&[(4.0, 3.0), (16.0, 3.0), (16.0, 13.0), (11.0, 17.0), (4.0, 17.0)]);
            pen.line(&[(7.0, 7.0), (13.0, 7.0)]);
            pen.line(&[(7.0, 10.0), (13.0, 10.0)]);
        }
        "tool-color-theme" | "tool-eyedropper" => {
            pen.closed(&[(3.0, 17.0), (3.5, 14.5), (11.0, 7.0), (13.0, 9.0), (5.5, 16.5)]);
            pen.line(&[(11.0, 5.0), (15.0, 9.0)]);
            pen.closed(&[(12.5, 6.5), (15.0, 3.0), (17.0, 5.0), (13.5, 7.5)]);
        }
        "tool-measure" => {
            pen.closed(&[(2.0, 13.0), (13.0, 2.0), (18.0, 7.0), (7.0, 18.0)]);
            for k in 0..4 {
                let t = 4.0 + k as f32 * 2.5;
                pen.line(&[(t, 13.0 - (t - 2.0)), (t + 1.5, 13.0 - (t - 2.0) + 1.5)]);
            }
        }
        "tool-hand" => {
            pen.line(&[(6.0, 11.0), (6.0, 5.0)]);
            pen.line(&[(8.5, 10.0), (8.5, 3.5)]);
            pen.line(&[(11.0, 10.0), (11.0, 4.0)]);
            pen.line(&[(13.5, 10.5), (13.5, 6.0)]);
            pen.line(&[(6.0, 11.0), (4.0, 9.0), (3.0, 10.0), (6.5, 16.5), (13.5, 17.5), (15.0, 13.0), (13.5, 10.5)]);
        }
        "tool-zoom" => {
            pen.circle(8.5, 8.5, 5.5);
            pen.w = 2.0;
            pen.line(&[(12.5, 12.5), (17.5, 17.5)]);
        }
        // ---- panel icons
        "panel-properties" => {
            pen.line(&[(3.0, 5.0), (17.0, 5.0)]);
            pen.line(&[(3.0, 10.0), (17.0, 10.0)]);
            pen.line(&[(3.0, 15.0), (17.0, 15.0)]);
            pen.fcircle(7.0, 5.0, 1.6);
            pen.fcircle(13.0, 10.0, 1.6);
            pen.fcircle(9.0, 15.0, 1.6);
        }
        "panel-pages" => {
            pen.rect(3.0, 3.0, 9.0, 11.0);
            pen.rect(11.0, 3.0, 17.0, 11.0);
            pen.rect(7.0, 13.0, 13.0, 18.0);
        }
        "panel-layers" => {
            pen.closed(&[(10.0, 3.0), (18.0, 7.0), (10.0, 11.0), (2.0, 7.0)]);
            pen.line(&[(2.0, 10.5), (10.0, 14.5), (18.0, 10.5)]);
            pen.line(&[(2.0, 14.0), (10.0, 18.0), (18.0, 14.0)]);
        }
        "panel-links" => {
            pen.closed(&[
                (3.0, 10.0),
                (6.0, 7.0),
                (10.0, 7.0),
                (10.0, 9.0),
                (6.5, 9.0),
                (5.0, 10.0),
                (6.5, 11.0),
                (10.0, 11.0),
                (10.0, 13.0),
                (6.0, 13.0),
            ]);
            pen.closed(&[(17.0, 10.0), (14.0, 7.0), (10.0, 7.0)]);
            pen.line(&[(8.0, 10.0), (12.0, 10.0)]);
            pen.line(&[(17.0, 10.0), (14.0, 13.0), (10.0, 13.0)]);
        }
        "panel-stroke" => {
            pen.w = 0.8;
            pen.line(&[(3.0, 5.0), (17.0, 5.0)]);
            pen.w = 1.6;
            pen.line(&[(3.0, 9.5), (17.0, 9.5)]);
            pen.w = 2.8;
            pen.line(&[(3.0, 15.0), (17.0, 15.0)]);
        }
        "panel-swatches" => {
            pen.frect(3.0, 3.0, 9.0, 9.0);
            pen.rect(11.0, 3.0, 17.0, 9.0);
            pen.rect(3.0, 11.0, 9.0, 17.0);
            pen.frect(11.0, 11.0, 17.0, 17.0);
        }
        "panel-color" => {
            pen.circle(7.5, 8.0, 4.5);
            pen.circle(12.5, 8.0, 4.5);
            pen.circle(10.0, 12.5, 4.5);
        }
        "panel-gradient" => {
            for i in 0..7 {
                let pc = Pen { p, r, c: c.gamma_multiply(0.15 + i as f32 * 0.14), w: 1.0 };
                pc.frect(3.0 + i as f32 * 2.0, 5.0, 5.0 + i as f32 * 2.0, 15.0);
            }
        }
        "panel-effects" => pen.text(10.0, 10.0, "fx", 11.0),
        "panel-character" => pen.text(10.0, 10.5, "A", 15.0),
        "panel-paragraph" => pen.text(10.0, 10.5, "¶", 15.0),
        "panel-pstyles" => {
            pen.text(8.0, 10.0, "¶", 13.0);
            pen.line(&[(13.0, 6.0), (18.0, 6.0)]);
            pen.line(&[(13.0, 10.0), (18.0, 10.0)]);
            pen.line(&[(13.0, 14.0), (18.0, 14.0)]);
        }
        "panel-cstyles" => {
            pen.text(8.0, 10.0, "A", 13.0);
            pen.line(&[(13.0, 6.0), (18.0, 6.0)]);
            pen.line(&[(13.0, 10.0), (18.0, 10.0)]);
            pen.line(&[(13.0, 14.0), (18.0, 14.0)]);
        }
        "panel-wrap" => {
            pen.frect(7.0, 7.0, 13.0, 13.0);
            for y in [3.0, 17.0] {
                pen.line(&[(3.0, y), (17.0, y)]);
            }
            pen.line(&[(3.0, 10.0), (5.0, 10.0)]);
            pen.line(&[(15.0, 10.0), (17.0, 10.0)]);
        }
        "panel-table" => {
            pen.frect(3.0, 3.0, 17.0, 7.5);
            pen.rect(3.0, 3.0, 17.0, 17.0);
            pen.line(&[(3.0, 12.0), (17.0, 12.0)]);
            pen.line(&[(10.0, 3.0), (10.0, 17.0)]);
        }
        "panel-align" => {
            pen.line(&[(3.0, 3.0), (3.0, 17.0)]);
            pen.frect(5.0, 5.0, 15.0, 9.0);
            pen.frect(5.0, 11.0, 11.0, 15.0);
        }
        "panel-transform" => {
            pen.rect(4.0, 4.0, 16.0, 16.0);
            for (x, y) in [(4.0, 4.0), (16.0, 4.0), (4.0, 16.0), (16.0, 16.0), (10.0, 10.0)] {
                pen.fcircle(x, y, 1.4);
            }
        }
        "panel-info" => {
            pen.circle(10.0, 10.0, 7.0);
            pen.text(10.0, 10.5, "i", 11.0);
        }
        "panel-preflight" => {
            pen.circle(10.0, 10.0, 7.0);
            pen.line(&[(6.5, 10.0), (9.0, 12.5), (13.5, 7.5)]);
        }
        "panel-hyperlinks" => {
            // Two chain links.
            pen.closed(&[(3.0, 10.0), (7.0, 6.0), (10.0, 6.0), (7.0, 9.0), (6.0, 11.0), (7.0, 13.0), (5.0, 14.0)]);
            pen.closed(&[(17.0, 10.0), (13.0, 14.0), (10.0, 14.0), (13.0, 11.0), (14.0, 9.0), (13.0, 7.0), (15.0, 6.0)]);
            pen.line(&[(8.0, 12.0), (12.0, 8.0)]);
        }
        "panel-tags" => {
            // Angle brackets.
            pen.line(&[(7.0, 5.0), (3.0, 10.0), (7.0, 15.0)]);
            pen.line(&[(13.0, 5.0), (17.0, 10.0), (13.0, 15.0)]);
            pen.line(&[(11.0, 4.0), (9.0, 16.0)]);
        }
        "panel-articles" => {
            // Numbered blocks in order.
            pen.rect(3.0, 3.0, 11.0, 8.0);
            pen.rect(3.0, 12.0, 11.0, 17.0);
            pen.line(&[(14.0, 5.5), (17.0, 5.5)]);
            pen.line(&[(14.0, 14.5), (17.0, 14.5)]);
        }
        "panel-bookmarks" => {
            pen.closed(&[(6.0, 3.0), (14.0, 3.0), (14.0, 17.0), (10.0, 13.0), (6.0, 17.0)]);
        }
        "panel-library" => {
            // Books on a shelf.
            pen.rect(3.0, 4.0, 7.0, 16.0);
            pen.rect(8.0, 6.0, 12.0, 16.0);
            pen.line(&[(13.0, 6.5), (17.0, 5.0), (17.5, 15.5), (13.5, 16.5)]);
        }
        "panel-notes" => {
            // A note with a folded corner.
            pen.closed(&[(4.0, 3.0), (16.0, 3.0), (16.0, 13.0), (12.0, 17.0), (4.0, 17.0)]);
            pen.line(&[(16.0, 13.0), (12.0, 13.0), (12.0, 17.0)]);
        }
        "panel-conditions" => {
            // Text lines, one marked.
            pen.line(&[(3.0, 6.0), (17.0, 6.0)]);
            pen.line(&[(3.0, 10.0), (17.0, 10.0)]);
            pen.line(&[(3.0, 14.0), (11.0, 14.0)]);
            pen.line(&[(3.0, 16.5), (11.0, 16.5)]);
        }
        "panel-attributes" => {
            // Two overlapping inks: overprint.
            pen.rect(3.0, 3.0, 13.0, 13.0);
            pen.rect(7.0, 7.0, 17.0, 17.0);
        }
        "panel-text-frame" => {
            pen.rect(3.0, 4.0, 17.0, 16.0);
            pen.text(10.0, 10.0, "T", 9.0);
        }
        // ---- small UI glyphs
        "chevron-down" => pen.line(&[(6.0, 8.0), (10.0, 12.0), (14.0, 8.0)]),
        "chevron-right" => pen.line(&[(8.0, 6.0), (12.0, 10.0), (8.0, 14.0)]),
        "chevron-left" => pen.line(&[(12.0, 6.0), (8.0, 10.0), (12.0, 14.0)]),
        "double-chevron-left" => {
            pen.line(&[(10.0, 6.0), (6.0, 10.0), (10.0, 14.0)]);
            pen.line(&[(15.0, 6.0), (11.0, 10.0), (15.0, 14.0)]);
        }
        "double-chevron-right" => {
            pen.line(&[(5.0, 6.0), (9.0, 10.0), (5.0, 14.0)]);
            pen.line(&[(10.0, 6.0), (14.0, 10.0), (10.0, 14.0)]);
        }
        "first" => {
            pen.line(&[(5.0, 5.0), (5.0, 15.0)]);
            pen.fill(&[(15.0, 5.0), (15.0, 15.0), (7.0, 10.0)]);
        }
        "prev" => pen.fill(&[(13.0, 5.0), (13.0, 15.0), (6.0, 10.0)]),
        "next" => pen.fill(&[(7.0, 5.0), (7.0, 15.0), (14.0, 10.0)]),
        "last" => {
            pen.line(&[(15.0, 5.0), (15.0, 15.0)]);
            pen.fill(&[(5.0, 5.0), (5.0, 15.0), (13.0, 10.0)]);
        }
        "eye" => {
            pen.closed(&[(2.0, 10.0), (6.0, 6.0), (10.0, 5.0), (14.0, 6.0), (18.0, 10.0), (14.0, 14.0), (10.0, 15.0), (6.0, 14.0)]);
            pen.fcircle(10.0, 10.0, 2.5);
        }
        "lock" => {
            pen.frect(5.0, 9.0, 15.0, 17.0);
            pen.line(&[(7.0, 9.0), (7.0, 6.0), (8.5, 4.0), (11.5, 4.0), (13.0, 6.0), (13.0, 9.0)]);
        }
        "plus" => {
            pen.line(&[(10.0, 4.0), (10.0, 16.0)]);
            pen.line(&[(4.0, 10.0), (16.0, 10.0)]);
        }
        "trash" => {
            pen.line(&[(4.0, 5.0), (16.0, 5.0)]);
            pen.closed(&[(5.5, 5.0), (6.5, 17.0), (13.5, 17.0), (14.5, 5.0)]);
            pen.line(&[(8.0, 5.0), (8.5, 3.0), (11.5, 3.0), (12.0, 5.0)]);
        }
        "menu" => {
            for y in [6.0, 10.0, 14.0] {
                pen.line(&[(4.0, y), (16.0, y)]);
            }
        }
        "home" => {
            pen.closed(&[(3.0, 9.0), (10.0, 3.0), (17.0, 9.0), (17.0, 17.0), (3.0, 17.0)]);
            pen.rect(8.0, 11.0, 12.0, 17.0);
        }
        "search" => {
            pen.circle(8.5, 8.5, 5.0);
            pen.line(&[(12.5, 12.5), (17.0, 17.0)]);
        }
        "share" => {
            pen.line(&[(10.0, 3.0), (10.0, 13.0)]);
            pen.line(&[(6.5, 6.5), (10.0, 3.0), (13.5, 6.5)]);
            pen.line(&[(5.0, 10.0), (4.0, 10.0), (4.0, 17.0), (16.0, 17.0), (16.0, 10.0), (15.0, 10.0)]);
        }
        "screen-normal" => pen.rect(3.0, 4.0, 17.0, 16.0),
        "screen-preview" => {
            pen.rect(3.0, 4.0, 17.0, 16.0);
            pen.frect(6.0, 7.0, 14.0, 13.0);
        }
        "align-left" | "align-center" | "align-right" | "align-justify" | "align-justify-all" => {
            let rows = [(3.0, 14.0), (3.0, 17.0), (3.0, 12.0), (3.0, 16.0)];
            for (i, (a, b)) in rows.iter().enumerate() {
                let y = 4.5 + i as f32 * 3.7;
                let w = b - a;
                let (x0, x1) = match name {
                    "align-center" => (10.0 - w / 2.0, 10.0 + w / 2.0),
                    "align-right" => (17.0 - w, 17.0),
                    "align-justify" if i < 3 => (3.0, 17.0),
                    "align-justify-all" => (3.0, 17.0),
                    _ => (*a, *b),
                };
                pen.line(&[(x0, y), (x1, y)]);
            }
        }
        "ref-point" => {
            for i in 0..9 {
                let (x, y) = (4.0 + (i % 3) as f32 * 6.0, 4.0 + (i / 3) as f32 * 6.0);
                pen.rect(x - 1.2, y - 1.2, x + 1.2, y + 1.2);
            }
        }
        "fill-proxy" => pen.frect(3.0, 3.0, 13.0, 13.0),
        "swap" => {
            pen.line(&[(5.0, 15.0), (5.0, 8.0), (8.0, 5.0), (15.0, 5.0)]);
            pen.line(&[(12.5, 2.5), (15.0, 5.0), (12.5, 7.5)]);
            pen.line(&[(2.5, 12.5), (5.0, 15.0), (7.5, 12.5)]);
        }
        "none" => {
            pen.rect(3.0, 3.0, 17.0, 17.0);
            pen.p.line_segment([pen.pt(3.0, 17.0), pen.pt(17.0, 3.0)], Stroke::new(1.6 * pen.s(), Color32::from_rgb(230, 40, 40)));
        }
        "fit-fill" | "fit-content" | "fit-frame" | "fit-center" | "fit-prop" => {
            pen.rect(3.0, 5.0, 17.0, 15.0);
            match name {
                "fit-fill" => pen.frect(5.0, 3.0, 15.0, 17.0),
                "fit-prop" => pen.frect(7.0, 6.5, 13.0, 13.5),
                "fit-content" => pen.frect(4.5, 6.5, 15.5, 13.5),
                "fit-center" => pen.fcircle(10.0, 10.0, 2.0),
                _ => pen.rect(5.0, 7.0, 15.0, 13.0),
            }
        }
        "wrap-none" | "wrap-bbox" | "wrap-contour" | "wrap-jump" | "wrap-next" => {
            match name {
                "wrap-contour" => {
                    pen.ellipse(10.0, 10.0, 4.0, 4.0);
                }
                _ => pen.frect(7.5, 7.5, 12.5, 12.5),
            }
            for y in [3.0, 5.5, 14.5, 17.0] {
                pen.line(&[(3.0, y), (17.0, y)]);
            }
            match name {
                "wrap-none" => pen.line(&[(3.0, 10.0), (17.0, 10.0)]),
                "wrap-bbox" | "wrap-contour" => {
                    pen.line(&[(3.0, 10.0), (5.5, 10.0)]);
                    pen.line(&[(14.5, 10.0), (17.0, 10.0)]);
                }
                _ => {}
            }
        }
        // ---- Properties panel glyphs (original drawings)
        "orient-portrait" => {
            pen.rect(5.5, 3.0, 14.5, 17.0);
            pen.fcircle(10.0, 8.0, 1.8);
            pen.frect(7.5, 11.0, 12.5, 15.0);
        }
        "orient-landscape" => {
            pen.rect(3.0, 5.5, 17.0, 14.5);
            pen.fcircle(13.0, 10.0, 1.8);
            pen.frect(5.0, 8.0, 9.0, 12.0);
        }
        "page-count" => {
            pen.closed(&[(10.0, 5.0), (3.0, 3.0), (3.0, 15.0), (10.0, 17.0)]);
            pen.closed(&[(10.0, 5.0), (17.0, 3.0), (17.0, 15.0), (10.0, 17.0)]);
            pen.line(&[(10.0, 2.0), (10.0, 18.0)]);
        }
        "margin-top" => {
            pen.frect(2.0, 3.0, 18.0, 7.0);
            pen.rect(2.5, 9.0, 17.5, 17.0);
        }
        "margin-bottom" => {
            pen.rect(2.5, 3.0, 17.5, 11.0);
            pen.frect(2.0, 13.0, 18.0, 17.0);
        }
        "margin-inside" => {
            pen.frect(2.0, 3.0, 6.0, 17.0);
            pen.rect(8.0, 3.5, 17.5, 16.5);
        }
        "margin-outside" => {
            pen.rect(2.5, 3.5, 12.0, 16.5);
            pen.frect(14.0, 3.0, 18.0, 17.0);
        }
        "link" | "link-broken" => {
            let broken = name == "link-broken";
            let (a, b) = if broken { (7.5, 12.5) } else { (9.0, 11.0) };
            pen.closed(&[(8.0, 3.0), (12.0, 3.0), (12.0, a), (8.0, a)]);
            pen.closed(&[(8.0, b), (12.0, b), (12.0, 17.0), (8.0, 17.0)]);
            if broken {
                pen.line(&[(4.0, 4.0), (16.0, 16.0)]);
            } else {
                pen.line(&[(10.0, 6.0), (10.0, 14.0)]);
            }
        }
        "rulers" => {
            pen.closed(&[(3.0, 3.0), (17.0, 3.0), (17.0, 7.0), (7.0, 7.0), (7.0, 17.0), (3.0, 17.0)]);
            for k in 0..4 {
                let v = 9.0 + k as f32 * 2.2;
                pen.line(&[(v, 3.0), (v, 5.0)]);
                pen.line(&[(3.0, v), (5.0, v)]);
            }
        }
        "grid-baseline" => {
            pen.rect(3.0, 3.0, 17.0, 17.0);
            for y in [6.5, 10.0, 13.5] {
                pen.line(&[(3.0, y), (17.0, y)]);
            }
        }
        "grid-document" => {
            pen.rect(3.0, 3.0, 17.0, 17.0);
            for v in [7.7, 12.3] {
                pen.line(&[(3.0, v), (17.0, v)]);
                pen.line(&[(v, 3.0), (v, 17.0)]);
            }
        }
        "guides-show" | "guides-lock" | "guides-smart" => {
            pen.line(&[(6.0, 2.0), (6.0, 18.0)]);
            pen.line(&[(10.0, 2.0), (10.0, 18.0)]);
            pen.line(&[(2.0, 7.0), (14.0, 7.0)]);
            pen.line(&[(2.0, 12.0), (14.0, 12.0)]);
            match name {
                "guides-lock" => {
                    pen.frect(12.5, 13.5, 18.0, 18.0);
                    pen.line(&[(13.5, 13.5), (13.5, 11.5), (17.0, 11.5), (17.0, 13.5)]);
                }
                "guides-smart" => {
                    pen.fill(&[(16.5, 9.0), (13.0, 14.0), (16.5, 14.0)]);
                    pen.fill(&[(15.0, 14.0), (18.0, 14.0), (14.5, 19.0)]);
                }
                _ => {}
            }
        }
        "align-to" | "frame-dashed" => {
            pen.w = 1.0;
            for (a, b) in [((3.0, 3.0), (15.0, 3.0)), ((15.0, 3.0), (15.0, 15.0)), ((15.0, 15.0), (3.0, 15.0)), ((3.0, 15.0), (3.0, 3.0))] {
                let n = 4;
                for k in 0..n {
                    let f0 = k as f32 / n as f32;
                    let f1 = f0 + 0.6 / n as f32;
                    pen.line(&[(a.0 + (b.0 - a.0) * f0, a.1 + (b.1 - a.1) * f0), (a.0 + (b.0 - a.0) * f1, a.1 + (b.1 - a.1) * f1)]);
                }
            }
            pen.fill(&[(16.0, 17.0), (19.5, 17.0), (17.75, 19.0)]);
        }
        "objalign-left" | "objalign-hcenter" | "objalign-right" => {
            let x = match name {
                "objalign-left" => 4.0,
                "objalign-hcenter" => 10.0,
                _ => 16.0,
            };
            pen.line(&[(x, 2.0), (x, 18.0)]);
            let (a0, a1, b0, b1) = match name {
                "objalign-left" => (4.0, 15.0, 4.0, 11.0),
                "objalign-hcenter" => (4.5, 15.5, 6.5, 13.5),
                _ => (5.0, 16.0, 9.0, 16.0),
            };
            pen.frect(a0, 4.5, a1, 8.5);
            pen.frect(b0, 11.5, b1, 15.5);
        }
        "objalign-top" | "objalign-vcenter" | "objalign-bottom" => {
            let y = match name {
                "objalign-top" => 4.0,
                "objalign-vcenter" => 10.0,
                _ => 16.0,
            };
            pen.line(&[(2.0, y), (18.0, y)]);
            let (a0, a1, b0, b1) = match name {
                "objalign-top" => (4.0, 15.0, 4.0, 11.0),
                "objalign-vcenter" => (4.5, 15.5, 6.5, 13.5),
                _ => (5.0, 16.0, 9.0, 16.0),
            };
            pen.frect(4.5, a0, 8.5, a1);
            pen.frect(11.5, b0, 15.5, b1);
        }
        "palign-left"
        | "palign-center"
        | "palign-right"
        | "palign-justify-left"
        | "palign-justify-center"
        | "palign-justify-right"
        | "palign-justify-all"
        | "palign-spine-towards"
        | "palign-spine-away" => {
            pen.w = 1.1;
            let rows = 5;
            for i in 0..rows {
                let y = 4.0 + i as f32 * 3.0;
                let last = i == rows - 1;
                let short = [14.0, 11.0, 13.0, 10.0, 12.0][i];
                let (x0, x1) = match name {
                    "palign-left" => (3.0, 3.0 + short),
                    "palign-center" => (10.0 - short / 2.0, 10.0 + short / 2.0),
                    "palign-right" => (17.0 - short, 17.0),
                    "palign-justify-left" if last => (3.0, 11.0),
                    "palign-justify-center" if last => (6.0, 14.0),
                    "palign-justify-right" if last => (9.0, 17.0),
                    "palign-spine-towards" => (if i % 2 == 0 { 3.0 } else { 6.0 }, 14.0),
                    "palign-spine-away" => (3.0, if i % 2 == 0 { 14.0 } else { 11.0 }),
                    _ => (3.0, 17.0),
                };
                pen.line(&[(x0, y), (x1, y)]);
            }
            if name == "palign-spine-towards" || name == "palign-spine-away" {
                pen.line(&[(17.0, 2.0), (17.0, 18.0)]);
            }
            if name == "palign-spine-away" {
                pen.line(&[(15.5, 2.0), (15.5, 18.0)]);
            }
        }
        "text-columns" => {
            pen.rect(2.5, 3.5, 17.5, 16.5);
            pen.line(&[(7.5, 3.5), (7.5, 16.5)]);
            pen.line(&[(12.5, 3.5), (12.5, 16.5)]);
        }
        "text-gutter" => {
            pen.rect(2.5, 3.5, 17.5, 16.5);
            pen.line(&[(8.0, 3.5), (8.0, 16.5)]);
            pen.line(&[(12.0, 3.5), (12.0, 16.5)]);
            pen.line(&[(9.0, 10.0), (11.0, 10.0)]);
        }
        "font-size" => {
            pen.text(6.0, 12.5, "T", 10.0);
            pen.text(13.0, 10.5, "T", 17.0);
        }
        "leading" => {
            pen.text(13.0, 6.0, "A", 9.0);
            pen.text(13.0, 14.5, "A", 9.0);
            pen.line(&[(4.0, 3.0), (4.0, 17.0)]);
            pen.line(&[(2.5, 5.0), (4.0, 3.0), (5.5, 5.0)]);
            pen.line(&[(2.5, 15.0), (4.0, 17.0), (5.5, 15.0)]);
        }
        "kerning" | "tracking" => {
            pen.text(10.0, 8.0, "VA", 10.0);
            if name == "kerning" {
                pen.line(&[(10.0, 13.0), (10.0, 18.0)]);
                pen.line(&[(4.0, 15.5), (16.0, 15.5)]);
            } else {
                pen.line(&[(3.0, 15.5), (17.0, 15.5)]);
                pen.line(&[(5.0, 13.5), (3.0, 15.5), (5.0, 17.5)]);
                pen.line(&[(15.0, 13.5), (17.0, 15.5), (15.0, 17.5)]);
            }
        }
        "corner" => {
            pen.w = 1.0;
            for (x, y) in [(3.0, 3.0), (14.0, 3.0), (3.0, 14.0), (14.0, 14.0)] {
                pen.frect(x, y, x + 3.0, y + 3.0);
            }
            pen.line(&[(7.0, 4.5), (13.0, 4.5)]);
            pen.line(&[(7.0, 15.5), (13.0, 15.5)]);
            pen.line(&[(4.5, 7.0), (4.5, 13.0)]);
            pen.line(&[(15.5, 7.0), (15.5, 13.0)]);
        }
        "opacity" => {
            for i in 0..4 {
                for j in 0..4 {
                    if (i + j) % 2 == 0 {
                        pen.frect(3.0 + i as f32 * 3.5, 3.0 + j as f32 * 3.5, 6.5 + i as f32 * 3.5, 6.5 + j as f32 * 3.5);
                    }
                }
            }
            pen.w = 0.8;
            pen.rect(3.0, 3.0, 17.0, 17.0);
        }
        "fx" => {
            pen.text(9.0, 10.0, "fx", 11.0);
            pen.fill(&[(16.0, 15.0), (19.0, 15.0), (17.5, 17.0)]);
        }
        "bullets" => {
            for (k, y) in [5.0, 10.0, 15.0].iter().enumerate() {
                pen.fcircle(4.0, *y, 1.5);
                pen.line(&[(7.5, *y), (if k == 1 { 14.0 } else { 17.0 }, *y)]);
            }
        }
        "numbering" => {
            for (k, y) in [5.0, 10.0, 15.0].iter().enumerate() {
                pen.text(4.0, *y, ["1", "2", "3"][k], 6.0);
                pen.line(&[(7.5, *y), (if k == 1 { 14.0 } else { 17.0 }, *y)]);
            }
        }
        "pilcrow-menu" => {
            pen.text(9.0, 10.0, "¶", 14.0);
            pen.fill(&[(14.0, 15.0), (17.0, 15.0), (15.5, 17.0)]);
        }
        "style-new" => {
            pen.rect(4.0, 4.0, 16.0, 16.0);
            pen.line(&[(10.0, 7.0), (10.0, 13.0)]);
            pen.line(&[(7.0, 10.0), (13.0, 10.0)]);
        }
        "style-clear" => {
            pen.text(8.0, 10.0, "¶", 13.0);
            pen.line(&[(13.0, 5.0), (17.0, 9.0)]);
            pen.line(&[(17.0, 5.0), (13.0, 9.0)]);
        }
        "style-load" => {
            pen.line(&[(2.0, 8.0), (9.0, 8.0)]);
            pen.line(&[(6.5, 5.5), (9.0, 8.0), (6.5, 10.5)]);
            pen.closed(&[(11.0, 3.0), (17.0, 3.0), (17.0, 17.0), (8.0, 17.0), (8.0, 12.0)]);
        }
        "grip" => {
            for i in 0..3 {
                for j in 0..2 {
                    pen.fcircle(8.0 + j as f32 * 4.0, 5.0 + i as f32 * 5.0, 1.1);
                }
            }
        }
        "fit-text" => {
            pen.line(&[(10.0, 2.0), (10.0, 18.0)]);
            pen.line(&[(7.0, 5.0), (10.0, 2.0), (13.0, 5.0)]);
            pen.line(&[(7.0, 15.0), (10.0, 18.0), (13.0, 15.0)]);
            pen.line(&[(3.0, 8.0), (7.0, 8.0)]);
            pen.line(&[(3.0, 12.0), (7.0, 12.0)]);
            pen.line(&[(13.0, 8.0), (17.0, 8.0)]);
            pen.line(&[(13.0, 12.0), (17.0, 12.0)]);
        }
        "frame-options" | "content-collector-frame" => {
            pen.w = 1.1;
            pen.rect(3.0, 3.0, 17.0, 17.0);
            pen.line(&[(6.0, 7.0), (14.0, 7.0)]);
            pen.line(&[(6.0, 10.0), (14.0, 10.0)]);
            pen.line(&[(6.0, 13.0), (11.0, 13.0)]);
        }
        "default-colors" => {
            pen.w = 1.2;
            pen.p.rect_filled(Rect::from_min_max(pen.pt(8.0, 8.0), pen.pt(19.0, 19.0)), 0.0, Color32::BLACK);
            pen.p.rect_filled(Rect::from_min_max(pen.pt(11.0, 11.0), pen.pt(16.0, 16.0)), 0.0, Color32::from_gray(0x53));
            pen.rect(8.0, 8.0, 19.0, 19.0);
            pen.p.rect_filled(Rect::from_min_max(pen.pt(1.0, 1.0), pen.pt(12.0, 12.0)), 0.0, Color32::WHITE);
            pen.p.line_segment([pen.pt(1.0, 12.0), pen.pt(12.0, 1.0)], Stroke::new(1.2 * pen.s(), Color32::from_rgb(230, 30, 30)));
            pen.rect(1.0, 1.0, 12.0, 12.0);
        }
        "format-container" => pen.rect(4.0, 4.0, 16.0, 16.0),
        "format-text" => pen.text(10.0, 10.5, "T", 15.0),
        "screen-mode" => {
            pen.rect(3.0, 4.0, 17.0, 16.0);
            pen.line(&[(3.0, 7.0), (17.0, 7.0)]);
        }
        _ => {
            pen.rect(4.0, 4.0, 16.0, 16.0);
        }
    }
}

/// An icon-sized clickable button. Returns the response.
pub fn button(ui: &mut egui::Ui, name: &str, size: f32, selected: bool, tip: &str) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), egui::Sense::click());
    if selected {
        ui.painter().rect_filled(r, 3.0, t.tool_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, t.hover);
    }
    let pad = size * 0.16;
    paint(ui.painter(), r.shrink(pad), name, if selected { t.text_strong } else { t.icon });
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_ui(|ui| {
            crate::rtl::label(ui, tip);
        })
    }
}
