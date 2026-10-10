//! Small InDesign-style widgets (measured from InDesign 2026, `plan/indesign/11-observed-ui.md` §4):
//! 21 pt fields (`#454545` fill, 1 pt `#747474` border) with an optional 18 pt spinner segment on
//! the **left** of the value and an optional preset chevron segment on the right, 20 pt outline
//! buttons, underlined link labels, `•••` more-options, 21 pt icon toggles in a `#303030` well,
//! section headers and 1.5 pt section rules.

use designcraft_geom::Unit;
use designcraft_geom::units::{format_measure_prec, parse_measure};
use egui::{Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::theme::Tokens;

/// Field height including the 1 pt border.
pub const FIELD_H: f32 = 21.0;
/// Width of the spinner (⌃⌄) and preset (⌄) segments.
pub const SEG_W: f32 = 18.0;
/// Outline button height.
pub const BUTTON_H: f32 = 20.0;
/// UI label size (InDesign: about 10.5–11 pt).
pub const LABEL_SIZE: f32 = 11.0;

fn label_font() -> egui::FontId {
    egui::FontId::proportional(LABEL_SIZE)
}

/// What a numeric field holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumKind<'a> {
    /// A length in points shown in `Unit`; accepts any unit and arithmetic.
    Measure(Unit),
    /// A plain number with a display suffix (`%`, `°`, ` pt`).
    Number { suffix: &'a str, decimals: usize },
}

impl NumKind<'_> {
    pub fn format(&self, v: f64) -> String {
        match *self {
            NumKind::Measure(u) => format_measure_prec(v, u, 3),
            NumKind::Number { suffix, decimals } => {
                let s = format!("{v:.decimals$}");
                let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
                let s = if s == "-0" { "0".to_string() } else { s };
                format!("{s}{suffix}")
            }
        }
    }
    pub fn parse(&self, s: &str) -> Option<f64> {
        match *self {
            NumKind::Measure(u) => parse_measure(s, u).ok(),
            NumKind::Number { .. } => {
                // Numbers accept the same arithmetic as measures (`12+3`, `50%*2`): evaluate as points.
                let cleaned: String =
                    s.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | '*' | '/' | '(' | ')' | ' ')).collect();
                parse_measure(cleaned.trim(), Unit::Points).ok().or_else(|| cleaned.trim().parse().ok())
            }
        }
    }
    /// One spinner click (points for measures).
    pub fn step(&self) -> f64 {
        match *self {
            NumKind::Measure(u) => match u {
                Unit::Points | Unit::Picas | Unit::Ciceros | Unit::Pixels | Unit::Agates => 1.0,
                Unit::Inches | Unit::InchesDecimal => 72.0 / 16.0,
                Unit::Millimeters | Unit::Centimeters | Unit::Q | Unit::Ha => Unit::Millimeters.to_pt(1.0),
            },
            NumKind::Number { .. } => 1.0,
        }
    }
}

/// A numeric field. Build with [`NumField::measure`] / [`NumField::number`], then [`NumField::show`].
pub struct NumField<'a> {
    id: &'a str,
    value: Option<f64>,
    kind: NumKind<'a>,
    width: f32,
    spinner: bool,
    presets: &'a [f64],
    step: Option<f64>,
    range: (f64, f64),
    enabled: bool,
    parens: bool,
}

impl<'a> NumField<'a> {
    pub fn measure(id: &'a str, value: Option<f64>, unit: Unit) -> Self {
        Self::new(id, value, NumKind::Measure(unit))
    }
    pub fn number(id: &'a str, value: Option<f64>, suffix: &'a str, decimals: usize) -> Self {
        Self::new(id, value, NumKind::Number { suffix, decimals })
    }
    fn new(id: &'a str, value: Option<f64>, kind: NumKind<'a>) -> Self {
        NumField { id, value, kind, width: 72.0, spinner: false, presets: &[], step: None, range: (f64::MIN, f64::MAX), enabled: true, parens: false }
    }
    /// Total width (spinner and preset segments included).
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
    /// Show the ⌃⌄ spinner segment on the left.
    pub fn spinner(mut self) -> Self {
        self.spinner = true;
        self
    }
    /// Show a ⌄ preset segment on the right listing `presets`.
    pub fn presets(mut self, p: &'a [f64]) -> Self {
        self.presets = p;
        self
    }
    pub fn step(mut self, s: f64) -> Self {
        self.step = Some(s);
        self
    }
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.range = (min, max);
        self
    }
    /// Show the value in parentheses (an automatic value, like auto leading "(14.4 pt)").
    pub fn parens(mut self, on: bool) -> Self {
        self.parens = on;
        self
    }
    fn fmt(&self, v: f64) -> String {
        if self.parens { format!("({})", self.kind.format(v)) } else { self.kind.format(v) }
    }
    fn parse(&self, s: &str) -> Option<f64> {
        self.kind.parse(s.trim().trim_start_matches('(').trim_end_matches(')'))
    }
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// Draw the field; returns the new value when the user commits an edit (Enter, focus loss,
    /// a spinner click, ↑/↓ while focused (Shift = ×10) or a preset).
    pub fn show(self, ui: &mut Ui) -> Option<f64> {
        let t = Tokens::get(ui.ctx());
        let key = ui.id().with(self.id);
        let (rect, _) = ui.allocate_exact_size(vec2(self.width, FIELD_H), Sense::hover());
        let step = self.step.unwrap_or_else(|| self.kind.step());
        let clamp = |v: f64| v.clamp(self.range.0, self.range.1);
        let mut out: Option<f64> = None;
        let mut text_rect = rect;
        let focused_id = key.with("te");
        let focused = ui.memory(|m| m.has_focus(focused_id));
        let (bg, fg, border) =
            if focused { (Color32::WHITE, Color32::from_gray(20), Color32::from_rgb(0x45, 0xa0, 0xf5)) } else { (t.input, t.text, t.field_border) };
        let fg = if self.enabled { fg } else { t.text_disabled };
        // Spinner segment.
        if self.spinner {
            let seg = Rect::from_min_size(rect.min, vec2(SEG_W, FIELD_H));
            text_rect.min.x = seg.max.x;
            ui.painter().rect_filled(seg, 0.0, t.input);
            let up = Rect::from_min_max(seg.min, pos2(seg.max.x, seg.center().y));
            let down = Rect::from_min_max(pos2(seg.min.x, seg.center().y), seg.max);
            for (r, dir) in [(up, 1.0), (down, -1.0)] {
                let resp = ui.interact(r, key.with(("spin", dir as i32)), if self.enabled { Sense::click() } else { Sense::hover() });
                if resp.hovered() && self.enabled {
                    ui.painter().rect_filled(r.shrink(1.0), 0.0, t.hover);
                }
                let c = r.center();
                let (a, b) = if dir > 0.0 { (2.5, -1.5) } else { (-2.5, 1.5) };
                ui.painter().line_segment(
                    [pos2(c.x - 4.0, c.y + a * 0.6), pos2(c.x, c.y + b)],
                    Stroke::new(1.0, if self.enabled { t.icon } else { t.text_disabled }),
                );
                ui.painter().line_segment(
                    [pos2(c.x, c.y + b), pos2(c.x + 4.0, c.y + a * 0.6)],
                    Stroke::new(1.0, if self.enabled { t.icon } else { t.text_disabled }),
                );
                if resp.clicked() {
                    let mul = if ui.input(|i| i.modifiers.shift) { 10.0 } else { 1.0 };
                    out = Some(clamp(self.value.unwrap_or(0.0) + dir * step * mul));
                }
            }
        }
        // Preset segment.
        let mut preset_resp = None;
        if !self.presets.is_empty() {
            let seg = Rect::from_min_max(pos2(rect.max.x - SEG_W, rect.min.y), rect.max);
            text_rect.max.x = seg.min.x;
            let resp = ui.interact(seg, key.with("presets"), Sense::click());
            ui.painter().rect_filled(seg, 0.0, if resp.hovered() { t.hover } else { t.input });
            chevron_down(ui.painter(), seg.center(), t.icon);
            preset_resp = Some(resp);
        }
        // Value.
        ui.painter().rect_filled(text_rect, 0.0, bg);
        let mut buf: String = ui.data(|d| d.get_temp::<String>(key)).unwrap_or_else(|| self.value.map(|v| self.fmt(v)).unwrap_or_default());
        let te = egui::TextEdit::singleline(&mut buf)
            .id(focused_id)
            .frame(egui::Frame::NONE)
            .font(label_font())
            .text_color(fg)
            .margin(egui::Margin::ZERO)
            .vertical_align(egui::Align::Center);
        let r = ui
            .add_enabled_ui(self.enabled, |ui| ui.put(Rect::from_min_max(text_rect.min + vec2(5.0, 1.0), text_rect.max - vec2(1.0, 1.0)), te))
            .inner;
        if r.has_focus() {
            // ↑/↓ step the value while typing.
            let (up, down, shift) = ui.input_mut(|i| {
                let s = i.modifiers.shift;
                let up = i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowUp) || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
                let down = i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowDown) || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
                (up, down, s)
            });
            if up || down {
                let base = self.parse(&buf).or(self.value).unwrap_or(0.0);
                let v = clamp(base + if up { step } else { -step } * if shift { 10.0 } else { 1.0 });
                buf = self.fmt(v);
                out = Some(v);
            }
            ui.data_mut(|d| d.insert_temp(key, buf.clone()));
        }
        if r.lost_focus() {
            if let Some(v) = self.parse(&buf).map(clamp)
                && self.value.is_none_or(|old| (old - v).abs() > 1e-9)
            {
                out = Some(v);
            }
            ui.data_mut(|d| d.remove::<String>(key));
        } else if !r.has_focus() {
            ui.data_mut(|d| d.remove::<String>(key));
        }
        if out.is_some() && !r.has_focus() {
            ui.data_mut(|d| d.remove::<String>(key));
        }
        // Frame: outer border plus segment rules.
        let p = ui.painter();
        p.rect_stroke(rect, 0.0, Stroke::new(1.0, border), StrokeKind::Inside);
        if self.spinner {
            p.line_segment([pos2(rect.min.x + SEG_W, rect.min.y), pos2(rect.min.x + SEG_W, rect.max.y)], Stroke::new(1.0, t.field_border));
        }
        if !self.presets.is_empty() {
            p.line_segment([pos2(rect.max.x - SEG_W, rect.min.y), pos2(rect.max.x - SEG_W, rect.max.y)], Stroke::new(1.0, t.field_border));
        }
        if let Some(resp) = preset_resp {
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(self.width);
                for v in self.presets {
                    if ui.selectable_label(self.value.is_some_and(|c| (c - v).abs() < 1e-9), self.kind.format(*v)).clicked() {
                        out = Some(*v);
                        ui.close();
                    }
                }
            });
        }
        out
    }
}

/// A plain measurement field (no spinner), e.g. Transform X/Y/W/H.
pub fn measure(ui: &mut Ui, id: &str, value: Option<f64>, unit: Unit, width: f32) -> Option<f64> {
    NumField::measure(id, value, unit).width(width).show(ui)
}

/// A measurement field with the spinner segment.
pub fn spin_measure(ui: &mut Ui, id: &str, value: Option<f64>, unit: Unit, width: f32) -> Option<f64> {
    NumField::measure(id, value, unit).width(width).spinner().show(ui)
}

/// A plain number field (percent, degrees, counts).
pub fn number(ui: &mut Ui, id: &str, value: Option<f64>, suffix: &str, width: f32, decimals: usize) -> Option<f64> {
    NumField::number(id, value, suffix, decimals).width(width).show(ui)
}

/// A number field with the spinner segment.
pub fn spin_number(ui: &mut Ui, id: &str, value: Option<f64>, suffix: &str, width: f32, decimals: usize) -> Option<f64> {
    NumField::number(id, value, suffix, decimals).width(width).spinner().show(ui)
}

pub fn chevron_down(p: &egui::Painter, c: egui::Pos2, col: Color32) {
    p.line_segment([pos2(c.x - 4.0, c.y - 2.0), pos2(c.x, c.y + 2.0)], Stroke::new(1.0, col));
    p.line_segment([pos2(c.x, c.y + 2.0), pos2(c.x + 4.0, c.y - 2.0)], Stroke::new(1.0, col));
}

/// A dropdown box (field style, ⌄ segment on the right). Returns the response; open a popup on it
/// with [`egui::Popup::menu`].
pub fn dropdown(ui: &mut Ui, text: &str, width: f32) -> Response {
    dropdown_h(ui, text, width, FIELD_H)
}

pub fn dropdown_h(ui: &mut Ui, text: &str, width: f32, h: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 0.0, if resp.hovered() { t.hover } else { t.input });
    p.rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    let seg = Rect::from_min_max(pos2(r.max.x - SEG_W, r.min.y), r.max);
    p.line_segment([seg.left_top(), seg.left_bottom()], Stroke::new(1.0, t.field_border));
    chevron_down(p, seg.center(), t.icon);
    let clip = Rect::from_min_max(r.min, pos2(seg.min.x - 2.0, r.max.y));
    p.with_clip_rect(clip).text(pos2(r.min.x + 7.0, r.center().y), egui::Align2::LEFT_CENTER, text, label_font(), t.text);
    resp
}

/// A dropdown that lists `items` and returns the picked index.
pub fn dropdown_list(ui: &mut Ui, text: &str, width: f32, items: &[String], current: Option<usize>) -> Option<usize> {
    let resp = dropdown(ui, text, width);
    let mut out = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(width);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (i, it) in items.iter().enumerate() {
                if ui.selectable_label(current == Some(i), it).clicked() {
                    out = Some(i);
                    ui.close();
                }
            }
        });
    });
    out
}

/// A 20 pt outline button (1 pt `#747474`, transparent fill, centred label).
pub fn outline_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, BUTTON_H), Sense::click());
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.rect_filled(r, 2.0, t.well);
    } else if resp.hovered() {
        p.rect_filled(r, 2.0, t.hover);
    }
    p.rect_stroke(r, 2.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    crate::rtl::paint(p, r.center(), egui::Align2::CENTER_CENTER, label, label_font(), t.text);
    resp
}

/// Two half-width buttons side by side (10 pt gutter); returns which was clicked.
pub fn button_pair(ui: &mut Ui, a: &str, b: &str) -> (bool, bool) {
    let w = (full_width(ui) - 10.0) / 2.0;
    let mut out = (false, false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        out.0 = outline_button(ui, a, w).clicked();
        out.1 = outline_button(ui, b, w).clicked();
    });
    out
}

/// Width of a full-width row in the Properties panel (214 pt in a 253 pt panel).
pub fn full_width(ui: &Ui) -> f32 {
    (ui.available_width() - 8.0).max(60.0)
}

/// An underlined link label (`Stroke`, `Corner`, `Opacity`) that opens the full panel.
pub fn link_label(ui: &mut Ui, text: &str, width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, FIELD_H), Sense::click());
    let g = crate::rtl::plain(ui.ctx(), text, label_font(), t.text);
    let pos = pos2(r.min.x, r.center().y - g.size().y / 2.0);
    let w = g.size().x;
    ui.painter().galley(pos, g, t.text);
    let y = r.center().y + 6.5;
    ui.painter().line_segment([pos2(r.min.x, y), pos2(r.min.x + w, y)], Stroke::new(1.0, if resp.hovered() { t.text_strong } else { t.text_dim }));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `•••` more-options, right-aligned on its own row at the bottom of a section.
pub fn more_options(ui: &mut Ui, language: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
    let r = Rect::from_min_max(pos2(row.max.x - 26.0, row.min.y), pos2(row.max.x - 4.0, row.max.y));
    let resp = ui.interact(r, ui.id().with(("more", row.min.y as i32)), Sense::click());
    for i in 0..3 {
        ui.painter().circle_filled(pos2(r.min.x + 4.0 + i as f32 * 7.0, r.center().y), 1.9, if resp.hovered() { t.text_strong } else { t.icon });
    }
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(language, "More Options"));
    })
}

/// A small dim label (field captions like `X:` or `W:`).
pub fn caption(ui: &mut Ui, s: &str) {
    let t = Tokens::get(ui.ctx());
    crate::rtl::label(ui, egui::RichText::new(s).size(LABEL_SIZE).color(t.text));
}

/// A right-aligned caption of a fixed width (`W:` before a spinner).
pub fn caption_w(ui: &mut Ui, s: &str, w: f32) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(w, FIELD_H), Sense::hover());
    crate::rtl::paint(ui.painter(), pos2(r.max.x - 2.0, r.center().y), egui::Align2::RIGHT_CENTER, s, egui::FontId::proportional(11.5), t.text);
}

/// The selection-type line at the top of the Properties panel ("No Selection", "Rectangle", …):
/// a 33 pt band followed by a section rule.
pub fn selection_band(ui: &mut Ui, title: &str) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 33.0), Sense::hover());
    crate::rtl::paint(ui.painter(), pos2(r.min.x, r.center().y + 1.0), egui::Align2::LEFT_CENTER, title, crate::theme::semibold(11.5), t.text_strong);
    rule(ui);
}

/// Panel section header (InDesign 2026: regular-weight white title, no disclosure chevron; a
/// 1.5 pt rule separates sections). Returns whether to show the section body (always true).
pub fn section(ui: &mut Ui, title: &str, _default_open: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.add_space(7.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
    crate::rtl::paint(ui.painter(), r.left_center(), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(11.5), t.text_strong);
    ui.add_space(3.0);
    true
}

/// A section header whose controls sit on the same row (Rulers & Grids, Guides, Flex Layout):
/// returns the remaining rect to the right of the title.
pub fn section_row(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.set_min_height(23.0);
        let (r, _) = ui.allocate_exact_size(vec2(100.0, 23.0), Sense::hover());
        crate::rtl::paint(ui.painter(), r.left_center(), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(11.5), t.text_strong);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            add(ui)
        });
    });
    ui.add_space(6.0);
}

/// The 1.5 pt section rule, full panel width.
pub fn rule(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.5), Sense::hover());
    let clip = ui.clip_rect();
    ui.painter().with_clip_rect(clip.expand2(vec2(12.0, 0.0))).rect_filled(
        Rect::from_min_max(pos2(r.min.x - 12.0, r.min.y), pos2(r.max.x + 12.0, r.max.y)),
        0.0,
        t.section_divider,
    );
}

pub fn divider(ui: &mut Ui) {
    ui.add_space(8.0);
    rule(ui);
}

/// A swatch chip (colour square, [None] slash, gradient ramp).
pub fn swatch_chip(ui: &mut Ui, size: f32, color: Option<Color32>, gradient: Option<(Color32, Color32)>) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    paint_chip(ui.painter(), r, color, gradient);
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    resp
}

pub fn paint_chip(p: &egui::Painter, r: egui::Rect, color: Option<Color32>, gradient: Option<(Color32, Color32)>) {
    match (color, gradient) {
        (_, Some((a, b))) => {
            let n = 12;
            for i in 0..n {
                let f = i as f32 / (n - 1) as f32;
                let c = Color32::from_rgb(
                    (a.r() as f32 * (1.0 - f) + b.r() as f32 * f) as u8,
                    (a.g() as f32 * (1.0 - f) + b.g() as f32 * f) as u8,
                    (a.b() as f32 * (1.0 - f) + b.b() as f32 * f) as u8,
                );
                let x0 = r.min.x + r.width() * i as f32 / n as f32;
                p.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, r.min.y), egui::pos2(x0 + r.width() / n as f32 + 0.5, r.max.y)), 0.0, c);
            }
        }
        (Some(c), None) => {
            p.rect_filled(r, 0.0, c);
        }
        (None, None) => {
            p.rect_filled(r, 0.0, Color32::WHITE);
            p.line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.5, Color32::from_rgb(230, 30, 30)));
        }
    }
}

/// A stroke chip: a ring of the colour around a dark centre (None = white with a red slash).
pub fn paint_stroke_chip(p: &egui::Painter, r: egui::Rect, color: Option<Color32>, inner: Color32) {
    match color {
        Some(c) => {
            p.rect_filled(r, 0.0, c);
            p.rect_filled(r.shrink(r.width() * 0.3), 0.0, inner);
        }
        None => {
            paint_chip(p, r, None, None);
            p.rect_stroke(r.shrink(r.width() * 0.3), 0.0, Stroke::new(1.0, Color32::from_gray(40)), StrokeKind::Middle);
        }
    }
}

/// Resolve a swatch to a display colour (and gradient end colours).
pub fn swatch_colors(doc: &designcraft_doc::Document, name: &str, tint: f32) -> (Option<Color32>, Option<(Color32, Color32)>) {
    let conv = |c: designcraft_color::Color| {
        let [r, g, b, _] = c.to_rgba8(1.0);
        Color32::from_rgb(r, g, b)
    };
    if let Some(g) = designcraft_color::swatch::resolve_gradient(&doc.swatches, name)
        && let (Some(a), Some(b)) = (g.stops.first(), g.stops.last())
    {
        return (None, Some((conv(a.color), conv(b.color))));
    }
    (doc.resolve_color(name, tint).map(conv), None)
}

/// A 21 pt icon toggle (active = `#303030` well with a 1 pt rim).
pub fn icon_toggle(ui: &mut Ui, icon: &str, on: bool, tip: &str) -> Response {
    icon_toggle_sized(ui, icon, on, tip, vec2(23.0, 21.0))
}

pub fn icon_toggle_sized(ui: &mut Ui, icon: &str, on: bool, tip: &str, size: egui::Vec2) -> Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    if on {
        ui.painter().rect(r, 2.0, t.well, Stroke::new(1.0, t.well_rim), StrokeKind::Inside);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 2.0, t.hover);
    }
    let s = (size.y - 4.0).min(17.0);
    crate::icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(s, s)), icon, if on { t.text_strong } else { t.icon });
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_ui(|ui| {
            crate::rtl::label(ui, tip);
        })
    }
}

/// An InDesign checkbox: 12 pt rounded box, white check, label.
pub fn checkbox(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let g = ui.painter().layout_no_wrap(label.to_string(), label_font(), t.text);
    let (r, mut resp) = ui.allocate_exact_size(vec2(18.0 + g.size().x, FIELD_H), Sense::click());
    let b = Rect::from_center_size(pos2(r.min.x + 6.5, r.center().y), vec2(12.0, 12.0));
    if *on {
        ui.painter().rect_filled(b, 2.0, t.icon);
        ui.painter().line_segment([b.min + vec2(2.5, 6.0), b.min + vec2(5.0, 8.5)], Stroke::new(1.5, t.panel));
        ui.painter().line_segment([b.min + vec2(5.0, 8.5), b.min + vec2(9.5, 3.5)], Stroke::new(1.5, t.panel));
    } else {
        ui.painter().rect_stroke(b, 2.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    }
    ui.painter().galley(pos2(r.min.x + 18.0, r.center().y - g.size().y / 2.0), g, t.text);
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp
}

/// A two-segment control ([Paragraph Styles | Character Styles]); returns the clicked index.
pub fn segmented(ui: &mut Ui, labels: &[&str], active: usize, width: f32) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(width, BUTTON_H), Sense::hover());
    let w = width / labels.len() as f32;
    let mut out = None;
    for (i, l) in labels.iter().enumerate() {
        let sr = Rect::from_min_size(pos2(r.min.x + w * i as f32, r.min.y), vec2(w, BUTTON_H));
        let resp = ui.interact(sr, ui.id().with(("seg", l)), Sense::click());
        if i == active {
            ui.painter().rect_filled(sr, 0.0, t.well);
        } else if resp.hovered() {
            ui.painter().rect_filled(sr, 0.0, t.hover);
        }
        crate::rtl::paint(ui.painter(), sr.center(), egui::Align2::CENTER_CENTER, l, label_font(), if i == active { t.accent } else { t.text });
        if i > 0 {
            ui.painter().line_segment([sr.left_top(), sr.left_bottom()], Stroke::new(1.0, t.field_border));
        }
        if resp.clicked() {
            out = Some(i);
        }
    }
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    out
}

/// Scrolling for a bar that a small window cuts off (Tools panel, Control panel, application
/// bar): the mouse wheel scrolls the bar's only direction, and a thin scroll bar floats over the
/// contents, so nothing moves and nothing shows while everything fits.
pub fn overflow_scrolling(ui: &mut Ui) {
    let style = ui.style_mut();
    style.always_scroll_the_only_direction = true;
    // A caption at the end of a scrolled row gets only what is left of it: extend, don't wrap.
    style.wrap_mode = Some(egui::TextWrapMode::Extend);
    let s = &mut style.spacing.scroll;
    *s = egui::style::ScrollStyle::floating();
    s.bar_width = 6.0;
    // A faint handle while the pointer is elsewhere: the bar shows there is more.
    s.dormant_handle_opacity = 0.5;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ctx: &egui::Context, events: Vec<egui::Event>, mods: egui::Modifiers, f: &mut dyn FnMut(&mut Ui)) {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 200.0))),
            events: std::iter::once(egui::Event::ModifiersChanged(mods)).chain(events).collect(),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| f(ui));
        out.textures_delta.clear();
    }

    fn click(p: egui::Pos2, mods: egui::Modifiers) -> Vec<Vec<egui::Event>> {
        vec![
            vec![egui::Event::PointerMoved(p)],
            vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: mods }],
            vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: mods }],
        ]
    }

    /// Drive a spinner field through frames; returns every committed value and the field rect.
    fn drive(frames: Vec<(Vec<egui::Event>, egui::Modifiers)>, value: f64) -> (Vec<f64>, Rect) {
        let ctx = egui::Context::default();
        let mut got = vec![];
        let mut rect = Rect::NOTHING;
        let mut all = vec![(vec![], egui::Modifiers::NONE)];
        all.extend(frames);
        for (ev, m) in all {
            run(&ctx, ev, m, &mut |ui| {
                rect = ui.available_rect_before_wrap();
                if let Some(v) = NumField::measure("w", Some(value), Unit::Points).width(80.0).spinner().show(ui) {
                    got.push(v);
                }
            });
        }
        (got, rect)
    }

    #[test]
    fn spinner_up_down_and_shift() {
        let (_, r) = drive(vec![], 10.0);
        let up = r.min + vec2(9.0, 5.0);
        let down = r.min + vec2(9.0, 16.0);
        let (got, _) = drive(click(up, egui::Modifiers::NONE).into_iter().map(|e| (e, egui::Modifiers::NONE)).collect(), 10.0);
        assert_eq!(got, vec![11.0]);
        let (got, _) = drive(click(down, egui::Modifiers::NONE).into_iter().map(|e| (e, egui::Modifiers::NONE)).collect(), 10.0);
        assert_eq!(got, vec![9.0]);
        let (got, _) = drive(click(up, egui::Modifiers::SHIFT).into_iter().map(|e| (e, egui::Modifiers::SHIFT)).collect(), 10.0);
        assert_eq!(got, vec![20.0]);
    }

    #[test]
    fn arrow_keys_step_and_typing_parses_arithmetic() {
        let (_, r) = drive(vec![], 10.0);
        let text = r.min + vec2(50.0, 10.0);
        let mut frames: Vec<(Vec<egui::Event>, egui::Modifiers)> =
            click(text, egui::Modifiers::NONE).into_iter().map(|e| (e, egui::Modifiers::NONE)).collect();
        let key = |k| egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        frames.push((vec![key(egui::Key::ArrowUp)], egui::Modifiers::NONE));
        let (got, _) = drive(frames.clone(), 10.0);
        assert_eq!(got, vec![11.0]);
        // Select all, type an expression, press Enter.
        frames.truncate(3);
        frames.push((
            vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }],
            egui::Modifiers::COMMAND,
        ));
        frames.push((vec![egui::Event::Text("6+6".into())], egui::Modifiers::NONE));
        frames.push((vec![key(egui::Key::Enter)], egui::Modifiers::NONE));
        let (got, _) = drive(frames, 10.0);
        assert_eq!(got, vec![12.0]);
    }

    #[test]
    fn number_kind_formats_and_parses() {
        let k = NumKind::Number { suffix: "%", decimals: 0 };
        assert_eq!(k.format(100.0), "100%");
        assert_eq!(k.parse("50%"), Some(50.0));
        assert_eq!(k.parse("10*3"), Some(30.0));
        let m = NumKind::Measure(Unit::Picas);
        assert_eq!(m.parse("1p0+0p6"), Some(18.0));
        assert_eq!(m.step(), 1.0);
    }
}
