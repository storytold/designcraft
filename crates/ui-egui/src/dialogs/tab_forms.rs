//! The tabs page of Paragraph Style Options on the craft framework: the alignment buttons, the stop's position, leader
//! and align-on character, Clear All, and the tab ruler as an app-defined framework component.
//!
//! The ruler keeps the painting and the pointer rules of the native ruler (`panels::tabs::ruler`): a click on the strip
//! adds a stop or selects one, a drag moves a stop (off the ruler it deletes it), and the triangles are the first-line,
//! left and right indents. It has no keyboard handling, as the native ruler has none.

use crate::DesignApp;
use crate::dialogs::Dialog;
use crate::panels::tabs::{RulerEdit, tab_icon};
use craft_behavior::{Metrics, PressPhase, Sense};
use craft_ui_widgets::buttons::ToggleButton;
use craft_ui_widgets::contract::{
    Bind, Common, Component, Cx, Input, MeasureCx, NoValue, Outcome, PaintUi, PartsCx, Pointer as _, Props as _, Remember as _, Role, Show as _,
    ShowContainer as _, Sizing,
};
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::primitives::IconSpec;
use craft_ui_widgets::runtime::env::Label;
use craft_ui_widgets::runtime::focus::KeyUse;
use craft_ui_widgets::style::{ComponentStyle, StyleCx};
use designcraft_doc::{TabAlign, TabStop};
use designcraft_engine::cmd::tabs::MAX_POSITION;
use designcraft_geom::Unit;
use egui::{Color32, Pos2, Rect, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};

/// Ruler height, the tab strip at its top and the inset of position 0 from its left edge.
const HEIGHT: f32 = 40.0;
const STRIP: f32 = 15.0;
const PAD: f32 = 10.0;
/// How far off the ruler a dragged stop has to go to be deleted.
const OFF: f32 = 24.0;

const AREA: craft_behavior::PartKey = craft_behavior::PartKey::new("ruler");

/// The look of the ruler: no surface of its own, the painting is the ruler's.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TabRulerStyle;

impl ComponentStyle for TabRulerStyle {
    fn metrics(&self) -> Metrics {
        Metrics::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Tab(usize),
    New,
    Indent(&'static str),
}

/// A drag in progress: what, where (points) and whether it is off the ruler.
#[derive(Clone, Copy, Debug)]
struct Drag {
    grab: Grab,
    pos: f64,
    off: bool,
}

#[derive(Clone, Default)]
struct Held {
    drag: Option<Drag>,
}

/// What a ruler reports: its response and the edit the pointer made in this pass.
#[derive(Debug)]
pub(crate) struct RulerOutput {
    pub(crate) response: egui::Response,
    pub(crate) edit: Option<RulerEdit>,
}

impl Outcome for RulerOutput {
    type Value = NoValue;

    fn response(&self) -> &egui::Response {
        &self.response
    }
}

/// The tab ruler: the stops of a paragraph over a scale as wide as the first page's column.
#[derive(craft_ui_widgets::contract::Component, craft_ui_widgets::contract::Props)]
#[component(content_painter)]
pub(crate) struct TabRuler {
    #[common]
    common: Common,
    tabs: Vec<TabStop>,
    left: f64,
    first: f64,
    right: f64,
    width: f64,
    unit: Unit,
    selected: Option<usize>,
    edit: Option<RulerEdit>,
    drag: Option<Drag>,
}

impl TabRuler {
    /// A ruler named `label` over `tabs` and the three indents, `width` points wide, its scale in `unit`.
    pub(crate) fn new(label: impl Into<Label>, tabs: Vec<TabStop>, indents: (f64, f64, f64), width: f64, unit: Unit, selected: Option<usize>) -> Self {
        TabRuler { common: Common::new(), tabs, left: indents.0, first: indents.1, right: indents.2, width, unit, selected, edit: None, drag: None }.label(label)
    }

    fn width_pt(&self) -> f64 {
        if self.width.is_finite() && self.width > 1.0 { self.width.min(MAX_POSITION) } else { 468.0 }
    }

    fn geometry(&self, rect: Rect) -> Geometry {
        let width_pt = self.width_pt();
        let scale = ((rect.width() - 2.0 * PAD) / width_pt as f32).max(0.01);
        let strip = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + STRIP));
        let bar = Rect::from_min_max(pos2(strip.min.x, strip.max.y), rect.max);
        Geometry { rect, x0: rect.min.x + PAD, scale, width_pt, strip, bar }
    }

    /// What the pointer at `p` takes hold of.
    fn hit(&self, g: &Geometry, p: Pos2) -> Option<Grab> {
        if g.bar.contains(p) {
            let near = |pt: f64| (p.x - g.to_x(pt)).abs() <= 6.0;
            if p.y <= g.bar.min.y + 10.0 && near(self.left + self.first) {
                return Some(Grab::Indent("firstLineIndent"));
            }
            if p.y >= g.bar.max.y - 10.0 {
                if near(self.left) {
                    return Some(Grab::Indent("leftIndent"));
                }
                if near(g.width_pt - self.right) {
                    return Some(Grab::Indent("rightIndent"));
                }
            }
            return None;
        }
        if !g.strip.expand2(vec2(0.0, 2.0)).contains(p) {
            return None;
        }
        let nearest = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, s)| (i, (g.to_x(s.position) - p.x).abs()))
            .filter(|(_, d)| *d <= 6.0)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        Some(nearest.map_or(Grab::New, |(i, _)| Grab::Tab(i)))
    }
}

/// Where the ruler's parts are on screen.
struct Geometry {
    rect: Rect,
    x0: f32,
    scale: f32,
    width_pt: f64,
    strip: Rect,
    bar: Rect,
}

impl Geometry {
    fn to_x(&self, pt: f64) -> f32 {
        self.x0 + pt as f32 * self.scale
    }

    fn to_pt(&self, x: f32) -> f64 {
        (((x - self.x0) / self.scale) as f64 * 2.0).round() / 2.0
    }
}

impl Component for TabRuler {
    const ROLE: Role = Role::GROUP;
    const OVERLAYS: bool = false;
    const KEYS: KeyUse = KeyUse::of(Input::Pointer);
    type Style = TabRulerStyle;
    type Output = RulerOutput;

    fn style(_: &StyleCx<'_>, _: TabRulerStyleKey) -> TabRulerStyle {
        TabRulerStyle
    }

    fn measure(&mut self, _: &MeasureCx<'_, Self>) -> Vec2 {
        vec2(120.0, HEIGHT)
    }

    fn sense(&self) -> Sense {
        Sense::HOVER
    }

    fn parts(&mut self, rect: Rect, cx: &mut PartsCx<'_, Self>) {
        cx.part(AREA, rect).sense(Sense::CLICK | Sense::DRAG);
    }

    /// The pointer's edits: a press that moves takes a stop or an indent and drags it; one that does not is a click that
    /// selects the stop under it or adds one on the strip.
    fn update(&mut self, cx: &mut Cx<'_, Self>) {
        let g = self.geometry(cx.rect);
        let mut held = cx.memory::<Held>().clone();
        match cx.part(AREA).press.phase {
            PressPhase::DragStart { start, pos, .. } => {
                if let Some(grab) = self.hit(&g, start.origin) {
                    held.drag = Some(Drag { grab, pos: g.to_pt(start.origin.x).clamp(0.0, MAX_POSITION), off: false });
                    if let Grab::Tab(i) = grab {
                        self.edit = Some(RulerEdit::Select(i));
                    }
                }
                follow(&mut held, &g, pos);
            }
            PressPhase::Drag { pos, .. } => follow(&mut held, &g, pos),
            PressPhase::Release { start, click, .. } => {
                if let Some(d) = held.drag.take() {
                    self.edit = Some(match d.grab {
                        Grab::Tab(i) if d.off => RulerEdit::Remove(i),
                        Grab::Tab(i) => RulerEdit::Move(i, d.pos),
                        Grab::New => RulerEdit::Add(d.pos),
                        Grab::Indent("firstLineIndent") => RulerEdit::Indent("firstLineIndent", d.pos - self.left),
                        Grab::Indent("rightIndent") => RulerEdit::Indent("rightIndent", (g.width_pt - d.pos).max(0.0)),
                        Grab::Indent(k) => RulerEdit::Indent(k, d.pos),
                    });
                } else if click {
                    match self.hit(&g, start.origin) {
                        Some(Grab::Tab(i)) => self.edit = Some(RulerEdit::Select(i)),
                        Some(Grab::New) => self.edit = Some(RulerEdit::Add(g.to_pt(start.origin.x).clamp(0.0, g.width_pt.max(0.0)))),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        self.drag = held.drag;
        *cx.memory::<Held>() = held;
    }

    fn paint(&mut self, ui: &mut PaintUi<'_>, cx: &Cx<'_, Self>) {
        let g = self.geometry(cx.rect);
        let content = ui.content_painter(cx);
        paint_ruler(&content.painter, self, &g, self.drag);
    }

    fn finish(self, cx: Cx<'_, Self>) -> RulerOutput {
        RulerOutput { response: cx.response.clone(), edit: self.edit }
    }
}

/// A drag's position and whether it has left the ruler, at the pointer `pos`.
fn follow(held: &mut Held, g: &Geometry, pos: Pos2) {
    if let Some(d) = held.drag.as_mut() {
        d.pos = g.to_pt(pos.x).clamp(0.0, g.width_pt.max(0.0));
        d.off = matches!(d.grab, Grab::Tab(_)) && (pos.y < g.rect.min.y - OFF || pos.y > g.rect.max.y + OFF);
    }
}

/// The strip, the scale with its ticks in the document's unit, the indent markers and the stops.
fn paint_ruler(painter: &egui::Painter, r: &TabRuler, g: &Geometry, drag: Option<Drag>) {
    let t = crate::theme::Tokens::get(painter.ctx());
    let (rect, strip, bar) = (g.rect, g.strip, g.bar);
    let p = painter;
    p.rect_filled(strip, 0.0, t.panel_darker);
    p.rect_filled(bar, 0.0, t.ruler);
    let end = g.to_x(g.width_pt);
    if end < rect.max.x {
        p.rect_filled(Rect::from_min_max(pos2(end, rect.min.y), rect.max), 0.0, t.input.gamma_multiply(0.7));
    }
    p.line_segment([pos2(g.x0, strip.min.y), pos2(g.x0, rect.max.y)], Stroke::new(1.0, t.ruler_tick));
    p.line_segment([pos2(end, strip.min.y), pos2(end, rect.max.y)], Stroke::new(1.0, t.ruler_tick));
    let (major, sub) = r.unit.ruler_ticks(g.scale as f64);
    let font = egui::FontId::proportional(9.0);
    let tick = Stroke::new(1.0, t.ruler_tick);
    let mut k = 0u32;
    while major > 0.0 && k < 2000 {
        let m = k as f64 * major;
        if g.to_x(m) > rect.max.x {
            break;
        }
        for s in 0..sub.max(1) {
            let x = g.to_x(m + major * s as f64 / sub.max(1) as f64);
            if x > rect.max.x {
                break;
            }
            let len = if s == 0 {
                bar.height() - 4.0
            } else if sub % 2 == 0 && s == sub / 2 {
                7.0
            } else {
                4.0
            };
            p.line_segment([pos2(x, bar.max.y - len), pos2(x, bar.max.y)], tick);
        }
        p.text(pos2(g.to_x(m) + 2.0, bar.min.y + 1.0), egui::Align2::LEFT_TOP, crate::canvas::fmt_tick(r.unit.from_pt(m)), font.clone(), t.ruler_text);
        k += 1;
    }
    // Indent markers (a dragged one follows the pointer; the first line follows the left indent).
    let (mut left, mut first_abs, mut right_x) = (r.left, r.left + r.first, g.width_pt - r.right);
    if let Some(Drag { grab: Grab::Indent(which), pos, .. }) = drag {
        match which {
            "leftIndent" => {
                first_abs = pos + r.first;
                left = pos;
            }
            "firstLineIndent" => first_abs = pos,
            _ => right_x = pos,
        }
    }
    let ink = t.text_strong;
    let tri = |x: f32, down: bool, c: Color32| {
        let (y0, y1) = if down { (bar.min.y, bar.min.y + 7.0) } else { (bar.max.y, bar.max.y - 7.0) };
        p.add(egui::Shape::convex_polygon(vec![pos2(x - 5.0, y0), pos2(x + 5.0, y0), pos2(x, y1)], c, Stroke::new(1.0, t.panel_darker)));
    };
    tri(g.to_x(first_abs), true, ink);
    tri(g.to_x(left), false, ink);
    tri(g.to_x(right_x), false, ink);
    // Tab stops.
    for (i, s) in r.tabs.iter().enumerate() {
        let dragging = drag.filter(|d| d.grab == Grab::Tab(i));
        if dragging.is_some_and(|d| d.off) {
            continue;
        }
        let x = g.to_x(dragging.map_or(s.position, |d| d.pos));
        let mark = Rect::from_center_size(pos2(x, strip.max.y - 6.0), vec2(14.0, 14.0));
        if r.selected == Some(i) {
            p.rect_filled(mark.expand(1.0), 2.0, t.accent);
        }
        crate::icons::paint(p, mark, tab_icon(s.align), ink);
    }
    if let Some(d) = drag.filter(|d| matches!(d.grab, Grab::New)) {
        crate::icons::paint(p, Rect::from_center_size(pos2(g.to_x(d.pos), strip.max.y - 6.0), vec2(16.0, 16.0)), "tab-left", t.accent_strong);
    }
    if let Some(d) = drag.filter(|d| !d.off) {
        p.line_segment([pos2(g.to_x(d.pos), strip.min.y), pos2(g.to_x(d.pos), rect.max.y)], Stroke::new(1.0, t.accent));
    }
}

/// A tab alignment as a glyph on a toggle button: the app's own artwork.
struct AlignGlyph(TabAlign);

impl craft_ui_widgets::primitives::Glyph for AlignGlyph {
    fn paint(&self, painter: &egui::Painter, _tokens: &craft_ui_widgets::style::UiTokens, rect: Rect, color: Color32, _mirrored: bool) {
        crate::icons::paint(painter, rect, tab_icon(self.0), color);
    }
}

/// Tabs: the style's stops on a ruler as wide as the first page's column, edited in `p.tabs` (and the indents in
/// `p.leftIndent` and the others) until OK.
pub(crate) fn tabs(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value, doc: &designcraft_doc::Document) {
    use super::style_kit::{committed_text, measure, push};
    use designcraft_engine::cmd::tabs as engine;
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let cur = |d: &Dialog, k: &str| d.fields.get(k).cloned().unwrap_or_else(|| pv[k.trim_start_matches("p.")].clone());
    let mut list: Vec<TabStop> = serde_json::from_value(cur(d, "p.tabs")).unwrap_or_default();
    engine::sort(&mut list);
    let num = |d: &Dialog, k: &str| cur(d, k).as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
    let (left, first, right) = (num(d, "p.leftIndent"), num(d, "p.firstLineIndent"), num(d, "p.rightIndent"));
    let width = doc.page(0).and_then(|p| p.column_rects().first().map(|r| r.width())).filter(|w| *w > 1.0).unwrap_or(468.0);
    let unit = doc.settings.horizontal_units;
    let mut sel = d.fields.get("tabsSelected").and_then(Value::as_u64).map(|i| i as usize).filter(|i| *i < list.len());
    let mut align: TabAlign = d.fields.get("tabsAlign").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let stop = sel.and_then(|i| list.get(i)).cloned();
    // Each control's edit, applied to the list below.
    let mut result: Option<Result<Option<usize>, String>> = None;
    let mut indent: Option<(&str, f64)> = None;
    Stack::new().id_salt("style_tabs").fill_width().show(ui, |ui, _| {
        Row::new().id_salt("style_tabs_controls").fill_width().show(ui, |ui, _| {
            for (a, tip) in crate::panels::tabs::ALIGNS {
                let glyph = AlignGlyph(a);
                let shown = stop.as_ref().map_or(align, |s| s.align) == a;
                let out = ToggleButton::new(tr(tip), Bind::given(shown)).icon(IconSpec::glyph(&glyph)).icon_only().id_salt(("style_tab_align", tab_icon(a))).show(ui);
                if out.events.committed().copied() == Some(true) && !shown {
                    align = a;
                    if let Some(i) = sel {
                        result = Some(engine::change(&mut list, i, Some(a), None, None));
                    }
                }
            }
            if let Some(x) = measure(ui, d, "tabs_x", &tr("X:"), stop.as_ref().map(|s| s.position), unit, true) {
                result = Some(match sel {
                    Some(i) => engine::move_to(&mut list, i, x),
                    None => engine::add(&mut list, TabStop { position: x, align, leader: String::new(), align_on: String::new() }),
                });
            }
            let leader = committed_text(ui, "tabs_leader", &tr("Leader:"), stop.as_ref().map_or("", |s| s.leader.as_str()), stop.is_some());
            if let (Some(l), Some(i)) = (leader, sel) {
                result = Some(engine::leader_value(&l, "tabs").map_err(|e| e.to_string()).and_then(|l| engine::change(&mut list, i, None, Some(l), None)));
            }
            let on_char = stop.as_ref().is_some_and(|s| s.align == TabAlign::Char);
            let align_on = committed_text(ui, "tabs_align_on", &tr("Align On:"), stop.as_ref().map_or("", |s| s.align_on.as_str()), on_char);
            if let (Some(c), Some(i)) = (align_on, sel) {
                result = Some(engine::align_on_value(&c, "tabs").map_err(|e| e.to_string()).and_then(|c| engine::change(&mut list, i, None, None, Some(c))));
            }
            if push(ui, "tabs_clear_all", &tr("Clear All"), true) {
                list.clear();
                result = Some(Ok(None));
            }
        });
        let ruler = TabRuler::new(tr("Tabs"), list.clone(), (left, first, right), width, unit, sel).id_salt("style_tab_ruler").sizing(Sizing::FillWidth).show(ui);
        match ruler.edit {
            Some(RulerEdit::Select(i)) => sel = Some(i),
            Some(RulerEdit::Add(x)) => result = Some(engine::add(&mut list, TabStop { position: x, align, leader: String::new(), align_on: String::new() })),
            Some(RulerEdit::Move(i, x)) => result = Some(engine::move_to(&mut list, i, x)),
            Some(RulerEdit::Remove(i)) => result = Some(engine::remove(&mut list, i)),
            Some(RulerEdit::Indent(k, x)) => indent = Some((k, x)),
            None => {}
        }
    });
    match result {
        Some(Ok(i)) => {
            sel = i;
            d.fields.insert("p.tabs".into(), serde_json::to_value(&list).unwrap_or_default());
            d.fields.remove("status");
        }
        Some(Err(e)) => {
            d.fields.insert("status".into(), json!(e));
        }
        None => {}
    }
    if let Some((k, x)) = indent {
        d.fields.insert(format!("p.{k}"), json!(x));
    }
    d.fields.insert("tabsSelected".into(), sel.map_or(Value::Null, |i| json!(i)));
    d.fields.insert("tabsAlign".into(), serde_json::to_value(align).unwrap_or_default());
}
