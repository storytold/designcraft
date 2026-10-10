//! Tabs panel (Type › Tabs) and the tab ruler it shares with Paragraph Style Options › Tabs.
//!
//! The ruler measures from the left edge of the text column, like composition. Click the strip
//! above the scale to add a stop, drag a stop to move it, drag it off the ruler to delete it; the
//! triangles on the scale are the first-line (top), left and right (bottom) indents.

use designcraft_doc::{TabAlign, TabStop};
use designcraft_engine::cmd::tabs::MAX_POSITION;
use designcraft_geom::{Point, Unit};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::theme::Tokens;

/// Ruler height, the tab strip at its top and the inset of position 0 from its left edge.
const H: f32 = 40.0;
const STRIP: f32 = 15.0;
pub(crate) const PAD: f32 = 10.0;
/// The narrowest the panel gets when it moves over a frame (its controls fit on one row).
const MIN_WIDTH: f32 = 560.0;
/// How far off the ruler a dragged stop has to go to be deleted.
const OFF: f32 = 24.0;

/// The Tabs panel's own state (the stops themselves live in the document).
#[derive(Clone, Debug, Default)]
pub struct PanelState {
    pub selected: Option<usize>,
    /// Alignment for stops added with a click.
    pub align: TabAlign,
    /// The panel sits above the text frame: ruler pixels per point (the view zoom then).
    pub snap: Option<f32>,
    /// Place the panel above the text frame over the next frames (the first one learns its height).
    pub snap_request: u8,
    /// Where the floating window has to go (top-left) and how wide it has to be, once.
    pub place: Option<(Pos2, f32)>,
}

/// What a ruler shows.
pub(crate) struct Ruler<'a> {
    pub tabs: &'a [TabStop],
    pub left: f64,
    pub first: f64,
    pub right: f64,
    /// Column width in points.
    pub width: f64,
    pub unit: Unit,
    pub selected: Option<usize>,
    /// Points to pixels; `None` fits the column into the available width.
    pub scale: Option<f32>,
}

/// An edit made on the ruler.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RulerEdit {
    Select(usize),
    Add(f64),
    Move(usize, f64),
    Remove(usize),
    /// `leftIndent`, `firstLineIndent` or `rightIndent` and its new value.
    Indent(&'static str, f64),
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

pub(crate) fn tab_icon(a: TabAlign) -> &'static str {
    match a {
        TabAlign::Left => "tab-left",
        TabAlign::Center => "tab-center",
        TabAlign::Right => "tab-right",
        TabAlign::Char => "tab-char",
    }
}

/// Tooltip names of the alignments, in button order.
pub(crate) const ALIGNS: [(TabAlign, &str); 4] = [
    (TabAlign::Left, "Left-aligned tab"),
    (TabAlign::Center, "Centered tab"),
    (TabAlign::Right, "Right-aligned tab"),
    (TabAlign::Char, "Tab aligned on a character"),
];

/// Draw the ruler and report the edit the pointer made.
pub(crate) fn ruler(ui: &mut Ui, salt: &str, r: &Ruler) -> Option<RulerEdit> {
    let t = Tokens::get(ui.ctx());
    let width_pt = if r.width.is_finite() && r.width > 1.0 { r.width.min(MAX_POSITION) } else { 468.0 };
    let w = ui.available_width().max(120.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, H), Sense::click_and_drag());
    let scale = r.scale.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(((w - 2.0 * PAD) / width_pt as f32).max(0.01));
    let x0 = rect.min.x + PAD;
    let to_x = |pt: f64| x0 + pt as f32 * scale;
    let to_pt = |x: f32| (((x - x0) / scale) as f64 * 2.0).round() / 2.0;
    let strip = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + STRIP));
    let bar = Rect::from_min_max(pos2(rect.min.x, strip.max.y), rect.max);

    let hit = |p: Pos2| -> Option<Grab> {
        if bar.contains(p) {
            let near = |pt: f64| (p.x - to_x(pt)).abs() <= 6.0;
            if p.y <= bar.min.y + 10.0 && near(r.left + r.first) {
                return Some(Grab::Indent("firstLineIndent"));
            }
            if p.y >= bar.max.y - 10.0 {
                if near(r.left) {
                    return Some(Grab::Indent("leftIndent"));
                }
                if near(width_pt - r.right) {
                    return Some(Grab::Indent("rightIndent"));
                }
            }
            return None;
        }
        if !strip.expand2(vec2(0.0, 2.0)).contains(p) {
            return None;
        }
        let nearest =
            r.tabs.iter().enumerate().map(|(i, s)| (i, (to_x(s.position) - p.x).abs())).filter(|(_, d)| *d <= 6.0).min_by(|a, b| a.1.total_cmp(&b.1));
        Some(nearest.map_or(Grab::New, |(i, _)| Grab::Tab(i)))
    };

    let key = ui.id().with(("tab_ruler", salt));
    let mut drag: Option<Drag> = ui.data(|d| d.get_temp(key));
    let mut edit = None;
    if resp.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
        && let Some(grab) = hit(p)
    {
        drag = Some(Drag { grab, pos: to_pt(p.x).clamp(0.0, MAX_POSITION), off: false });
        if let Grab::Tab(i) = grab {
            edit = Some(RulerEdit::Select(i));
        }
    }
    if resp.dragged()
        && let (Some(d), Some(p)) = (drag.as_mut(), resp.interact_pointer_pos())
    {
        d.pos = to_pt(p.x).clamp(0.0, width_pt.max(0.0));
        d.off = matches!(d.grab, Grab::Tab(_)) && (p.y < rect.min.y - OFF || p.y > rect.max.y + OFF);
    }
    if resp.drag_stopped()
        && let Some(d) = drag.take()
    {
        edit = Some(match d.grab {
            Grab::Tab(i) if d.off => RulerEdit::Remove(i),
            Grab::Tab(i) => RulerEdit::Move(i, d.pos),
            Grab::New => RulerEdit::Add(d.pos),
            Grab::Indent("firstLineIndent") => RulerEdit::Indent("firstLineIndent", d.pos - r.left),
            Grab::Indent("rightIndent") => RulerEdit::Indent("rightIndent", (width_pt - d.pos).max(0.0)),
            Grab::Indent(k) => RulerEdit::Indent(k, d.pos),
        });
    }
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        match hit(p) {
            Some(Grab::Tab(i)) => edit = Some(RulerEdit::Select(i)),
            Some(Grab::New) => edit = Some(RulerEdit::Add(to_pt(p.x).clamp(0.0, width_pt.max(0.0)))),
            _ => {}
        }
    }
    ui.data_mut(|d| match drag {
        Some(v) => {
            d.insert_temp(key, v);
        }
        None => d.remove::<Drag>(key),
    });

    // Paint: the strip, the scale with ticks in document units, the area past the column.
    let p = ui.painter_at(rect.expand(1.0));
    p.rect_filled(strip, 0.0, t.panel_darker);
    p.rect_filled(bar, 0.0, t.ruler);
    let end = to_x(width_pt);
    if end < rect.max.x {
        p.rect_filled(Rect::from_min_max(pos2(end, rect.min.y), rect.max), 0.0, t.input.gamma_multiply(0.7));
    }
    p.line_segment([pos2(x0, strip.min.y), pos2(x0, rect.max.y)], Stroke::new(1.0, t.ruler_tick));
    p.line_segment([pos2(end, strip.min.y), pos2(end, rect.max.y)], Stroke::new(1.0, t.ruler_tick));
    let (major, sub) = r.unit.ruler_ticks(scale as f64);
    let font = egui::FontId::proportional(9.0);
    let tick = Stroke::new(1.0, t.ruler_tick);
    let mut k = 0u32;
    while major > 0.0 && k < 2000 {
        let m = k as f64 * major;
        if to_x(m) > rect.max.x {
            break;
        }
        for s in 0..sub.max(1) {
            let x = to_x(m + major * s as f64 / sub.max(1) as f64);
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
        p.text(pos2(to_x(m) + 2.0, bar.min.y + 1.0), egui::Align2::LEFT_TOP, crate::canvas::fmt_tick(r.unit.from_pt(m)), font.clone(), t.ruler_text);
        k += 1;
    }
    // Indent markers (a dragged one follows the pointer; the first line follows the left indent).
    let (mut left, mut first_abs, mut right_x) = (r.left, r.left + r.first, width_pt - r.right);
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
    tri(to_x(first_abs), true, ink);
    tri(to_x(left), false, ink);
    tri(to_x(right_x), false, ink);
    // Tab stops.
    for (i, s) in r.tabs.iter().enumerate() {
        let dragging = drag.filter(|d| d.grab == Grab::Tab(i));
        if dragging.is_some_and(|d| d.off) {
            continue;
        }
        let x = to_x(dragging.map_or(s.position, |d| d.pos));
        let mark = Rect::from_center_size(pos2(x, strip.max.y - 6.0), vec2(14.0, 14.0));
        if r.selected == Some(i) {
            p.rect_filled(mark.expand(1.0), 2.0, t.accent);
        }
        crate::icons::paint(&p, mark, tab_icon(s.align), ink);
    }
    if let Some(d) = drag.filter(|d| matches!(d.grab, Grab::New)) {
        crate::icons::paint(&p, Rect::from_center_size(pos2(to_x(d.pos), strip.max.y - 6.0), vec2(16.0, 16.0)), "tab-left", t.accent_strong);
    }
    if let Some(d) = drag.filter(|d| !d.off) {
        p.line_segment([pos2(to_x(d.pos), strip.min.y), pos2(to_x(d.pos), rect.max.y)], Stroke::new(1.0, t.accent));
    }
    edit
}

/// The four alignment buttons; returns the one clicked.
pub(crate) fn align_buttons(ui: &mut Ui, lang: &str, current: TabAlign) -> Option<TabAlign> {
    let mut out = None;
    for (a, tip) in ALIGNS {
        if crate::icons::button(ui, tab_icon(a), 22.0, a == current, crate::i18n::tr(lang, tip)).clicked() {
            out = Some(a);
        }
    }
    out
}

/// A one-line text field that commits on Enter or when it loses focus.
pub(crate) fn text_commit(ui: &mut Ui, id: &str, current: &str, width: f32, enabled: bool) -> Option<String> {
    let tid = ui.id().with(("tabs_text", id));
    let focused = ui.memory(|m| m.has_focus(tid));
    let mut buf: String = if focused { ui.data(|d| d.get_temp(tid)).unwrap_or_else(|| current.to_string()) } else { current.to_string() };
    let r = ui.add_enabled(enabled, egui::TextEdit::singleline(&mut buf).id(tid).desired_width(width));
    ui.data_mut(|d| d.insert_temp(tid, buf.clone()));
    (r.lost_focus() && buf != current).then_some(buf)
}

/// The text column of the selection on screen: (top-left, top-right, top), when the
/// frame's text runs left to right unrotated in the view.
fn column_on_screen(app: &DesignApp, frame: u64, col: [f64; 4]) -> Option<(Pos2, Pos2, f32)> {
    let st = app.session.active()?;
    let v = app.view()?;
    let rect = app.canvas_rect?;
    let xf = crate::canvas::Xf::new(rect, v);
    let id = designcraft_doc::ItemId(frame);
    let loc = st.doc.find(id)?;
    let it = st.doc.item_at(&loc)?;
    let story = it.text_frame().and_then(|tf| st.doc.story(tf.story))?;
    if story.vertical {
        return None;
    }
    let layout = designcraft_tools::CanvasLayout::new(&st.doc, st.editing_parents);
    let m = layout.xf(loc.spread) * st.doc.parent_xf(&loc) * st.doc.text_xf(it);
    let a = xf.to_screen(m * Point::new(col[0], col[1]));
    let b = xf.to_screen(m * Point::new(col[2], col[1]));
    let c = xf.to_screen(m * Point::new(col[0], col[3]));
    // Only when the column runs straight across the screen.
    if (a.y - b.y).abs() > 0.5 || b.x <= a.x || (c.x - a.x).abs() > 0.5 {
        return None;
    }
    Some((a, b, a.y))
}

/// The Tabs panel.
pub fn show(app: &mut DesignApp, ui: &mut Ui) {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    let Ok(v) = app.session.execute("type.tabs.get", &json!({})) else {
        crate::rtl::label(ui, tr("Select text or a text frame."));
        return;
    };
    let tabs: Vec<TabStop> = serde_json::from_value(v["tabs"].clone()).unwrap_or_default();
    let num = |k: &str| v[k].as_f64().filter(|x| x.is_finite()).unwrap_or(0.0);
    let (left, first, right, width) = (num("leftIndent"), num("firstLineIndent"), num("rightIndent"), num("width"));
    let unit = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    let column = v["frame"].as_u64().zip(v["column"].as_array().and_then(|a| {
        let f = |i: usize| a.get(i).and_then(Value::as_f64);
        Some([f(0)?, f(1)?, f(2)?, f(3)?])
    }));
    let screen = column.and_then(|(f, c)| column_on_screen(app, f, c));
    let zoom = app.view().map(|v| v.zoom as f32);

    // Panel position above the frame: on request (the magnet, or opening the panel).
    let win = ui.ctx().memory(|m| m.area_rect(egui::Id::new(("floating_panel", "tabs"))));
    let state = &mut app.ui.tabs_panel;
    if state.selected.is_some_and(|i| i >= tabs.len()) {
        state.selected = None;
    }
    if state.snap_request > 0 {
        state.snap_request -= 1;
        if let (Some((a, b, top)), Some(z)) = (screen, zoom) {
            let h = win.map_or(90.0, |r| r.height());
            // The window's inner margin (10) plus the ruler's inset put position 0 over the column
            // edge; the panel spans at least the column.
            let width = (b.x - a.x + 2.0 * PAD + 20.0).max(MIN_WIDTH);
            state.place = Some((pos2(a.x - 10.0 - PAD, (top - h - 8.0).max(30.0)), width));
            state.snap = Some(z);
            ui.ctx().request_repaint();
        }
    }
    if state.snap.is_some() && state.snap != zoom {
        state.snap = None;
    }
    let selected = state.selected;
    let sel = selected.and_then(|i| tabs.get(i));
    let current_align = sel.map_or(state.align, |s| s.align);
    let mut cmds: Vec<(&'static str, Value)> = Vec::new();
    let mut snap_now = false;

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if let Some(a) = align_buttons(ui, &lang, current_align) {
            app.ui.tabs_panel.align = a;
            if let Some(i) = selected {
                cmds.push(("type.tabs.change", json!({"index": i, "align": a})));
            }
        }
        ui.add_space(6.0);
        crate::rtl::label(ui, tr("X:"));
        if let Some(x) = crate::widgets::measure(ui, "tabs_x", sel.map(|s| s.position), unit, 72.0) {
            match selected {
                Some(i) => cmds.push(("type.tabs.move", json!({"index": i, "position": x}))),
                None => cmds.push(("type.tabs.add", json!({"position": x, "align": current_align}))),
            }
        }
        crate::rtl::label(ui, tr("Leader:"));
        if let (Some(l), Some(i)) = (text_commit(ui, "leader", sel.map_or("", |s| s.leader.as_str()), 36.0, sel.is_some()), selected) {
            cmds.push(("type.tabs.change", json!({"index": i, "leader": l})));
        }
        crate::rtl::label(ui, tr("Align On:"));
        let char_tab = sel.is_some_and(|s| s.align == TabAlign::Char);
        if let (Some(c), Some(i)) = (text_commit(ui, "alignOn", sel.map_or("", |s| s.align_on.as_str()), 18.0, char_tab), selected) {
            cmds.push(("type.tabs.change", json!({"index": i, "alignOn": c})));
        }
        ui.add_space(6.0);
        let snap_tip = tr("Position panel above text frame");
        if ui.add_enabled_ui(screen.is_some(), |ui| crate::icons::button(ui, "tab-snap", 22.0, false, snap_tip)).inner.clicked() {
            snap_now = true;
        }
        crate::menus::menu_button(ui, "☰", |ui| {
            for (label, id, needs_stop) in
                [("Clear All", "type.tabs.clear", false), ("Delete Tab", "type.tabs.remove", true), ("Repeat Tab", "type.tabs.repeat", true)]
            {
                if ui.add_enabled(!needs_stop || selected.is_some(), egui::Button::new(crate::rtl::widget(ui, tr(label)))).clicked() {
                    cmds.push((id, selected.map_or(json!({}), |i| json!({"index": i}))));
                    ui.close();
                }
            }
            if ui.button(crate::rtl::widget(ui, tr("Reset Indents"))).clicked() {
                cmds.push(("type.para", json!({"attrs": {"leftIndent": 0, "firstLineIndent": 0, "rightIndent": 0}})));
                ui.close();
            }
        });
    });
    ui.add_space(4.0);
    let r = Ruler { tabs: &tabs, left, first, right, width, unit, selected, scale: app.ui.tabs_panel.snap };
    match ruler(ui, "panel", &r) {
        Some(RulerEdit::Select(i)) => app.ui.tabs_panel.selected = Some(i),
        Some(RulerEdit::Add(x)) => cmds.push(("type.tabs.add", json!({"position": x, "align": app.ui.tabs_panel.align}))),
        Some(RulerEdit::Move(i, x)) => cmds.push(("type.tabs.move", json!({"index": i, "position": x}))),
        Some(RulerEdit::Remove(i)) => cmds.push(("type.tabs.remove", json!({"index": i}))),
        Some(RulerEdit::Indent(k, x)) => cmds.push(("type.para", json!({"attrs": {k: x}}))),
        None => {}
    }
    if snap_now {
        app.ui.tabs_panel.snap_request = 2;
        ui.ctx().request_repaint();
    }
    for (id, p) in cmds {
        match app.run(id, p) {
            Ok(r) if id.starts_with("type.tabs.") => app.ui.tabs_panel.selected = r["index"].as_u64().map(|i| i as usize),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use crate::test_window;

    fn stops(app: &mut crate::DesignApp) -> Vec<f64> {
        let v = app.session.execute("type.tabs.get", &json!({})).unwrap();
        v["tabs"].as_array().unwrap().iter().map(|t| t["position"].as_f64().unwrap()).collect()
    }

    #[test]
    fn shortcut_opens_the_panel_and_a_click_on_the_ruler_adds_a_stop() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 372, 300], "content": "text", "text": "Name\tPrice"})).unwrap();
        app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        let mut h = test_window::open(app, egui::vec2(1400.0, 900.0));
        h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::T);
        h.run_steps(6);
        assert!(h.state().app.ui.floating.iter().any(|(p, _)| p == "tabs"), "Cmd+Shift+T floats the Tabs panel");
        // The ruler's strip lies just below the row of alignment buttons.
        let b = h.get_by_label("Left-aligned tab").rect();
        test_window::click_at(&mut h, egui::pos2(b.min.x + super::PAD + 60.0, b.max.y + 14.0));
        let got = stops(&mut h.state_mut().app);
        assert_eq!(got.len(), 1, "one stop added");
        assert!(got[0] > 0.0);
        assert_eq!(h.state().app.ui.tabs_panel.selected, Some(0));
        // The menu item (Type › Tabs) closes it again.
        h.state_mut().app.run("app.tabsPanel", json!({})).unwrap();
        assert!(!h.state().app.ui.floating.iter().any(|(p, _)| p == "tabs"));
    }
}
