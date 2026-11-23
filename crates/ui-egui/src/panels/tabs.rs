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

    let key = ui.scope_id().with(("tab_ruler", salt));
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
///
/// `name` is the visible caption's text (`Leader:`, `Align On:`): the field has no label widget of
/// its own, so screen readers and tests need it named here.
pub(crate) fn text_commit(ui: &mut Ui, id: &str, current: &str, width: f32, enabled: bool, name: &str) -> Option<String> {
    let tid = ui.scope_id().with(("tabs_text", id));
    let focused = ui.memory(|m| m.has_focus(tid));
    let mut buf: String = if focused { ui.data(|d| d.get_temp(tid)).unwrap_or_else(|| current.to_string()) } else { current.to_string() };
    let r = ui.add_enabled(enabled, egui::TextEdit::singleline(&mut buf).id(tid).desired_width(width)).accessible_name(name.to_owned());
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

// This panel's action faces only. The shared style-dialog alignment/text/ruler helpers above
// remain unchanged; their separate migration is not implied by this installed-native branch.
#[cfg(not(target_arch = "wasm32"))]
#[derive(craft_ui::contract::MemoProps)]
pub(crate) struct TabGlyph(pub(crate) &'static str);

#[cfg(not(target_arch = "wasm32"))]
impl craft_ui_widgets::primitives::Glyph for TabGlyph {
    fn paint(&self, painter: &egui::Painter, _tokens: &craft_ui_widgets::style::UiTokens, rect: Rect, color: Color32, _mirrored: bool) {
        // The app's existing tab-foot, alignment-dot, magnet and menu artwork; no new provider.
        crate::icons::paint(painter, rect, self.0, color);
    }
}

fn panel_alignments(ui: &mut Ui, lang: &str, framework: bool, target: &Option<String>, current: TabAlign) -> Option<TabAlign> {
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::buttons::IconButton;
        use craft_ui_widgets::contract::{Props as _, Show as _};
        use craft_ui_widgets::primitives::IconSpec;

        let mut picked = None;
        for (align, label) in ALIGNS {
            let label = crate::i18n::tr(lang, label);
            if IconButton::new(IconSpec::glyph(&TabGlyph(tab_icon(align))))
                .id_salt(("tabs-alignment", target, tab_icon(align)))
                .label(label.to_owned())
                .tooltip(label.to_owned())
                .selected(align == current)
                .show(ui)
                .clicked()
            {
                picked = Some(align);
            }
        }
        return picked;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (framework, target);
    align_buttons(ui, lang, current)
}

fn panel_snap(ui: &mut Ui, framework: bool, target: &Option<String>, label: &str, enabled: bool) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::buttons::IconButton;
        use craft_ui_widgets::contract::{Props as _, Show as _};
        use craft_ui_widgets::primitives::IconSpec;

        return IconButton::new(IconSpec::glyph(&TabGlyph("tab-snap")))
            .id_salt(("tabs-snap", target))
            .label(label.to_owned())
            .tooltip(label.to_owned())
            .enabled(enabled)
            .show(ui)
            .clicked();
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (framework, target);
    ui.add_enabled_ui(enabled, |ui| crate::icons::button(ui, "tab-snap", 22.0, false, label)).inner.clicked()
}

fn panel_menu(ui: &mut Ui, lang: &str, framework: bool, target: &Option<String>, selected: Option<usize>, cmds: &mut Vec<(&'static str, Value)>) {
    let tr = |s: &'static str| crate::i18n::tr(lang, s);
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::buttons::IconButton;
        use craft_ui_widgets::contract::{PopupKind, Props as _, Show as _, ShowContainer as _, Trigger};
        use craft_ui_widgets::menu::{Menu, MenuItem};
        use craft_ui_widgets::primitives::IconSpec;

        let opener = IconButton::new(IconSpec::glyph(&TabGlyph("menu")))
            .id_salt(("tabs-menu-opener", target))
            .label(tr("Tabs").to_owned())
            .tooltip(tr("Tabs").to_owned())
            .popup(PopupKind::Menu)
            .show(ui);
        let _ = Menu::new(tr("Tabs").to_owned()).id_salt(("tabs-menu", target)).opened_by(&opener, Trigger::Click).show(ui, |ui, _| {
            for (label, id, needs_stop) in
                [("Clear All", "type.tabs.clear", false), ("Delete Tab", "type.tabs.remove", true), ("Repeat Tab", "type.tabs.repeat", true)]
            {
                if MenuItem::new(tr(label).to_owned()).id_salt(id).enabled(!needs_stop || selected.is_some()).show(ui).ran {
                    cmds.push((id, selected.map_or(json!({}), |i| json!({"index": i}))));
                }
            }
            if MenuItem::new(tr("Reset Indents").to_owned()).id_salt("type.para").show(ui).ran {
                cmds.push(("type.para", json!({"attrs": {"leftIndent": 0, "firstLineIndent": 0, "rightIndent": 0}})));
            }
        });
        return;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (framework, target);
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
}

/// The X field: a measure in the document's unit, committed with Enter or when the field is left. Points in, points out.
#[cfg(not(target_arch = "wasm32"))]
fn measure_points(ui: &mut Ui, label: &str, points: Option<f64>, unit: Unit) -> Option<f64> {
    use crate::dialogs::framework::style_forms::style_kit;
    let shown = points.filter(|v| v.is_finite()).map(|v| designcraft_geom::format_measure(v, unit)).unwrap_or_default();
    let text = style_kit::committed_text(ui, "tabs_x", label, &shown, true)?;
    designcraft_geom::units::parse_measure(&text, unit).ok().filter(|v| v.is_finite())
}

/// The Tabs panel. Numeric fields, draft text and the shared ruler remain future migration work.
pub fn show(app: &mut DesignApp, ui: &mut Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::runtime::env::{Direction, with_env};

        let direction = if crate::i18n::is_rtl(&app.ui.language) { Direction::Rtl } else { Direction::Ltr };
        with_env(ui, |scope| scope.direction = Some(direction), |ui| show_contents(app, ui));
        return;
    }
    show_contents(app, ui);
}

fn show_contents(app: &mut DesignApp, ui: &mut Ui) {
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
    let win = ui.ctx().memory(|m| m.area_rect(egui::Id::unique(("floating_panel", "tabs"))));
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
    #[cfg(not(target_arch = "wasm32"))]
    let framework = app.framework_installed;
    #[cfg(target_arch = "wasm32")]
    let framework = false;
    // Stops have index identity, scoped to the real selected paragraphs and their document.
    // A held action may not follow a different range, stop, story, cell or document.
    // Alignment/leader values are not identity: ordinary edits must keep the same focus stop.
    let target =
        app.session.active().map(|st| format!("{:?}", (st.uid, st.identity_epoch, &st.selection, selected, sel.map(|stop| stop.position.to_bits()))));
    let mut cmds: Vec<(&'static str, Value)> = Vec::new();
    let mut snap_now = false;

    #[cfg(not(target_arch = "wasm32"))]
    let edit = if framework {
        use crate::dialogs::framework::style_forms::{style_kit, tab_forms::TabRuler};
        use craft_ui_widgets::contract::{Props as _, Show as _, ShowContainer as _, Sizing};
        use craft_ui_widgets::form::Field;
        use craft_ui_widgets::layout_components::{Row, Stack};

        let mut edit = None;
        let snap_scale = app.ui.tabs_panel.snap;
        Stack::new().id_salt("tabs_panel").fill_width().show(ui, |ui, _| {
            Row::new().id_salt("tabs_controls").wrap(true).show(ui, |ui, _| {
                if let Some(a) = panel_alignments(ui, &lang, true, &target, current_align) {
                    app.ui.tabs_panel.align = a;
                    if let Some(i) = selected {
                        cmds.push(("type.tabs.change", json!({"index": i, "align": a})));
                    }
                }
                Field::new(tr("X:")).id_salt("tabs_x").sizing(Sizing::Content).show(ui, |ui, _| {
                    if let Some(x) = measure_points(ui, tr("X:"), sel.map(|s| s.position), unit) {
                        match selected {
                            Some(i) => cmds.push(("type.tabs.move", json!({"index": i, "position": x}))),
                            None => cmds.push(("type.tabs.add", json!({"position": x, "align": current_align}))),
                        }
                    }
                });
                Field::new(tr("Leader:")).id_salt("tabs_leader").sizing(Sizing::Content).show(ui, |ui, _| {
                    let leader = style_kit::committed_text(ui, "tabs_leader", tr("Leader:"), sel.map_or("", |s| s.leader.as_str()), sel.is_some());
                    if let (Some(l), Some(i)) = (leader, selected) {
                        cmds.push(("type.tabs.change", json!({"index": i, "leader": l})));
                    }
                });
                Field::new(tr("Align On:")).id_salt("tabs_align_on").sizing(Sizing::Content).show(ui, |ui, _| {
                    let char_tab = sel.is_some_and(|s| s.align == TabAlign::Char);
                    let align_on = style_kit::committed_text(ui, "tabs_align_on", tr("Align On:"), sel.map_or("", |s| s.align_on.as_str()), char_tab);
                    if let (Some(c), Some(i)) = (align_on, selected) {
                        cmds.push(("type.tabs.change", json!({"index": i, "alignOn": c})));
                    }
                });
                if panel_snap(ui, true, &target, tr("Position panel above text frame"), screen.is_some()) {
                    snap_now = true;
                }
                panel_menu(ui, &lang, true, &target, selected, &mut cmds);
            });
            let out = TabRuler::new(tr("Tabs"), tabs.clone(), (left, first, right), width, unit, selected)
                .scale(snap_scale)
                .id_salt("tabs_ruler")
                .sizing(Sizing::FillWidth)
                .show(ui);
            edit = out.edit;
        });
        edit
    } else {
        None
    };
    #[cfg(target_arch = "wasm32")]
    let edit = None;
    if framework {
        match edit {
            Some(RulerEdit::Select(i)) => app.ui.tabs_panel.selected = Some(i),
            Some(RulerEdit::Add(x)) => cmds.push(("type.tabs.add", json!({"position": x, "align": app.ui.tabs_panel.align}))),
            Some(RulerEdit::Move(i, x)) => cmds.push(("type.tabs.move", json!({"index": i, "position": x}))),
            Some(RulerEdit::Remove(i)) => cmds.push(("type.tabs.remove", json!({"index": i}))),
            Some(RulerEdit::Indent(k, x)) => cmds.push(("type.para", json!({"attrs": {k: x}}))),
            None => {}
        }
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Some(a) = panel_alignments(ui, &lang, framework, &target, current_align) {
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
            let leader = text_commit(ui, "leader", sel.map_or("", |s| s.leader.as_str()), 36.0, sel.is_some(), tr("Leader:"));
            if let (Some(l), Some(i)) = (leader, selected) {
                cmds.push(("type.tabs.change", json!({"index": i, "leader": l})));
            }
            crate::rtl::label(ui, tr("Align On:"));
            let char_tab = sel.is_some_and(|s| s.align == TabAlign::Char);
            let align_on = text_commit(ui, "alignOn", sel.map_or("", |s| s.align_on.as_str()), 18.0, char_tab, tr("Align On:"));
            if let (Some(c), Some(i)) = (align_on, selected) {
                cmds.push(("type.tabs.change", json!({"index": i, "alignOn": c})));
            }
            ui.add_space(6.0);
            let snap_tip = tr("Position panel above text frame");
            if panel_snap(ui, framework, &target, snap_tip, screen.is_some()) {
                snap_now = true;
            }
            panel_menu(ui, &lang, framework, &target, selected, &mut cmds);
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod framework_actions_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::sync::Arc;

    struct State {
        app: DesignApp,
        recorded: Option<Arc<Recorded>>,
        parent: Rect,
    }

    fn text_app() -> (DesignApp, Value) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 1, "facingPages": false})).unwrap();
        let frame = app.run("frame.create", json!({"rect": [72, 72, 372, 300], "content": "text", "text": "first\trow\nsecond\trow"})).unwrap();
        app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 0})).unwrap();
        app.canvas_rect = Some(Rect::from_min_size(pos2(500.0, 0.0), vec2(600.0, 700.0)));
        (app, frame["story"].clone())
    }

    fn add_stops(app: &mut DesignApp, positions: &[f64]) {
        for position in positions {
            app.run("type.tabs.add", json!({"position": position})).unwrap();
        }
    }

    fn install_arabic_fixture(ctx: &egui::Context, language: &str) {
        #[cfg(target_os = "macos")]
        let family = "Geeza Pro";
        #[cfg(target_os = "windows")]
        let family = "Segoe UI";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let family = "DejaVu Sans";
        let db = designcraft_fonts::FontDb::global();
        assert!(db.has_family(family), "the native Arabic fixture requires already-installed {family}; no tofu, font installation or skipped locale");
        let face = db.face(family, "Regular");
        assert!(face.family.eq_ignore_ascii_case(family) && !face.is_placeholder(), "require the real {family}, not {}", face.family);
        let keys = [
            "Select text or a text frame.",
            "Tabs",
            "X:",
            "Leader:",
            "Align On:",
            "Position panel above text frame",
            "Clear All",
            "Delete Tab",
            "Repeat Tab",
            "Reset Indents",
        ];
        let mut covered = std::collections::BTreeSet::new();
        for key in keys.into_iter().chain(ALIGNS.into_iter().map(|(_, label)| label)) {
            for c in crate::i18n::tr(language, key).chars().filter(|c| designcraft_fonts::is_rtl(*c) || ('\u{0600}'..='\u{06ff}').contains(c)) {
                assert!(face.covers(c), "{family} lacks real Tabs glyph U+{:04X} from {key}", u32::from(c));
                covered.insert(c);
            }
        }
        assert!(!covered.is_empty());
        let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        let fallback = crate::theme::SystemFont::new("Arab", face.family.clone(), face.data().to_vec(), face.index());
        crate::theme::add_system_fallbacks(&mut definitions, &[fallback], language);
        ctx.set_fonts(definitions);
        eprintln!(
            "Tabs Arabic fixture: {} {}, face {}, {} bytes, {} distinct Tabs glyphs covered",
            face.family,
            face.style,
            face.index(),
            face.data().len(),
            covered.len()
        );
    }

    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, State> {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        if installed {
            app.framework_opacity = true;
            app.install_framework(&ctx);
            assert!(app.framework_installed);
        }
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut locale_fonts_ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(900.0, 330.0)).build_ui_state(
            move |ui, state: &mut State| {
                if !locale_fonts_ready {
                    locale_fonts_ready = true;
                    if state.app.ui.language == "ar" {
                        install_arabic_fixture(ui.ctx(), &state.app.ui.language);
                        return;
                    }
                }
                let region = ui.scope_id();
                let (_, recorded) = craft_ui_widgets::runtime::inspect::record(ui, region, |ui| show(&mut state.app, ui));
                state.parent = ui.min_rect();
                if recorded.is_some() {
                    state.recorded = recorded;
                }
            },
            State { app, recorded: None, parent: Rect::NOTHING },
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn steps(h: &mut Harness<'_, State>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn event(h: &mut Harness<'_, State>, event: Event) {
        h.input_mut().events.push(event);
        steps(h, 1);
    }

    fn key(h: &mut Harness<'_, State>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        steps(h, 2);
    }

    fn pointer(h: &mut Harness<'_, State>, pos: egui::Pos2, pressed: bool) {
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }

    fn click_at(h: &mut Harness<'_, State>, pos: egui::Pos2) {
        pointer(h, pos, true);
        pointer(h, pos, false);
        steps(h, 2);
    }

    fn label(h: &Harness<'_, State>, english: &str) -> String {
        crate::i18n::tr(&h.state().app.ui.language, english).to_owned()
    }

    fn element(h: &Harness<'_, State>, component: &str, english: &str) -> Element {
        let name = label(h, english);
        let found: Vec<_> = h
            .state()
            .recorded
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.component == component && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some(name.as_str())))
            .cloned()
            .collect();
        assert_eq!(found.len(), 1, "one real {component} named {name}: {found:?}");
        found[0].clone()
    }

    fn click(h: &mut Harness<'_, State>, component: &str, english: &str) {
        let _ = element(h, component, english);
        let name = label(h, english);
        let role = if component == "MenuItem" { egui::accesskit::Role::MenuItem } else { egui::accesskit::Role::Button };
        let pos = h.get_by(|n| n.role() == role && n.label().as_deref() == Some(name.as_str())).rect().center();
        click_at(h, pos);
    }

    fn tab_to(h: &mut Harness<'_, State>, english: &str) {
        let name = label(h, english);
        // One complete current accessibility-tree cycle also includes the legacy fields/ruler.
        let bound = h.query_all_by(|_| true).count();
        for _ in 0..=bound {
            if h.query_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(name.as_str()))
                .is_some_and(|n| n.is_focused())
            {
                return;
            }
            key(h, Key::Tab);
        }
        panic!("natural Tab did not reach {name} in the current real control cycle");
    }

    fn menu_key_to(h: &mut Harness<'_, State>, english: &str) {
        let name = label(h, english);
        let bound = h.query_all_by(|n| n.role() == egui::accesskit::Role::MenuItem).count();
        assert!(bound > 0, "open the actual menu first");
        for _ in 0..=bound {
            if h.query_by(|n| n.role() == egui::accesskit::Role::MenuItem && n.label().as_deref() == Some(name.as_str()))
                .is_some_and(|n| n.is_focused())
            {
                return;
            }
            key(h, Key::ArrowDown);
        }
        panic!("natural menu arrows did not reach {name}");
    }

    fn closed(h: &Harness<'_, State>) {
        assert_eq!(h.query_all_by(|n| n.role() == egui::accesskit::Role::MenuItem).count(), 0, "the real menu must close");
    }

    fn tab_state(h: &mut Harness<'_, State>) -> Value {
        h.state_mut().app.session.execute("type.tabs.get", &json!({})).unwrap()
    }

    fn positions(h: &mut Harness<'_, State>) -> Vec<f64> {
        tab_state(h)["tabs"].as_array().unwrap().iter().map(|tab| tab["position"].as_f64().unwrap()).collect()
    }

    fn select(h: &mut Harness<'_, State>, story: &Value, at: usize) {
        h.state_mut().app.run("text.select", json!({"story": story, "anchor": at, "focus": at})).unwrap();
        steps(h, 2);
    }

    fn ruler_at(h: &mut Harness<'_, State>, position: f64) -> egui::Pos2 {
        let state = tab_state(h);
        let parent = h.state().parent;
        // The unmodified ruler is the full-width final H-point allocation of the actual parent.
        let scale = h.state().app.ui.tabs_panel.snap.unwrap_or((parent.width() - 2.0 * PAD) / state["width"].as_f64().unwrap() as f32);
        pos2(parent.left() + PAD + position as f32 * scale, parent.bottom() - H + STRIP * 0.5)
    }

    fn pick_stop(h: &mut Harness<'_, State>, index: usize) {
        let position = positions(h)[index];
        let pos = ruler_at(h, position);
        let start = h.state().app.session.journal.len();
        click_at(h, pos);
        assert_eq!(h.state().app.ui.tabs_panel.selected, Some(index), "select using the real ruler");
        assert!(calls(h, start).is_empty(), "selecting a stop is not an edit");
    }

    fn calls(h: &Harness<'_, State>, from: usize) -> Vec<(String, Value)> {
        h.state().app.session.journal[from..]
            .iter()
            .filter(|(id, _)| {
                matches!(
                    id.as_str(),
                    "type.tabs.add"
                        | "type.tabs.move"
                        | "type.tabs.change"
                        | "type.tabs.clear"
                        | "type.tabs.remove"
                        | "type.tabs.repeat"
                        | "type.para"
                )
            })
            .cloned()
            .collect()
    }

    fn call(id: &str, params: Value) -> (String, Value) {
        (id.to_owned(), params)
    }

    fn undo_len(h: &Harness<'_, State>) -> usize {
        h.state().app.session.active().unwrap().history.undo.len()
    }

    fn undo(h: &mut Harness<'_, State>) {
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        steps(h, 2);
    }

    #[test]
    fn installed_default_alignment_uses_real_pointer_and_tab_without_document_edits_until_ruler_add() {
        let (app, _) = text_app();
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        let start = h.state().app.session.journal.len();
        for (label, align, keyboard) in [
            ("Centered tab", TabAlign::Center, false),
            ("Tab aligned on a character", TabAlign::Char, true),
            ("Left-aligned tab", TabAlign::Left, false),
            ("Right-aligned tab", TabAlign::Right, true),
        ] {
            if keyboard {
                tab_to(&mut h, label);
                key(&mut h, Key::Space);
            } else {
                click(&mut h, "IconButton", label);
            }
            assert_eq!(h.state().app.ui.tabs_panel.align, align);
            assert_eq!(h.state().app.ui.tabs_panel.selected, None);
            assert!(positions(&mut h).is_empty());
            assert!(calls(&h, start).is_empty());
            assert_eq!(undo_len(&h), history);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        }
        let pos = ruler_at(&mut h, 75.0);
        click_at(&mut h, pos);
        assert_eq!(calls(&h, start), [call("type.tabs.add", json!({"position": 75.0, "align": "right"}))]);
        assert_eq!(positions(&mut h), [75.0]);
        assert_eq!(tab_state(&mut h)["tabs"][0]["align"], "right");
        assert_eq!(undo_len(&h), history + 1);
        steps(&mut h, 3);
        assert_eq!(calls(&h, start).len(), 1);
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
    }

    #[test]
    fn installed_selected_stop_alignment_preserves_other_stop_and_paragraph_with_separate_undo() {
        let (mut app, story) = text_app();
        add_stops(&mut app, &[72.0, 144.0]);
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        pick_stop(&mut h, 1);
        let start = h.state().app.session.journal.len();
        click(&mut h, "IconButton", "Right-aligned tab");
        assert_eq!(calls(&h, start), [call("type.tabs.change", json!({"index": 1, "align": "right"}))]);
        let right = tab_state(&mut h)["tabs"].clone();
        assert_eq!(right[0]["align"], "left");
        assert_eq!(right[1]["align"], "right");
        assert_eq!(positions(&mut h), [72.0, 144.0]);
        assert_eq!(undo_len(&h), history + 1);
        tab_to(&mut h, "Tab aligned on a character");
        let start = h.state().app.session.journal.len();
        key(&mut h, Key::Space);
        assert_eq!(calls(&h, start), [call("type.tabs.change", json!({"index": 1, "align": "char"}))]);
        let changed = tab_state(&mut h)["tabs"].clone();
        assert_eq!(changed[0], right[0]);
        assert_eq!(changed[1]["align"], "char");
        assert_eq!(changed[1]["alignOn"], ".");
        let name = label(&h, "Tab aligned on a character");
        assert!(
            h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(name.as_str())).is_focused(),
            "an ordinary alignment edit retains the real keyboard focus stop"
        );
        assert_eq!(undo_len(&h), history + 2);
        steps(&mut h, 3);
        assert_eq!(calls(&h, start).len(), 1);
        undo(&mut h);
        assert_eq!(tab_state(&mut h)["tabs"], right);
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        select(&mut h, &story, 10);
        assert!(positions(&mut h).is_empty(), "the other paragraph is not the command target");
        select(&mut h, &story, 0);
        assert_eq!(positions(&mut h), [72.0, 144.0]);
    }

    #[test]
    fn installed_menu_delete_clear_disabled_boundaries_close_and_do_not_replay() {
        let (mut app, _) = text_app();
        add_stops(&mut app, &[72.0, 144.0]);
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        let start = h.state().app.session.journal.len();
        let name = label(&h, "Tabs");
        assert_eq!(
            h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(name.as_str())).accesskit_node().has_popup(),
            Some(egui::accesskit::HasPopup::Menu)
        );
        click(&mut h, "IconButton", "Tabs");
        for name in ["Delete Tab", "Repeat Tab"] {
            assert!(element(&h, "MenuItem", name).facts.unwrap().disabled);
            click(&mut h, "MenuItem", name);
            assert!(calls(&h, start).is_empty());
            assert_eq!(undo_len(&h), history);
        }
        key(&mut h, Key::Escape);
        closed(&h);
        tab_to(&mut h, "Tabs");
        key(&mut h, Key::Space);
        menu_key_to(&mut h, "Clear All");
        key(&mut h, Key::Enter);
        assert_eq!(calls(&h, start), [call("type.tabs.clear", json!({}))]);
        assert!(positions(&mut h).is_empty());
        closed(&h);
        assert_eq!(undo_len(&h), history + 1);
        steps(&mut h, 3);
        assert_eq!(calls(&h, start).len(), 1);
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        for (index, label, id, remaining) in [(1, "Delete Tab", "type.tabs.remove", vec![72.0]), (0, "Clear All", "type.tabs.clear", vec![])] {
            pick_stop(&mut h, index);
            let start = h.state().app.session.journal.len();
            click(&mut h, "IconButton", "Tabs");
            click(&mut h, "MenuItem", label);
            assert_eq!(calls(&h, start), [call(id, json!({"index": index}))]);
            assert_eq!(positions(&mut h), remaining);
            assert_eq!(h.state().app.ui.tabs_panel.selected, None);
            assert_eq!(undo_len(&h), history + 1);
            closed(&h);
            steps(&mut h, 3);
            assert_eq!(calls(&h, start).len(), 1);
            undo(&mut h);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        }
    }

    #[test]
    fn installed_repeat_uses_real_menu_keys_and_preserves_real_refusal_without_history() {
        let (mut app, _) = text_app();
        add_stops(&mut app, &[72.0, 144.0]);
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        pick_stop(&mut h, 1);
        tab_to(&mut h, "Tabs");
        key(&mut h, Key::Space);
        menu_key_to(&mut h, "Repeat Tab");
        let start = h.state().app.session.journal.len();
        key(&mut h, Key::Enter);
        assert_eq!(calls(&h, start), [call("type.tabs.repeat", json!({"index": 1}))]);
        assert_eq!(positions(&mut h), [72.0, 144.0, 216.0, 288.0]);
        assert_eq!(h.state().app.ui.tabs_panel.selected, Some(1));
        assert_eq!(undo_len(&h), history + 1);
        closed(&h);
        steps(&mut h, 3);
        assert_eq!(calls(&h, start).len(), 1);
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));

        let (mut app, _) = text_app();
        add_stops(&mut app, &[0.5]);
        let mut h = harness(app, true);
        pick_stop(&mut h, 0);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        let start = h.state().app.session.journal.len();
        click(&mut h, "IconButton", "Tabs");
        assert!(!element(&h, "MenuItem", "Repeat Tab").facts.unwrap().disabled, "the existing UI lets the engine report this legitimate refusal");
        click(&mut h, "MenuItem", "Repeat Tab");
        assert!(calls(&h, start).is_empty(), "a refused command is not a successful journal entry");
        assert!(h.state().app.ui.status.contains("at least 1 pt"));
        assert_eq!(positions(&mut h), [0.5]);
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        closed(&h);
    }

    #[test]
    fn installed_reset_indents_retains_stops_exact_payload_and_one_independent_undo() {
        let (mut app, _) = text_app();
        add_stops(&mut app, &[72.0]);
        app.run("type.para", json!({"attrs": {"leftIndent": 18.0, "firstLineIndent": -6.0, "rightIndent": 12.0}})).unwrap();
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let previous = tab_state(&mut h);
        let history = undo_len(&h);
        tab_to(&mut h, "Tabs");
        key(&mut h, Key::Space);
        menu_key_to(&mut h, "Reset Indents");
        let start = h.state().app.session.journal.len();
        key(&mut h, Key::Space);
        assert_eq!(calls(&h, start), [call("type.para", json!({"attrs": {"leftIndent": 0, "firstLineIndent": 0, "rightIndent": 0}}))]);
        let reset = tab_state(&mut h);
        for name in ["leftIndent", "firstLineIndent", "rightIndent"] {
            assert_eq!(reset[name].as_f64(), Some(0.0));
        }
        assert_eq!(reset["tabs"], previous["tabs"]);
        assert_eq!(undo_len(&h), history + 1);
        closed(&h);
        steps(&mut h, 3);
        assert_eq!(calls(&h, start).len(), 1);
        undo(&mut h);
        assert_eq!(tab_state(&mut h), previous);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
    }

    #[test]
    fn installed_snap_enabled_and_disabled_keep_panel_state_separate_from_document_history() {
        for enabled in [true, false] {
            let (mut app, _) = text_app();
            if !enabled {
                app.canvas_rect = None;
            }
            let mut h = harness(app, true);
            let before = h.state().app.session.active().unwrap().doc.clone();
            let history = undo_len(&h);
            let start = h.state().app.session.journal.len();
            let button = element(&h, "IconButton", "Position panel above text frame");
            assert_eq!(!button.facts.unwrap().disabled, enabled);
            click(&mut h, "IconButton", "Position panel above text frame");
            if enabled {
                assert_eq!(h.state().app.ui.tabs_panel.snap, h.state().app.view().map(|view| view.zoom as f32));
                let (position, width) = h.state().app.ui.tabs_panel.place.unwrap();
                assert!(position.x.is_finite() && position.y.is_finite());
                assert!(width >= MIN_WIDTH);
            } else {
                assert_eq!(h.state().app.ui.tabs_panel.snap, None);
                assert_eq!(h.state().app.ui.tabs_panel.place, None);
                assert_eq!(h.state().app.ui.tabs_panel.snap_request, 0);
            }
            assert!(calls(&h, start).is_empty());
            assert_eq!(undo_len(&h), history);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
            steps(&mut h, 3);
            assert!(calls(&h, start).is_empty());
        }
    }

    #[test]
    fn installed_held_alignment_cannot_follow_another_paragraph_stop_document_or_absent_selection() {
        for transition in ["paragraph", "stop", "document", "absent"] {
            let (mut app, story) = text_app();
            add_stops(&mut app, &[72.0, 144.0]);
            app.run("text.select", json!({"story": story, "anchor": 10, "focus": 10})).unwrap();
            add_stops(&mut app, &[72.0, 144.0]);
            app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
            let mut h = harness(app, true);
            pick_stop(&mut h, 0);
            let name = label(&h, "Right-aligned tab");
            let pos = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(name.as_str())).rect().center();
            pointer(&mut h, pos, true);
            match transition {
                "paragraph" => {
                    h.state_mut().app.run("text.select", json!({"story": story, "anchor": 10, "focus": 10})).unwrap();
                }
                "stop" => {
                    h.state_mut().app.run("type.tabs.remove", json!({"index": 0})).unwrap();
                }
                "document" => {
                    h.state_mut().app.run("file.new", json!({"pages": 1})).unwrap();
                    let frame = h
                        .state_mut()
                        .app
                        .run("frame.create", json!({"rect": [72, 72, 372, 300], "content": "text", "text": "another\tparagraph"}))
                        .unwrap();
                    h.state_mut().app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 0})).unwrap();
                    add_stops(&mut h.state_mut().app, &[72.0, 144.0]);
                }
                "absent" => {
                    h.state_mut().app.run("selection.set", json!({"ids": []})).unwrap();
                }
                _ => unreachable!(),
            }
            steps(&mut h, 2);
            let target_before = h.state().app.session.active().unwrap().doc.clone();
            let original_before = h.state().app.session.documents()[0].doc.clone();
            let history = undo_len(&h);
            let start = h.state().app.session.journal.len();
            pointer(&mut h, pos, false);
            steps(&mut h, 3);
            assert!(calls(&h, start).is_empty(), "held action must cancel across {transition}");
            assert_eq!(undo_len(&h), history);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &target_before));
            if transition == "absent" {
                select(&mut h, &story, 0);
            }
            assert_eq!(h.state().app.ui.tabs_panel.selected, Some(0));
            let start = h.state().app.session.journal.len();
            click(&mut h, "IconButton", "Right-aligned tab");
            assert_eq!(calls(&h, start), [call("type.tabs.change", json!({"index": 0, "align": "right"}))]);
            assert_eq!(tab_state(&mut h)["tabs"][0]["align"], "right");
            assert_eq!(undo_len(&h), history + 1);
            undo(&mut h);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &target_before));
            assert!(Arc::ptr_eq(&h.state().app.session.documents()[0].doc, &original_before));
        }
    }

    #[test]
    fn installed_held_menu_row_cannot_retarget_another_paragraph_and_fresh_menu_recovers() {
        let (mut app, story) = text_app();
        add_stops(&mut app, &[72.0, 144.0]);
        app.run("text.select", json!({"story": story, "anchor": 10, "focus": 10})).unwrap();
        add_stops(&mut app, &[80.0, 160.0]);
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
        let mut h = harness(app, true);
        pick_stop(&mut h, 0);
        click(&mut h, "IconButton", "Tabs");
        let name = label(&h, "Delete Tab");
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::MenuItem && n.label().as_deref() == Some(name.as_str())).rect().center();
        pointer(&mut h, pos, true);
        select(&mut h, &story, 10);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        let start = h.state().app.session.journal.len();
        pointer(&mut h, pos, false);
        steps(&mut h, 3);
        closed(&h);
        assert!(calls(&h, start).is_empty());
        assert_eq!(positions(&mut h), [80.0, 160.0]);
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        click(&mut h, "IconButton", "Tabs");
        click(&mut h, "MenuItem", "Delete Tab");
        assert_eq!(calls(&h, start), [call("type.tabs.remove", json!({"index": 0}))]);
        assert_eq!(positions(&mut h), [160.0]);
        assert_eq!(undo_len(&h), history + 1);
        closed(&h);
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        select(&mut h, &story, 0);
        assert_eq!(positions(&mut h), [72.0, 144.0]);
    }

    #[test]
    fn installed_localized_complete_tab_and_menu_labels_keep_natural_keyboard_and_logical_rtl() {
        for language in ["de", "ar"] {
            let (mut app, _) = text_app();
            app.ui.language = language.to_owned();
            let mut h = harness(app, true);
            let before = h.state().app.session.active().unwrap().doc.clone();
            let history = undo_len(&h);
            let start = h.state().app.session.journal.len();
            for (_, english) in ALIGNS {
                let name = label(&h, english);
                assert_ne!(name, english, "exercise real translated complete names");
                let button = element(&h, "IconButton", english);
                assert_eq!(button.facts.as_ref().unwrap().name.as_deref(), Some(name.as_str()));
                assert!(button.stop.is_some());
            }
            assert_eq!(label(&h, "Right-aligned tab"), if language == "de" { "Rechtsbündiger Tabulator" } else { "علامة جدولة محاذاة لليمين" });
            tab_to(&mut h, "Right-aligned tab");
            key(&mut h, Key::Space);
            assert_eq!(h.state().app.ui.tabs_panel.align, TabAlign::Right);
            assert!(calls(&h, start).is_empty());
            assert_eq!(undo_len(&h), history);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
            let pos = ruler_at(&mut h, 60.0);
            click_at(&mut h, pos);
            assert_eq!(calls(&h, start), [call("type.tabs.add", json!({"position": 60.0, "align": "right"}))]);
            assert_eq!(tab_state(&mut h)["tabs"][0]["align"], "right");
            assert_eq!(undo_len(&h), history + 1);
            undo(&mut h);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
            let start = h.state().app.session.journal.len();
            tab_to(&mut h, "Tabs");
            key(&mut h, Key::Space);
            for english in ["Clear All", "Delete Tab", "Repeat Tab", "Reset Indents"] {
                let name = label(&h, english);
                assert_ne!(name, english);
                assert_eq!(element(&h, "MenuItem", english).facts.as_ref().unwrap().name.as_deref(), Some(name.as_str()));
            }
            key(&mut h, Key::Escape);
            closed(&h);
            assert!(calls(&h, start).is_empty());
            assert_eq!(undo_len(&h), history);
        }
    }

    #[test]
    fn uninstalled_full_parent_retains_native_alignment_menu_payloads_and_independent_undo() {
        let (mut app, _) = text_app();
        add_stops(&mut app, &[72.0, 144.0]);
        let mut h = harness(app, false);
        assert!(!h.state().app.framework_installed);
        assert!(!h.state().recorded.as_ref().unwrap().elements.iter().any(|e| matches!(e.component, "IconButton" | "Menu" | "MenuItem")));
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        pick_stop(&mut h, 1);
        let start = h.state().app.session.journal.len();
        let pos = h.get_by_label("Centered tab").rect().center();
        click_at(&mut h, pos);
        assert_eq!(calls(&h, start), [call("type.tabs.change", json!({"index": 1, "align": "center"}))]);
        assert_eq!(tab_state(&mut h)["tabs"][1]["align"], "center");
        assert_eq!(undo_len(&h), history + 1);
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        let start = h.state().app.session.journal.len();
        let pos = h.get_by_label("☰").rect().center();
        click_at(&mut h, pos);
        let pos = h.get_by_label("Delete Tab").rect().center();
        click_at(&mut h, pos);
        assert_eq!(calls(&h, start), [call("type.tabs.remove", json!({"index": 1}))]);
        assert_eq!(positions(&mut h), [72.0]);
        assert_eq!(undo_len(&h), history + 1);
        steps(&mut h, 3);
        assert_eq!(calls(&h, start).len(), 1);
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
    }
}
