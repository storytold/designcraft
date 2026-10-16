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
        self.show_or_clear(ui).flatten()
    }

    /// Like [`NumField::show`]; a field emptied while it had a value commits `Some(None)` (for
    /// values that may be unset, like a character style's attributes).
    pub fn show_or_clear(self, ui: &mut Ui) -> Option<Option<f64>> {
        let t = Tokens::get(ui.ctx());
        let key = ui.id().with(self.id);
        let (rect, _) = ui.allocate_exact_size(vec2(self.width, FIELD_H), Sense::hover());
        let step = self.step.unwrap_or_else(|| self.kind.step());
        let clamp = |v: f64| v.clamp(self.range.0, self.range.1);
        let mut out: Option<f64> = None;
        let mut cleared = false;
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
            } else if buf.trim().is_empty() && self.value.is_some() {
                cleared = true;
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
        if cleared { Some(None) } else { out.map(Some) }
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
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, text));
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
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
    crate::rtl::paint(ui.painter(), r.left_center(), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(11.5), t.text_strong);
    // Screen readers hear the section's title.
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, title));
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
    // Named by its tooltip, for screen readers.
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, tip));
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_ui(|ui| {
            crate::rtl::label(ui, tip);
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CheckboxDrag {
    origin: egui::Id,
    target: bool,
    deferred: bool,
    dragging: bool,
    last_held_frame: u64,
    observed_pass: Option<u64>,
    cancelled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CheckboxCompletion {
    endpoint: egui::Id,
    position: egui::Pos2,
    target: bool,
    frame: u64,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct CheckboxGesture {
    press_frame: Option<u64>,
    click_origin: Option<(egui::Id, u64)>,
    drag: Option<CheckboxDrag>,
    early_release: Option<(egui::Id, u64)>,
    completion: Option<CheckboxCompletion>,
    settled_completion: Option<CheckboxCompletion>,
    cancelled_frame: Option<u64>,
}

/// Resolve gestures before painting. Sparse origins wait for a drag so their
/// ordinary unset -> on -> off -> unset click cycle remains intact.
fn checkbox_value(ui: &Ui, response: &Response, value: Option<bool>, sparse: bool) -> Option<bool> {
    let ctx = ui.ctx();
    let key = egui::Id::new("designcraft-checkbox-drag").with(ctx.viewport_id());
    let frame = ctx.cumulative_frame_nr();
    let pass = ctx.cumulative_pass_nr();
    let (pressed, down, released, dragging, press_pos, release_pos, cancelled) = ui.input(|i| {
        let press = i.events.iter().enumerate().rev().find_map(|(index, event)| match event {
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. } => Some((index, *pos)),
            _ => None,
        });
        let press_pos = press.map(|(_, pos)| pos);
        // An earlier gesture's release cannot complete the latest press.
        let release_pos = i.events.iter().skip(press.map_or(0, |(index, _)| index + 1)).find_map(|event| match event {
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, .. } => Some(*pos),
            _ => None,
        });
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.primary_released(),
            i.pointer.is_decidedly_dragging(),
            press_pos,
            release_pos,
            i.key_pressed(egui::Key::Escape),
        )
    });
    let finishing = released && !down && release_pos.is_some();
    let stored = ctx.data(|d| d.get_temp::<CheckboxGesture>(key)).unwrap_or_default();
    let mut state = stored;
    if pressed && state.press_frame != Some(frame) {
        state = CheckboxGesture { press_frame: Some(frame), ..Default::default() };
    } else if !down && !released {
        // Keep the release frame until every row has suppressed the origin click.
        state.click_origin = None;
        state.drag = None;
        state.early_release = None;
    }
    let cancelled = cancelled || state.cancelled_frame == Some(frame);
    if state.completion.is_some_and(|c| cancelled || (frame != c.frame && (pressed || down || released || frame.saturating_sub(c.frame) != 1))) {
        state.completion = None;
    }
    if state.settled_completion.is_some_and(|c| cancelled || pressed || frame.saturating_sub(c.frame) != 1) {
        state.settled_completion = None;
    }
    if cancelled || state.click_origin.is_some_and(|(_, held)| frame.saturating_sub(held) > 1) {
        state.click_origin = None;
    }
    if let Some(drag) = &mut state.drag {
        // egui clears capture even on frames with no checkbox calls. A release
        // may finish only a gesture observed held in this or the preceding frame.
        let origin = ctx.read_response(drag.origin);
        let owns = origin.as_ref().is_some_and(|origin| {
            origin.enabled() && ctx.memory(|m| m.allows_interaction(origin.layer_id)) && (!down || origin.is_pointer_button_down_on())
        });
        if cancelled || !owns || frame.saturating_sub(drag.last_held_frame) > 1 {
            // Retain the origin receipt so cancellation cannot turn its release
            // into a second ordinary click; it no longer authorizes painting.
            drag.cancelled = true;
        }
        // Classify a deferred release before any row can apply the click cycle.
        // Leaving the origin commits its drag target, even below egui's click
        // distance; only eligible neighbors can receive that assignment.
        if !drag.cancelled
            && drag.deferred
            && finishing
            && release_pos.zip(origin.as_ref()).is_some_and(|(pos, origin)| {
                let local = ctx.layer_transform_from_global(origin.layer_id).map_or(pos, |transform| transform * pos);
                !origin.interact_rect.contains(local)
            })
        {
            drag.dragging = true;
        }
    }
    let enabled = ui.is_enabled() && response.enabled();
    let over = enabled && response.contains_pointer() && ui.rect_contains_pointer(response.interact_rect);
    let eligible_at = |pos| {
        let local = ctx.layer_transform_from_global(response.layer_id).map_or(pos, |transform| transform * pos);
        enabled
            && ui.clip_rect().intersect(response.interact_rect).contains(local)
            && ctx.layer_id_at(pos) == Some(response.layer_id)
            && ctx.memory(|m| m.allows_interaction(response.layer_id))
    };
    // egui can give capture to the final hover in a press + move batch. Also
    // require the actual primary press to be inside this eligible checkbox.
    let press_over = press_pos.is_some_and(eligible_at);
    // Mixed origins cannot arm a paint drag, but an ordinary primary click must
    // still belong to the checkbox that actually received the eligible press.
    if pressed && !cancelled && press_over {
        state.click_origin = Some((response.id, frame));
    }
    if down
        && enabled
        && response.is_pointer_button_down_on()
        && ctx.memory(|m| m.allows_interaction(response.layer_id))
        && let Some((origin, held)) = &mut state.click_origin
        && *origin == response.id
    {
        *held = frame;
    }
    // Input can contain move + release + later hover. Use the release event's
    // position, never the later hover position, and do not interpolate a path.
    let release_over = release_pos.is_some_and(eligible_at);
    // The endpoint may be drawn before its origin, including a held release.
    // Only a fresh origin in this same pass can authorize this tentative endpoint.
    if finishing && !cancelled && release_over && value.is_some() && state.drag.is_none_or(|drag| !drag.cancelled && drag.observed_pass != Some(pass))
    {
        state.early_release = Some((response.id, pass));
    }
    if pressed
        && (down || finishing)
        && !cancelled
        && press_over
        && state.drag.is_none()
        && let Some(on) = value
    {
        // A press on this checkbox followed by motion can give egui's capture to
        // another widget. Correct only that disagreement, using the public API.
        if down && !response.is_pointer_button_down_on() {
            ctx.set_dragged_id(response.id);
        }
        let crossed = finishing
            && release_pos.is_some_and(|pos| {
                let local = ctx.layer_transform_from_global(response.layer_id).map_or(pos, |transform| transform * pos);
                !response.interact_rect.contains(local)
            });
        state.drag = Some(CheckboxDrag {
            origin: response.id,
            target: !on,
            deferred: sparse,
            dragging: crossed,
            last_held_frame: frame,
            observed_pass: None,
            cancelled: false,
        });
        // Held gestures and completed batches both get a fresh endpoint pass.
        ctx.request_repaint();
    }
    if let Some(drag) = &mut state.drag
        && !drag.cancelled
    {
        if response.id == drag.origin
            && value.is_some()
            && enabled
            && ui.clip_rect().intersect(response.interact_rect).is_positive()
            && ctx.memory(|m| m.allows_interaction(response.layer_id))
        {
            // read_response can fall back to the previous pass. Its geometry
            // preserves capture checks but cannot prove the origin still exists.
            drag.observed_pass = Some(pass);
            if finishing
                && let Some((endpoint, seen_pass)) = state.early_release
                && seen_pass == pass
                && endpoint != drag.origin
                && let Some(position) = release_pos
            {
                state.completion = Some(CheckboxCompletion { endpoint, position, target: drag.target, frame });
                ctx.request_repaint();
            }
        }
        drag.dragging |= dragging || ((down && over || finishing && release_over) && response.id != drag.origin);
        if down {
            drag.last_held_frame = frame;
        }
    }
    let click_value = || {
        if sparse {
            match value {
                None => Some(true),
                Some(true) => Some(false),
                Some(false) => None,
            }
        } else {
            Some(!value.unwrap_or(false))
        }
    };
    let completion = state.completion.filter(|c| c.frame != frame && c.endpoint == response.id);
    if completion.is_some() {
        // Consume even if this endpoint became mixed, disabled, clipped or moved.
        state.completion = None;
    }
    let mut next = value;
    if enabled {
        if response.clicked() && !response.clicked_by(egui::PointerButton::Primary) {
            // Keyboard/accessibility activation is independent of pointer dragging.
            next = click_value();
        } else if let Some(completion) = completion {
            // Fresh allocation must still authorize the recorded release point.
            if value.is_some() && eligible_at(completion.position) {
                next = Some(completion.target);
                state.settled_completion = Some(completion);
            }
        } else if let Some(drag) = state.drag {
            let at_target = down && (over || response.id == drag.origin)
                || finishing && drag.observed_pass == Some(pass) && (release_over || response.id == drag.origin && drag.dragging);
            if !drag.cancelled && at_target && value.is_some() && (!drag.deferred || drag.dragging || response.id != drag.origin) {
                next = Some(drag.target);
            } else if response.clicked() && !drag.cancelled && finishing && response.id == drag.origin && drag.deferred && !drag.dragging {
                // Capture correction can leave egui's click candidate on a
                // neighbor. Only the genuine sparse origin may cycle here.
                next = click_value();
            }
        } else if response.clicked() && state.click_origin.is_some_and(|(origin, _)| origin == response.id) {
            // Includes genuine mixed-state clicks and press/release in one frame.
            next = click_value();
        }
    }
    if state != stored {
        ctx.data_mut(|d| d.insert_temp(key, state));
    }
    next
}

/// End pointer edits when their document or dialog is replaced. Keep a receipt
/// for this frame so later rows cannot rearm from the same raw input.
pub(crate) fn cancel_checkbox_gesture(ctx: &egui::Context) {
    let key = egui::Id::new("designcraft-checkbox-drag").with(ctx.viewport_id());
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|data| {
        let mut state = data.get_temp::<CheckboxGesture>(key).unwrap_or_default();
        state.press_frame = Some(frame);
        state.cancelled_frame = Some(frame);
        state.click_origin = None;
        state.early_release = None;
        state.completion = None;
        state.settled_completion = None;
        if let Some(drag) = &mut state.drag {
            drag.cancelled = true;
        }
        data.insert_temp(key, state);
    });
}

/// An option edit can resolve a covered-cell caret to its owner while keeping
/// the same selected cells. Compare that harmless normalization only when the
/// selection actually changes; owner lookup allocates a table-sized map.
fn checkbox_selection_normalized(
    previous: &designcraft_doc::Selection,
    current: &designcraft_doc::Selection,
    doc: &designcraft_doc::Document,
) -> bool {
    let (Some(mut text), Some(current_text)) = (previous.text, current.text) else { return false };
    let (Some(cell), Some(current_cell)) = (text.cell, current_text.cell) else { return false };
    if text.story != current_text.story || cell.table != current_cell.table {
        return false;
    }
    text.cell = Some(current_cell);
    if text != current_text {
        return false;
    }
    let designcraft_doc::Selection { items, content, anchors, text: _, cells, key, page } = previous;
    if items != &current.items
        || *content != current.content
        || anchors != &current.anchors
        || cells != &current.cells
        || key != &current.key
        || page != &current.page
    {
        return false;
    }
    let Some(table) = doc.story(text.story).and_then(|story| story.tables.get(&cell.table)) else { return false };
    if table.cell(cell.row, cell.col).is_none() {
        return false;
    }
    let owner = table.owner(cell.row, cell.col);
    owner != (cell.row, cell.col) && owner == (current_cell.row, current_cell.col)
}

/// A panel can reuse widget IDs and geometry for a different document or selection.
pub(crate) fn checkbox_context(ctx: &egui::Context, context: Option<(u64, &designcraft_doc::Selection, &designcraft_doc::Document)>) {
    let key = egui::Id::new("designcraft-checkbox-context").with(ctx.viewport_id());
    let changed = ctx.data_mut(|data| {
        let previous = data.get_temp_mut_or_insert_with(key, || context.map(|(uid, selection, _)| (uid, selection.clone())));
        let changed = previous.as_ref().map(|(uid, selection)| (*uid, selection)) != context.map(|(uid, selection, _)| (uid, selection));
        let cancel = changed
            && match (previous.as_ref(), context) {
                (Some((old_uid, old_selection)), Some((uid, selection, doc))) => {
                    *old_uid != uid || !checkbox_selection_normalized(old_selection, selection, doc)
                }
                _ => true,
            };
        if changed {
            *previous = context.map(|(uid, selection, _)| (uid, selection.clone()));
        }
        cancel
    });
    if changed {
        cancel_checkbox_gesture(ctx);
    }
}

/// A reverse completed batch needs its current endpoint redrawn before a
/// dialog can commit all of its checkbox edits.
pub(crate) fn checkbox_completion_pending(ctx: &egui::Context) -> Option<CheckboxCompletion> {
    let key = egui::Id::new("designcraft-checkbox-drag").with(ctx.viewport_id());
    let frame = ctx.cumulative_frame_nr();
    ctx.data(|d| d.get_temp::<CheckboxGesture>(key).and_then(|state| state.completion.filter(|completion| completion.frame == frame)))
}

/// A queued dialog confirmation applies only after its eligible endpoint has
/// received the recorded value. Missing or cancelled endpoints cannot confirm.
pub(crate) fn checkbox_completion_settled(ctx: &egui::Context, completion: CheckboxCompletion) -> bool {
    let key = egui::Id::new("designcraft-checkbox-drag").with(ctx.viewport_id());
    ctx.cumulative_frame_nr().saturating_sub(completion.frame) == 1
        && ctx.data(|d| d.get_temp::<CheckboxGesture>(key).is_some_and(|state| state.settled_completion == Some(completion)))
}

/// An ordinary checkbox click may close a popup; a crossed or cancelled primary
/// gesture must leave its consumer alive, including a pending release endpoint.
pub fn checkbox_clicked(ui: &Ui, response: &Response) -> bool {
    if !response.clicked() {
        return false;
    }
    if !response.clicked_by(egui::PointerButton::Primary) {
        return true; // keyboard/accessibility activation
    }
    let key = egui::Id::new("designcraft-checkbox-drag").with(ui.ctx().viewport_id());
    ui.ctx().data(|d| {
        d.get_temp::<CheckboxGesture>(key).is_some_and(|state| {
            state.click_origin.is_some_and(|(origin, _)| origin == response.id) && state.drag.is_none_or(|drag| !drag.dragging && !drag.cancelled)
        })
    })
}

/// Keep egui's native checkbox appearance, labels and keyboard activation.
pub fn native_checkbox(ui: &mut Ui, on: &mut bool, label: impl Into<egui::WidgetText>) -> Response {
    let mut value = Some(*on);
    let response = native_checkbox_value(ui, &mut value, label.into(), false);
    *on = value.unwrap_or(false);
    response
}

/// Sparse style attributes keep their existing three-state click cycle.
pub fn sparse_checkbox(ui: &mut Ui, value: &mut Option<bool>, label: impl Into<egui::WidgetText>) -> Response {
    native_checkbox_value(ui, value, label.into(), true)
}

fn native_checkbox_value(ui: &mut Ui, value: &mut Option<bool>, label: egui::WidgetText, sparse: bool) -> Response {
    // Distinct labels must not share a held origin when earlier controls disappear.
    let id = egui::Id::new(("native_checkbox", label.text()));
    ui.push_id(id, |ui| {
        let before = *value;
        let mut painted = before.unwrap_or(false);
        if ui.is_enabled() && ui.ctx().read_response(ui.next_auto_id()).is_some_and(|r| r.clicked()) {
            // Compensate egui's click toggle for painting only. A previous rectangle
            // must never authorize a model write after the layout has moved.
            painted = !painted;
        }
        // Allocate current geometry before resolving the gesture. interact returns
        // fresh input flags, so changed reflects only our actual model assignment.
        let mut response = ui.add(egui::Checkbox::new(&mut painted, label).indeterminate(before.is_none())).interact(Sense::click_and_drag());
        *value = checkbox_value(ui, &response, before, sparse);
        if *value != before {
            response.mark_changed();
        }
        if *value != before || painted != value.unwrap_or(false) {
            ui.ctx().request_repaint();
        }
        response
    })
    .inner
}

/// An InDesign checkbox: 12 pt rounded box, white check, label.
pub fn checkbox(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let g = ui.painter().layout_no_wrap(label.to_string(), label_font(), t.text);
    let (r, mut resp) = ui.allocate_exact_size(vec2(18.0 + g.size().x, FIELD_H), Sense::click_and_drag());
    let before = *on;
    *on = checkbox_value(ui, &resp, Some(before), false).unwrap_or(before);
    if *on != before {
        resp.mark_changed();
    }
    let b = Rect::from_center_size(pos2(r.min.x + 6.5, r.center().y), vec2(12.0, 12.0));
    if *on {
        ui.painter().rect_filled(b, 2.0, t.icon);
        ui.painter().line_segment([b.min + vec2(2.5, 6.0), b.min + vec2(5.0, 8.5)], Stroke::new(1.5, t.panel));
        ui.painter().line_segment([b.min + vec2(5.0, 8.5), b.min + vec2(9.5, 3.5)], Stroke::new(1.5, t.panel));
    } else {
        ui.painter().rect_stroke(b, 2.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    }
    ui.painter().galley(pos2(r.min.x + 18.0, r.center().y - g.size().y / 2.0), g, t.text);
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
    fn emptying_a_clearable_field_clears_it() {
        let ctx = egui::Context::default();
        let field = |ui: &mut Ui| NumField::number("c", Some(12.0), " pt", 2).width(80.0).show_or_clear(ui);
        let mut rect = Rect::NOTHING;
        run(&ctx, vec![], egui::Modifiers::NONE, &mut |ui| {
            rect = ui.available_rect_before_wrap();
            field(ui);
        });
        let key = |k, m| egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m };
        let mut frames: Vec<Vec<egui::Event>> = click(rect.min + vec2(40.0, 10.0), egui::Modifiers::NONE);
        frames.push(vec![key(egui::Key::A, egui::Modifiers::COMMAND)]);
        frames.push(vec![key(egui::Key::Backspace, egui::Modifiers::NONE)]);
        frames.push(vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
        let mut got = vec![];
        for ev in frames {
            let m = if ev.iter().any(|e| matches!(e, egui::Event::Key { modifiers, .. } if modifiers.command)) {
                egui::Modifiers::COMMAND
            } else {
                egui::Modifiers::NONE
            };
            run(&ctx, ev, m, &mut |ui| {
                if let Some(v) = field(ui) {
                    got.push(v);
                }
            });
        }
        assert_eq!(got, vec![None]);
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

    fn checkbox_row(ctx: &egui::Context, events: Vec<egui::Event>, values: &mut [bool; 3], custom: bool) -> Vec<Response> {
        let mut responses = Vec::new();
        run(ctx, events, egui::Modifiers::NONE, &mut |ui| {
            responses.clear();
            let scroll = egui::ScrollArea::vertical().max_height(135.0).show(ui, |ui| {
                for (i, label) in ["Alpha", "Beta", "Gamma"].into_iter().enumerate() {
                    responses.push(if custom && i != 1 { checkbox(ui, &mut values[i], label) } else { native_checkbox(ui, &mut values[i], label) });
                }
                ui.add_space(240.0);
            });
            assert_eq!(scroll.state.offset.y, 0.0, "checkbox drag must not pan its scroll area");
        });
        responses
    }

    fn primary(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn checkbox_drag_assigns_check_and_uncheck_once_across_both_appearances() {
        for custom in [false, true] {
            for target in [true, false] {
                let ctx = egui::Context::default();
                let mut values = [!target, !target, target];
                checkbox_row(&ctx, vec![], &mut values, custom);
                let rows = checkbox_row(&ctx, vec![], &mut values, custom);
                let points: Vec<_> = rows.iter().map(|r| r.rect.center()).collect();
                let outside = pos2(380.0, 180.0);
                let frames = [
                    (vec![egui::Event::PointerMoved(points[0]), primary(points[0], true)], [target, !target, target]),
                    (vec![egui::Event::PointerMoved(points[1])], [target; 3]),
                    (vec![egui::Event::PointerMoved(points[2])], [target; 3]),
                    (vec![egui::Event::PointerMoved(points[0])], [target; 3]),
                    (vec![egui::Event::PointerMoved(points[1])], [target; 3]),
                    (vec![egui::Event::PointerMoved(outside), primary(outside, false)], [target; 3]),
                    (vec![egui::Event::PointerMoved(points[2])], [target; 3]),
                ];
                let mut changes = [0; 3];
                for (events, expected) in frames {
                    for (i, response) in checkbox_row(&ctx, events, &mut values, custom).iter().enumerate() {
                        changes[i] += usize::from(response.changed());
                    }
                    assert_eq!(values, expected);
                }
                assert_eq!(changes, [1, 1, 0], "dwell, revisit, equal value and release are not edits");
                // A fresh press outside must not reuse the previous target.
                for events in [
                    vec![egui::Event::PointerMoved(outside), primary(outside, true)],
                    vec![egui::Event::PointerMoved(points[0])],
                    vec![primary(points[0], false)],
                ] {
                    assert!(checkbox_row(&ctx, events, &mut values, custom).iter().all(|r| !r.changed()));
                    assert_eq!(values, [target; 3]);
                }
                // The next checkbox press establishes the opposite target.
                checkbox_row(&ctx, vec![primary(points[0], true)], &mut values, custom);
                assert_eq!(values, [!target, target, target]);
                checkbox_row(&ctx, vec![primary(points[0], false)], &mut values, custom);
                assert_eq!(values, [!target, target, target]);
            }
        }
        // Start from the native middle row and cross onto the custom first row.
        for target in [true, false] {
            let ctx = egui::Context::default();
            let mut values = [!target, !target, target];
            checkbox_row(&ctx, vec![], &mut values, true);
            let rows = checkbox_row(&ctx, vec![], &mut values, true);
            let points: Vec<_> = rows.iter().map(|r| r.rect.center()).collect();
            let mut changes = [0; 3];
            for (events, expected) in [
                (vec![egui::Event::PointerMoved(points[1]), primary(points[1], true)], [!target, target, target]),
                (vec![egui::Event::PointerMoved(points[0])], [target; 3]),
                (vec![egui::Event::PointerMoved(points[2])], [target; 3]),
                (vec![primary(points[2], false)], [target; 3]),
            ] {
                for (i, response) in checkbox_row(&ctx, events, &mut values, true).iter().enumerate() {
                    changes[i] += usize::from(response.changed());
                }
                assert_eq!(values, expected);
            }
            assert_eq!(changes, [1, 1, 0]);
        }
    }

    #[test]
    fn checkbox_drag_does_not_survive_checkbox_free_release_and_new_blank_press() {
        for custom in [false, true] {
            for target in [true, false] {
                let ctx = egui::Context::default();
                let mut values = [!target; 3];
                checkbox_row(&ctx, vec![], &mut values, custom);
                let rows = checkbox_row(&ctx, vec![], &mut values, custom);
                let points: Vec<_> = rows.iter().map(|r| r.rect.center()).collect();
                let outside = pos2(380.0, 180.0);
                checkbox_row(&ctx, vec![egui::Event::PointerMoved(points[0]), primary(points[0], true)], &mut values, custom);
                assert_eq!(values, [target, !target, !target]);
                // No adapter sees either boundary; temporary egui data survives these frames.
                for events in [vec![egui::Event::PointerMoved(outside), primary(outside, false)], vec![primary(outside, true)]] {
                    run(&ctx, events, egui::Modifiers::NONE, &mut |_| {});
                }
                for events in [
                    vec![], // Register restored native widgets while the new blank press is held.
                    vec![],
                    vec![egui::Event::PointerMoved(points[2])],
                    vec![],
                    vec![primary(points[2], false)],
                    vec![egui::Event::PointerMoved(points[1])],
                ] {
                    let responses = checkbox_row(&ctx, events, &mut values, custom);
                    assert!(responses.iter().all(|response| !response.changed()));
                    assert_eq!(values, [target, !target, !target], "targets must oppose the stale target to expose an accidental assignment");
                }
                // A fresh checkbox press still arms painting, once per changed control.
                let mut changes = [0; 3];
                for events in [
                    vec![egui::Event::PointerMoved(points[2]), primary(points[2], true)],
                    vec![egui::Event::PointerMoved(points[1])],
                    vec![primary(points[1], false)],
                ] {
                    for (i, response) in checkbox_row(&ctx, events, &mut values, custom).iter().enumerate() {
                        changes[i] += usize::from(response.changed());
                    }
                }
                assert_eq!(values, [target; 3]);
                assert_eq!(changes, [0, 1, 1]);
                // Escape stops painting and still suppresses an ordinary release on the origin.
                for release_on_origin in [true, false] {
                    let before = values;
                    checkbox_row(&ctx, vec![egui::Event::PointerMoved(points[0]), primary(points[0], true)], &mut values, custom);
                    let expected = [!before[0], target, target];
                    assert_eq!(values, expected);
                    let release_at = if release_on_origin { points[0] } else { points[2] };
                    for events in [
                        vec![egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                        vec![egui::Event::PointerMoved(release_at)],
                        vec![primary(release_at, false)],
                        vec![egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: false,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                    ] {
                        assert!(checkbox_row(&ctx, events, &mut values, custom).iter().all(|response| !response.changed()));
                        assert_eq!(values, expected);
                    }
                }
            }
        }
    }

    fn release_checkbox_row(ctx: &egui::Context, events: Vec<egui::Event>, values: &mut [Option<bool>; 3], appearance: &str) -> Vec<Response> {
        let mut responses = Vec::new();
        run(ctx, events, egui::Modifiers::NONE, &mut |ui| {
            responses.clear();
            for (value, label) in values.iter_mut().zip(["Origin", "Release target", "Later hover"]) {
                responses.push(if appearance == "sparse" {
                    sparse_checkbox(ui, value, label)
                } else {
                    let mut on = value.unwrap();
                    let response = if appearance == "custom" { checkbox(ui, &mut on, label) } else { native_checkbox(ui, &mut on, label) };
                    *value = Some(on);
                    response
                });
            }
        });
        responses
    }

    #[test]
    fn checkbox_drag_finishes_batched_release_endpoint_without_painting_later_hover() {
        for appearance in ["native", "custom", "sparse"] {
            for target in [true, false] {
                for batched in [false, true] {
                    let ctx = egui::Context::default();
                    let mut values = [Some(!target); 3];
                    release_checkbox_row(&ctx, vec![], &mut values, appearance);
                    let rows = release_checkbox_row(&ctx, vec![], &mut values, appearance);
                    let points: Vec<_> = rows.iter().map(|r| r.rect.center()).collect();
                    let mut changes = [0; 3];
                    let mut frame = |events| {
                        for (i, response) in release_checkbox_row(&ctx, events, &mut values, appearance).iter().enumerate() {
                            changes[i] += usize::from(response.changed());
                        }
                    };
                    frame(vec![egui::Event::PointerMoved(points[0]), primary(points[0], true)]);
                    if !batched {
                        frame(vec![egui::Event::PointerMoved(points[1])]);
                    }
                    frame(vec![egui::Event::PointerMoved(points[1]), primary(points[1], false), egui::Event::PointerMoved(points[2])]);
                    // Neither later hover nor an unarmed release may paint the third checkbox.
                    frame(vec![]);
                    frame(vec![egui::Event::PointerMoved(points[2])]);
                    frame(vec![primary(points[2], false)]);
                    assert_eq!(values, [Some(target), Some(target), Some(!target)], "{appearance}, batched={batched}");
                    assert_eq!(changes, [1, 1, 0], "{appearance}, batched={batched}");
                }
            }
        }
        // Sparse ordinary clicks still cycle off -> unset -> on -> off.
        let ctx = egui::Context::default();
        let mut values = [Some(false); 3];
        release_checkbox_row(&ctx, vec![], &mut values, "sparse");
        let rows = release_checkbox_row(&ctx, vec![], &mut values, "sparse");
        let point = rows[0].rect.center();
        for expected in [None, Some(true), Some(false)] {
            let before = values;
            let press = release_checkbox_row(&ctx, vec![egui::Event::PointerMoved(point), primary(point, true)], &mut values, "sparse");
            assert_eq!(values, before);
            assert!(press.iter().all(|response| !response.changed()));
            let release = release_checkbox_row(&ctx, vec![primary(point, false)], &mut values, "sparse");
            assert_eq!(values, [expected, Some(false), Some(false)]);
            assert!(release[0].changed());
            assert!(release[1..].iter().all(|response| !response.changed()));
        }
        // A mixed, disabled or clipped release target stays unchanged.
        for target in [true, false] {
            for guarded in [1, 2, 3] {
                let ctx = egui::Context::default();
                let mut values = [Some(!target), None, Some(!target), Some(!target), Some(!target)];
                guarded_checkbox_row(&ctx, vec![], &mut values);
                let rows = guarded_checkbox_row(&ctx, vec![], &mut values);
                let start = rows[0].rect.center();
                let end = rows[guarded].rect.center();
                guarded_checkbox_row(&ctx, vec![egui::Event::PointerMoved(start), primary(start, true)], &mut values);
                let responses = guarded_checkbox_row(&ctx, vec![egui::Event::PointerMoved(end), primary(end, false)], &mut values);
                assert_eq!(values, [Some(target), None, Some(!target), Some(!target), Some(!target)]);
                assert!(responses[0].changed());
                assert!(responses[1..].iter().all(|response| !response.changed()));
            }
        }
    }

    fn guarded_checkbox_row(ctx: &egui::Context, events: Vec<egui::Event>, values: &mut [Option<bool>; 5]) -> Vec<Response> {
        let mut responses = Vec::new();
        run(ctx, events, egui::Modifiers::NONE, &mut |ui| {
            responses.clear();
            for (i, label) in ["Start", "Mixed", "Disabled", "Clipped", "End"].into_iter().enumerate() {
                responses.push(
                    ui.scope(|ui| {
                        if i == 3 {
                            ui.set_clip_rect(Rect::NOTHING);
                        }
                        ui.add_enabled_ui(i != 2, |ui| sparse_checkbox(ui, &mut values[i], label)).inner
                    })
                    .inner,
                );
            }
        });
        responses
    }

    #[test]
    fn checkbox_drag_skips_mixed_disabled_clipped_and_non_checkbox_starts() {
        let original = [Some(false), None, Some(false), Some(false), Some(false)];
        let ctx = egui::Context::default();
        let mut values = original;
        guarded_checkbox_row(&ctx, vec![], &mut values);
        let rows = guarded_checkbox_row(&ctx, vec![], &mut values);
        let points: Vec<_> = rows.iter().map(|r| r.rect.center()).collect();
        guarded_checkbox_row(&ctx, vec![egui::Event::PointerMoved(points[0]), primary(points[0], true)], &mut values);
        assert_eq!(values, original, "sparse origin defers its click cycle");
        for &point in &points[1..] {
            guarded_checkbox_row(&ctx, vec![egui::Event::PointerMoved(point)], &mut values);
        }
        assert_eq!(values, [Some(true), None, Some(false), Some(false), Some(true)]);
        guarded_checkbox_row(&ctx, vec![primary(points[4], false)], &mut values);
        assert_eq!(values, [Some(true), None, Some(false), Some(false), Some(true)]);

        for start in [points[1], points[2], points[3], pos2(380.0, 180.0)] {
            let ctx = egui::Context::default();
            values = original;
            guarded_checkbox_row(&ctx, vec![], &mut values);
            guarded_checkbox_row(&ctx, vec![], &mut values);
            for events in [
                vec![egui::Event::PointerMoved(start), primary(start, true)],
                vec![egui::Event::PointerMoved(points[4])],
                vec![primary(points[4], false)],
            ] {
                assert!(guarded_checkbox_row(&ctx, events, &mut values).iter().all(|r| !r.changed()));
                assert_eq!(values, original);
            }
        }
        // A secondary-button drag is not a checkbox gesture.
        let ctx = egui::Context::default();
        values = original;
        guarded_checkbox_row(&ctx, vec![], &mut values);
        guarded_checkbox_row(&ctx, vec![], &mut values);
        for (pos, pressed) in [(points[0], true), (points[4], false)] {
            guarded_checkbox_row(
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE },
                ],
                &mut values,
            );
            assert_eq!(values, original);
        }
    }

    #[test]
    fn checkbox_label_click_changes_on_press_closes_on_click_and_keeps_keyboard_activation() {
        for custom in [false, true] {
            let ctx = egui::Context::default();
            let mut values = [false; 3];
            checkbox_row(&ctx, vec![], &mut values, custom);
            let rows = checkbox_row(&ctx, vec![], &mut values, custom);
            let label = pos2(rows[0].rect.right() - 2.0, rows[0].rect.center().y);
            let press = checkbox_row(&ctx, vec![egui::Event::PointerMoved(label), primary(label, true)], &mut values, custom);
            assert!(press[0].changed());
            assert!(!press[0].clicked(), "popup stays open during the press");
            assert_eq!(values, [true, false, false]);
            let release = checkbox_row(&ctx, vec![primary(label, false)], &mut values, custom);
            assert!(!release[0].changed());
            assert!(release[0].clicked(), "completed click can close the popup without repeating the command");
            assert_eq!(values, [true, false, false]);
            release[0].request_focus();
            for (key, expected) in [(egui::Key::Space, false), (egui::Key::Enter, true)] {
                let response = checkbox_row(
                    &ctx,
                    vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
                    &mut values,
                    custom,
                );
                assert!(response[0].changed() && response[0].clicked());
                assert_eq!(values, [expected, false, false]);
                let response = checkbox_row(
                    &ctx,
                    vec![egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::NONE }],
                    &mut values,
                    custom,
                );
                assert!(response.iter().all(|r| !r.changed()));
            }
        }
    }

    #[test]
    fn checkbox_drag_rejects_batched_blank_press_before_motion() {
        for appearance in ["native", "custom", "sparse"] {
            for target in [true, false] {
                for batched in [false, true] {
                    let ctx = egui::Context::default();
                    let original = [Some(!target), Some(target), Some(!target)];
                    let mut values = original;
                    release_checkbox_row(&ctx, vec![], &mut values, appearance);
                    let rows = release_checkbox_row(&ctx, vec![], &mut values, appearance);
                    let points: Vec<_> = rows.iter().map(|r| r.rect.center()).collect();
                    let outside = pos2(380.0, 180.0);
                    assert!(rows.iter().all(|r| !r.rect.contains(outside)));
                    let mut frames = vec![vec![egui::Event::PointerMoved(outside), primary(outside, true)]];
                    if batched {
                        // One RawInput: the final hover must not become the press origin.
                        frames[0].push(egui::Event::PointerMoved(points[1]));
                    } else {
                        frames.push(vec![egui::Event::PointerMoved(points[1])]);
                    }
                    frames.extend([
                        vec![], // dwell while held
                        vec![egui::Event::PointerMoved(points[0])],
                        vec![egui::Event::PointerMoved(points[2]), primary(points[2], false)],
                        vec![egui::Event::PointerMoved(points[1])],
                        vec![],
                    ]);
                    let mut changes = [0; 3];
                    for events in frames {
                        for (i, response) in release_checkbox_row(&ctx, events, &mut values, appearance).iter().enumerate() {
                            changes[i] += usize::from(response.changed());
                        }
                        assert_eq!(values, original, "{appearance}, target={target}, batched={batched}");
                        assert_eq!(changes, [0, 0, 0], "an ineligible press must not dispatch model changes");
                    }
                    // A new eligible press uses its own value, opposite to the spurious
                    // target that the earlier blank -> second-row batch could have armed.
                    // Motion inside that same checkbox remains a valid press batch.
                    for events in [
                        vec![egui::Event::PointerMoved(points[0]), primary(points[0], true), egui::Event::PointerMoved(points[0] + vec2(1.0, 0.0))],
                        vec![],
                        vec![egui::Event::PointerMoved(points[1])],
                        vec![egui::Event::PointerMoved(points[2])],
                        vec![egui::Event::PointerMoved(points[1])],
                        vec![primary(points[1], false)],
                        vec![],
                    ] {
                        for (i, response) in release_checkbox_row(&ctx, events, &mut values, appearance).iter().enumerate() {
                            changes[i] += usize::from(response.changed());
                        }
                    }
                    assert_eq!(values, [Some(target); 3], "{appearance}, target={target}, batched={batched}");
                    assert_eq!(changes, [1, 0, 1], "already-equal and revisited rows must not dispatch duplicate changes");
                }
            }
        }
    }

    #[test]
    fn checkbox_drag_rejects_batched_mixed_disabled_and_clipped_starts() {
        for value in [false, true] {
            for start in [1, 2, 3] {
                for batched in [false, true] {
                    let ctx = egui::Context::default();
                    let original = [Some(value), None, Some(value), Some(value), Some(value)];
                    let mut values = original;
                    guarded_checkbox_row(&ctx, vec![], &mut values);
                    let rows = guarded_checkbox_row(&ctx, vec![], &mut values);
                    let from = rows[start].rect.center();
                    let to = rows[4].rect.center();
                    let mut frames = vec![vec![egui::Event::PointerMoved(from), primary(from, true)]];
                    if batched {
                        frames[0].push(egui::Event::PointerMoved(to));
                    } else {
                        frames.push(vec![egui::Event::PointerMoved(to)]);
                    }
                    frames.extend([vec![], vec![primary(to, false)], vec![egui::Event::PointerMoved(rows[0].rect.center())], vec![]]);
                    let mut changes = [0; 5];
                    for events in frames {
                        for (i, response) in guarded_checkbox_row(&ctx, events, &mut values).iter().enumerate() {
                            changes[i] += usize::from(response.changed());
                        }
                        assert_eq!(values, original, "start={start}, value={value}, batched={batched}");
                        assert_eq!(changes, [0; 5], "a later eligible hover must not authorize the earlier press");
                    }
                }
            }
        }
    }

    #[test]
    fn checkbox_drag_batched_valid_press_uses_origin_value_in_both_directions() {
        for appearance in ["native", "custom", "sparse"] {
            for reverse in [false, true] {
                for target in [true, false] {
                    for neighbor_equal in [false, true] {
                        let ctx = egui::Context::default();
                        let (origin, neighbor) = if reverse { (1, 0) } else { (0, 1) };
                        let mut values = [Some(!target); 3];
                        if neighbor_equal {
                            values[neighbor] = Some(target);
                        }
                        release_checkbox_row(&ctx, vec![], &mut values, appearance);
                        let rows = release_checkbox_row(&ctx, vec![], &mut values, appearance);
                        let p = rows[origin].rect.center();
                        let q = rows[neighbor].rect.center();
                        let mut changes = [0; 3];
                        for events in [
                            vec![egui::Event::PointerMoved(p), primary(p, true), egui::Event::PointerMoved(q)],
                            vec![], // an earlier-drawn neighbor gets the next held pass
                            vec![],
                            vec![primary(q, false)],
                            vec![egui::Event::PointerMoved(rows[2].rect.center())],
                            vec![],
                        ] {
                            for (i, response) in release_checkbox_row(&ctx, events, &mut values, appearance).iter().enumerate() {
                                changes[i] += usize::from(response.changed());
                            }
                        }
                        assert_eq!(values, [Some(target), Some(target), Some(!target)], "{appearance}, reverse={reverse}, equal={neighbor_equal}");
                        let mut expected = [0; 3];
                        expected[origin] = 1;
                        expected[neighbor] = usize::from(!neighbor_equal);
                        assert_eq!(changes, expected, "the immutable assignment comes from the physical press origin");
                    }
                }
            }
        }
    }

    fn short_sparse_checkbox_grid(ctx: &egui::Context, events: Vec<egui::Event>, values: &mut [Option<bool>; 3]) -> Vec<Response> {
        let mut responses = Vec::new();
        run(ctx, events, egui::Modifiers::NONE, &mut |ui| {
            responses.clear();
            egui::Grid::new("short-checkbox-press").num_columns(1).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
                for (value, label) in values.iter_mut().zip(["First", "Second", "Untouched"]) {
                    responses.push(sparse_checkbox(ui, value, label));
                    ui.end_row();
                }
            });
        });
        responses
    }

    #[test]
    fn checkbox_sparse_batched_short_press_and_escape_preserve_assignment() {
        for reverse in [false, true] {
            for cancel in [false, true] {
                let ctx = egui::Context::default();
                let mut values = [Some(false); 3];
                for _ in 0..3 {
                    short_sparse_checkbox_grid(&ctx, vec![], &mut values);
                }
                let rows = short_sparse_checkbox_grid(&ctx, vec![], &mut values);
                let (origin, neighbor) = if reverse { (1, 0) } else { (0, 1) };
                let from = rows[origin].interact_rect;
                let to = rows[neighbor].interact_rect;
                let x = from.left().max(to.left()) + 1.0;
                let (p, q) = if reverse {
                    (pos2(x, from.top() + 0.25), pos2(x, to.bottom() - 0.25))
                } else {
                    (pos2(x, from.bottom() - 0.25), pos2(x, to.top() + 0.25))
                };
                assert!(from.contains(p) && to.contains(q) && !from.contains(q));
                assert!(p.distance(q) > 0.0 && p.distance(q) < 6.0, "actual short grid edge: {p:?} -> {q:?}");
                let mut changes = [0; 3];
                for events in [vec![egui::Event::PointerMoved(p), primary(p, true), egui::Event::PointerMoved(q)], vec![], vec![]] {
                    for (i, response) in short_sparse_checkbox_grid(&ctx, events, &mut values).iter().enumerate() {
                        changes[i] += usize::from(response.changed());
                    }
                }
                assert_eq!(values, [Some(true), Some(true), Some(false)]);
                assert_eq!(changes, [1, 1, 0]);
                assert!(!ctx.input(|i| i.pointer.is_decidedly_dragging()), "exercise the below-threshold click candidate");
                if cancel {
                    let responses = short_sparse_checkbox_grid(
                        &ctx,
                        vec![egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                        &mut values,
                    );
                    assert!(responses.iter().all(|r| !r.changed()));
                }
                let released = short_sparse_checkbox_grid(&ctx, vec![primary(q, false)], &mut values);
                assert!(released[neighbor].clicked_by(egui::PointerButton::Primary), "egui retains the displaced native click candidate");
                assert!(released.iter().all(|r| !r.changed()), "the residual click must not run another sparse cycle");
                short_sparse_checkbox_grid(&ctx, vec![egui::Event::PointerMoved(rows[2].rect.center())], &mut values);
                assert_eq!(values, [Some(true), Some(true), Some(false)], "reverse={reverse}, cancel={cancel}");
            }
        }
        let ctx = egui::Context::default();
        let mut values = [Some(false); 3];
        short_sparse_checkbox_grid(&ctx, vec![], &mut values);
        let rows = short_sparse_checkbox_grid(&ctx, vec![], &mut values);
        let p = rows[0].rect.center();
        short_sparse_checkbox_grid(&ctx, vec![egui::Event::PointerMoved(p), primary(p, true)], &mut values);
        assert_eq!(values, [Some(false); 3]);
        let released = short_sparse_checkbox_grid(&ctx, vec![primary(p, false)], &mut values);
        assert_eq!(values, [None, Some(false), Some(false)], "a genuine same-origin click still cycles false to unset");
        assert!(released[0].changed() && released[1..].iter().all(|r| !r.changed()));
    }

    fn positioned_checkbox_row(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        values: &mut [Option<bool>; 3],
        appearance: &str,
        top: f32,
        cover: Option<Rect>,
    ) -> Vec<Response> {
        let mut responses = Vec::new();
        run(ctx, events, egui::Modifiers::NONE, &mut |ui| {
            responses.clear();
            if let Some(rect) = cover {
                egui::Area::new(egui::Id::new("checkbox-press-cover")).order(egui::Order::Foreground).fixed_pos(rect.min).movable(false).show(
                    ui.ctx(),
                    |ui| {
                        let _ = ui.allocate_exact_size(rect.size(), Sense::click());
                    },
                );
            }
            ui.scope_builder(
                egui::UiBuilder::new().id_salt("positioned-checkboxes").max_rect(Rect::from_min_size(pos2(20.0, top), vec2(300.0, 90.0))),
                |ui| {
                    for (value, label) in values.iter_mut().zip(["First", "Second", "Untouched"]) {
                        responses.push(if appearance == "sparse" {
                            sparse_checkbox(ui, value, label)
                        } else {
                            let mut on = value.unwrap();
                            let response = if appearance == "custom" { checkbox(ui, &mut on, label) } else { native_checkbox(ui, &mut on, label) };
                            *value = Some(on);
                            response
                        });
                    }
                },
            );
        });
        responses
    }

    #[test]
    fn checkbox_drag_uses_current_allocated_geometry_after_rows_move() {
        for appearance in ["native", "custom", "sparse"] {
            for value in [false, true] {
                for valid_start in [false, true] {
                    let ctx = egui::Context::default();
                    let mut values = [Some(value); 3];
                    positioned_checkbox_row(&ctx, vec![], &mut values, appearance, 20.0, None);
                    let old = positioned_checkbox_row(&ctx, vec![], &mut values, appearance, 20.0, None);
                    // Measure the moved layout separately; leave this context's previous
                    // widget geometry at the original placement until the press frame.
                    let probe = egui::Context::default();
                    let mut probe_values = values;
                    positioned_checkbox_row(&probe, vec![], &mut probe_values, appearance, 100.0, None);
                    let moved = positioned_checkbox_row(&probe, vec![], &mut probe_values, appearance, 100.0, None);
                    let p = if valid_start { moved[0].rect.center() } else { old[0].rect.center() };
                    let q = moved[1].rect.center();
                    let first_motion = if valid_start { q } else { p + vec2(1.0, 0.0) };
                    assert!(moved.iter().all(|r| !r.rect.contains(old[0].rect.center())));
                    let mut changes = [0; 3];
                    for events in [
                        vec![egui::Event::PointerMoved(p), primary(p, true), egui::Event::PointerMoved(first_motion)],
                        vec![egui::Event::PointerMoved(q)],
                        vec![],
                        vec![primary(q, false)],
                        vec![egui::Event::PointerMoved(moved[2].rect.center())],
                        vec![],
                    ] {
                        let rows = positioned_checkbox_row(&ctx, events, &mut values, appearance, 100.0, None);
                        for (i, response) in rows.iter().enumerate() {
                            assert_eq!(response.rect, moved[i].rect);
                            changes[i] += usize::from(response.changed());
                        }
                    }
                    let changed = if valid_start { !value } else { value };
                    assert_eq!(values, [Some(changed), Some(changed), Some(value)], "{appearance}, valid={valid_start}");
                    assert_eq!(changes, if valid_start { [1, 1, 0] } else { [0, 0, 0] });
                }
            }
        }
    }

    #[test]
    fn checkbox_drag_batched_press_under_cover_does_not_arm_uncovered_target() {
        for appearance in ["native", "custom", "sparse"] {
            for value in [false, true] {
                let ctx = egui::Context::default();
                let mut values = [Some(value); 3];
                positioned_checkbox_row(&ctx, vec![], &mut values, appearance, 20.0, None);
                let rows = positioned_checkbox_row(&ctx, vec![], &mut values, appearance, 20.0, None);
                let cover = rows[0].rect;
                for _ in 0..2 {
                    positioned_checkbox_row(&ctx, vec![], &mut values, appearance, 20.0, Some(cover));
                }
                let p = rows[0].rect.center();
                let q = rows[1].rect.center();
                assert_ne!(ctx.layer_id_at(p), Some(rows[0].layer_id), "the actual press is covered");
                assert_eq!(ctx.layer_id_at(q), Some(rows[1].layer_id), "the final checkbox remains eligible");
                let mut changes = [0; 3];
                for events in [
                    vec![egui::Event::PointerMoved(p), primary(p, true), egui::Event::PointerMoved(q)],
                    vec![],
                    vec![primary(q, false)],
                    vec![egui::Event::PointerMoved(rows[2].rect.center())],
                    vec![],
                ] {
                    for (i, response) in positioned_checkbox_row(&ctx, events, &mut values, appearance, 20.0, Some(cover)).iter().enumerate() {
                        changes[i] += usize::from(response.changed());
                    }
                    assert_eq!(values, [Some(value); 3]);
                    assert_eq!(changes, [0; 3]);
                }
            }
        }
    }

    #[test]
    fn checkbox_sparse_short_mixed_origin_rejects_displaced_click_and_keeps_genuine_clicks() {
        for reverse in [false, true] {
            for same_frame in [false, true] {
                for neighbor_value in [false, true] {
                    let ctx = egui::Context::default();
                    let (origin, neighbor) = if reverse { (1, 0) } else { (0, 1) };
                    let mut values = [Some(false), Some(false), Some(!neighbor_value)];
                    values[origin] = None;
                    values[neighbor] = Some(neighbor_value);
                    let original = values;
                    for _ in 0..3 {
                        short_sparse_checkbox_grid(&ctx, vec![], &mut values);
                    }
                    let rows = short_sparse_checkbox_grid(&ctx, vec![], &mut values);
                    let from = rows[origin].interact_rect;
                    let to = rows[neighbor].interact_rect;
                    let x = from.left().max(to.left()) + 1.0;
                    let (p, q) = if reverse {
                        (pos2(x, from.top() + 0.25), pos2(x, to.bottom() - 0.25))
                    } else {
                        (pos2(x, from.bottom() - 0.25), pos2(x, to.top() + 0.25))
                    };
                    assert!(from.contains(p) && to.contains(q) && !from.contains(q));
                    assert!(p.distance(q) > 0.0 && p.distance(q) < 6.0, "actual short grid edge: {p:?} -> {q:?}");
                    let mut events = vec![egui::Event::PointerMoved(p), primary(p, true), egui::Event::PointerMoved(q)];
                    if same_frame {
                        events.push(primary(q, false));
                    } else {
                        let pressed = short_sparse_checkbox_grid(&ctx, events, &mut values);
                        assert_eq!(values, original);
                        assert!(pressed.iter().all(|r| !r.changed()));
                        assert!(!ctx.input(|i| i.pointer.is_decidedly_dragging()));
                        events = vec![primary(q, false)];
                    }
                    let released = short_sparse_checkbox_grid(&ctx, events, &mut values);
                    assert!(released[neighbor].clicked_by(egui::PointerButton::Primary), "exercise egui's displaced primary-click fallback");
                    assert_eq!(values, original, "reverse={reverse}, same_frame={same_frame}, neighbor={neighbor_value}");
                    assert!(released.iter().all(|r| !r.changed()), "an unset origin must not cycle its concrete neighbor");
                    for events in [vec![egui::Event::PointerMoved(rows[2].rect.center())], vec![]] {
                        let responses = short_sparse_checkbox_grid(&ctx, events, &mut values);
                        assert_eq!(values, original);
                        assert!(responses.iter().all(|r| !r.changed()));
                    }
                }
            }
        }
        // Origin validation must retain all three ordinary sparse click steps,
        // whether press/release share a RawInput or span held frames.
        for same_frame in [false, true] {
            for origin in 0..3 {
                let ctx = egui::Context::default();
                let mut values = [None, Some(false), Some(true)];
                for _ in 0..3 {
                    short_sparse_checkbox_grid(&ctx, vec![], &mut values);
                }
                let rows = short_sparse_checkbox_grid(&ctx, vec![], &mut values);
                let p = rows[origin].rect.center();
                let original = values;
                let mut expected = original;
                expected[origin] = [Some(true), None, Some(false)][origin];
                let mut events = vec![egui::Event::PointerMoved(p), primary(p, true)];
                if same_frame {
                    events.push(primary(p, false));
                } else {
                    let pressed = short_sparse_checkbox_grid(&ctx, events, &mut values);
                    assert_eq!(values, original);
                    assert!(pressed.iter().all(|r| !r.changed()));
                    for _ in 0..2 {
                        let held = short_sparse_checkbox_grid(&ctx, vec![], &mut values);
                        assert_eq!(values, original);
                        assert!(held.iter().all(|r| !r.changed()));
                    }
                    events = vec![primary(p, false)];
                }
                let released = short_sparse_checkbox_grid(&ctx, events, &mut values);
                assert!(released[origin].clicked_by(egui::PointerButton::Primary));
                assert_eq!(values, expected, "genuine origin={origin}, same_frame={same_frame}");
                for (i, response) in released.iter().enumerate() {
                    assert_eq!(response.changed(), i == origin);
                }
                let settled = short_sparse_checkbox_grid(&ctx, vec![], &mut values);
                assert_eq!(values, expected);
                assert!(settled.iter().all(|r| !r.changed()));
            }
        }
    }

    #[test]
    fn checkbox_completed_batch_assigns_only_endpoints_after_one_repaint() {
        for appearance in ["native", "custom", "sparse"] {
            for reverse in [false, true] {
                for target in [false, true] {
                    for neighbor_equal in [false, true] {
                        for later_hover in [false, true] {
                            let ctx = egui::Context::default();
                            let draw = |events, values: &mut [Option<bool>; 3]| {
                                if appearance == "sparse" {
                                    short_sparse_checkbox_grid(&ctx, events, values)
                                } else {
                                    release_checkbox_row(&ctx, events, values, appearance)
                                }
                            };
                            let (origin, neighbor) = if reverse { (1, 0) } else { (0, 1) };
                            let mut values = [Some(!target); 3];
                            if neighbor_equal {
                                values[neighbor] = Some(target);
                            }
                            for _ in 0..3 {
                                draw(vec![], &mut values);
                            }
                            let rows = draw(vec![], &mut values);
                            let from = rows[origin].interact_rect;
                            let to = rows[neighbor].interact_rect;
                            let x = from.left().max(to.left()) + 1.0;
                            let (p, q) = if reverse {
                                (pos2(x, from.top() + 0.25), pos2(x, to.bottom() - 0.25))
                            } else {
                                (pos2(x, from.bottom() - 0.25), pos2(x, to.top() + 0.25))
                            };
                            assert!(from.contains(p) && to.contains(q) && !from.contains(q));
                            if appearance == "sparse" {
                                assert!((p.distance(q) - 4.5).abs() < 0.001, "actual Grid boundary: {p:?} -> {q:?}");
                            }
                            let mut events = vec![egui::Event::PointerMoved(p), primary(p, true), egui::Event::PointerMoved(q), primary(q, false)];
                            if later_hover {
                                events.push(egui::Event::PointerMoved(rows[2].rect.center()));
                            }
                            let before = values;
                            let released = draw(events, &mut values);
                            let mut changes = [0; 3];
                            for (i, response) in released.iter().enumerate() {
                                assert_eq!(response.changed(), values[i] != before[i]);
                                changes[i] += usize::from(response.changed());
                            }
                            assert_eq!(values[origin], Some(target), "the current physical origin supplies the target");
                            assert_eq!(values[2], Some(!target));
                            assert!(values.iter().all(Option::is_some), "a short sweep must never cycle false to unset");
                            if !reverse || neighbor_equal {
                                assert_eq!(values[neighbor], Some(target));
                            }
                            // An earlier-drawn endpoint can finish on exactly one requested
                            // repaint. Already-equal endpoints must never invert or emit changed.
                            for (i, response) in draw(vec![], &mut values).iter().enumerate() {
                                changes[i] += usize::from(response.changed());
                            }
                            let mut expected_changes = [0; 3];
                            expected_changes[origin] = 1;
                            expected_changes[neighbor] = usize::from(!neighbor_equal);
                            assert_eq!(values, [Some(target), Some(target), Some(!target)], "{appearance}, reverse={reverse}, target={target}");
                            assert_eq!(changes, expected_changes);
                            for events in
                                [vec![egui::Event::PointerMoved(rows[2].rect.center())], vec![], vec![primary(rows[2].rect.center(), false)]]
                            {
                                assert!(draw(events, &mut values).iter().all(|r| !r.changed()));
                                assert_eq!(values, [Some(target), Some(target), Some(!target)]);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn checkbox_completed_batch_retains_guards_and_cannot_replay_stale_endpoint() {
        for target in [false, true] {
            for guarded in [1, 2, 3] {
                for guarded_origin in [false, true] {
                    let ctx = egui::Context::default();
                    let mut values = [Some(!target), None, Some(!target), Some(!target), Some(!target)];
                    for _ in 0..3 {
                        guarded_checkbox_row(&ctx, vec![], &mut values);
                    }
                    let rows = guarded_checkbox_row(&ctx, vec![], &mut values);
                    let (origin, endpoint) = if guarded_origin { (guarded, 0) } else { (4, guarded) };
                    let p = rows[origin].rect.center();
                    let q = rows[endpoint].rect.center();
                    let mut expected = values;
                    let mut expected_changes = [0; 5];
                    if !guarded_origin {
                        expected[origin] = Some(target);
                        expected_changes[origin] = 1;
                    }
                    let mut changes = [0; 5];
                    for events in [
                        vec![egui::Event::PointerMoved(p), primary(p, true), egui::Event::PointerMoved(q), primary(q, false)],
                        vec![],
                        vec![egui::Event::PointerMoved(rows[0].rect.center())],
                        vec![],
                    ] {
                        for (i, response) in guarded_checkbox_row(&ctx, events, &mut values).iter().enumerate() {
                            changes[i] += usize::from(response.changed());
                        }
                        assert_eq!(values, expected, "guard={guarded}, origin={guarded_origin}");
                    }
                    assert_eq!(changes, expected_changes);
                }
            }
            for appearance in ["native", "custom", "sparse"] {
                for invalidation in ["escape", "blank_press", "moved", "missing"] {
                    let ctx = egui::Context::default();
                    let mut values = [Some(!target); 3];
                    for _ in 0..3 {
                        positioned_checkbox_row(&ctx, vec![], &mut values, appearance, 20.0, None);
                    }
                    let rows = positioned_checkbox_row(&ctx, vec![], &mut values, appearance, 20.0, None);
                    let p = rows[1].rect.center();
                    let q = rows[0].rect.center();
                    let released = positioned_checkbox_row(
                        &ctx,
                        vec![egui::Event::PointerMoved(p), primary(p, true), egui::Event::PointerMoved(q), primary(q, false)],
                        &mut values,
                        appearance,
                        20.0,
                        None,
                    );
                    let expected = [Some(!target), Some(target), Some(!target)];
                    assert_eq!(values, expected, "reverse draw order records the endpoint instead of guessing its target");
                    assert!(!released[0].changed() && released[1].changed() && !released[2].changed());
                    let outside = pos2(380.0, 180.0);
                    let mut top = 20.0;
                    let events = match invalidation {
                        "escape" => vec![egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                        "blank_press" => {
                            vec![egui::Event::PointerMoved(outside), primary(outside, true), egui::Event::PointerMoved(rows[2].rect.center())]
                        }
                        "moved" => {
                            top = 100.0;
                            vec![]
                        }
                        _ => {
                            assert_eq!(invalidation, "missing");
                            run(&ctx, vec![], egui::Modifiers::NONE, &mut |_| {});
                            vec![]
                        }
                    };
                    let after = positioned_checkbox_row(&ctx, events, &mut values, appearance, top, None);
                    if invalidation == "moved" {
                        assert!(!after[0].interact_rect.contains(q));
                    }
                    assert!(after.iter().all(|r| !r.changed()));
                    assert_eq!(values, expected, "{appearance}, {invalidation}");
                    for events in [vec![primary(outside, false)], vec![egui::Event::PointerMoved(rows[2].rect.center())], vec![]] {
                        assert!(positioned_checkbox_row(&ctx, events, &mut values, appearance, 20.0, None).iter().all(|r| !r.changed()));
                        assert_eq!(values, expected, "the completed receipt cannot authorize a later hover or a new blank press");
                    }
                }
            }
        }
    }
}

/// `img` made to fit the GPU's largest texture (`max_side` pixels a side), sampled down
/// (nearest pixel) when it is larger. A texture past the limit panics in the renderer, so every
/// UI texture goes through this: an oversized image loses resolution, not the document.
pub fn fit_texture(img: egui::ColorImage, max_side: usize) -> egui::ColorImage {
    let [w, h] = img.size;
    let max_side = max_side.max(1);
    if w <= max_side && h <= max_side {
        return img;
    }
    let k = (max_side as f64 / w.max(h) as f64).min(1.0);
    let (nw, nh) = (((w as f64 * k).floor() as usize).clamp(1, max_side), ((h as f64 * k).floor() as usize).clamp(1, max_side));
    let mut pixels = Vec::with_capacity(nw * nh);
    for y in 0..nh {
        let sy = (y * h / nh).min(h.saturating_sub(1));
        for x in 0..nw {
            let sx = (x * w / nw).min(w.saturating_sub(1));
            pixels.push(img.pixels.get(sy * w + sx).copied().unwrap_or(Color32::TRANSPARENT));
        }
    }
    egui::ColorImage::new([nw, nh], pixels)
}

/// [`egui::Context::load_texture`] for an image of any size (see [`fit_texture`]).
pub fn load_texture(ctx: &egui::Context, name: impl Into<String>, img: egui::ColorImage, options: egui::TextureOptions) -> egui::TextureHandle {
    let max = ctx.input(|i| i.max_texture_side);
    ctx.load_texture(name, fit_texture(img, max), options)
}

#[cfg(test)]
mod texture_tests {
    /// The glyph grid that crashed the renderer: 7 columns of 1200 characters at 60 px, 10320 px
    /// tall against an 8192 px limit.
    #[test]
    fn oversized_images_fit_the_texture_limit() {
        let img = egui::ColorImage::new([420, 10320], vec![egui::Color32::WHITE; 420 * 10320]);
        let out = super::fit_texture(img, 8192);
        assert!(out.size[0] <= 8192 && out.size[1] == 8192, "{:?}", out.size);
        assert_eq!(out.pixels.len(), out.size[0] * out.size[1]);
        let small = egui::ColorImage::new([10, 10], vec![egui::Color32::RED; 100]);
        assert_eq!(super::fit_texture(small, 8192).size, [10, 10]);
        let wide = egui::ColorImage::new([20000, 3], vec![egui::Color32::RED; 60000]);
        let out = super::fit_texture(wide, 8192);
        assert!(out.size[0] == 8192 && out.size[1] >= 1);
    }
}
