//! File › New › Document, laid out like InDesign's New Document window: preset tabs (Recent,
//! Saved, Print, Web, Mobile) over page cards on one side, the chosen preset's details on the
//! other. Every value lives in the dialog's fields, so the control channel sets them
//! (`ui.dialog.set`) like a user would; `confirm` turns them into `file.new`.

use designcraft_doc::Intent;
use designcraft_doc::build::{NewDocument, PRESETS};
use designcraft_geom::units::{PT_PER_MM, parse_measure};
use designcraft_geom::{Unit, format_measure};
use egui::{Align, Align2, Color32, FontId, Layout, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Map, Value, json};

use crate::DesignApp;
use crate::dialogs::Dialog;
use crate::theme::{Tokens, semibold};

/// Tab key and English label.
const TABS: [(&str, &str); 5] = [("recent", "Recent"), ("saved", "Saved"), ("print", "Print"), ("web", "Web"), ("mobile", "Mobile")];
/// The card of the factory settings, first on the Recent tab.
const DEFAULT_CARD: &str = "[Default]";
/// The details pane (and the stacked layout's width below `TWO_PANES`).
const DETAILS_W: f32 = 300.0;
const TWO_PANES: f32 = 700.0;
const COLUMN_W: f32 = 124.0;
const FIELD_H: f32 = 24.0;
const CARD: egui::Vec2 = egui::vec2(132.0, 150.0);
const MAX_RECENT: usize = 12;
const EDGES: [&str; 4] = ["Top", "Bottom", "Inside", "Outside"];
/// Edge groups: field prefix (`marginTop`, `bleedTop`, `slugTop`).
const GROUPS: [&str; 3] = ["margin", "bleed", "slug"];

/// The dialog's fields for `nd`, its measures in its own units.
fn fields_for(nd: &NewDocument) -> Map<String, Value> {
    let u = nd.units;
    let m = |v: f64| json!(format_measure(v, u));
    let mut f = Map::new();
    f.insert("intent".into(), json!(nd.intent));
    f.insert("units".into(), json!(u));
    f.insert("width".into(), m(nd.width));
    f.insert("height".into(), m(nd.height));
    f.insert("pages".into(), json!(nd.pages));
    f.insert("startPage".into(), json!(nd.start_page));
    f.insert("facingPages".into(), json!(nd.facing_pages));
    f.insert("primaryTextFrame".into(), json!(nd.primary_text_frame));
    f.insert("columns".into(), json!(nd.columns));
    f.insert("gutter".into(), m(nd.gutter));
    let margins = [nd.margins.top, nd.margins.bottom, nd.margins.inside, nd.margins.outside];
    for (group, values) in GROUPS.into_iter().zip([margins, nd.bleed, nd.slug]) {
        for (edge, v) in EDGES.into_iter().zip(values) {
            f.insert(format!("{group}{edge}"), m(v));
        }
        f.insert(format!("{group}Linked"), json!(values.iter().all(|v| (v - values[0]).abs() < 1e-6)));
    }
    f
}

/// The fields a new New Document dialog starts with: the factory settings on the Recent tab.
pub(crate) fn defaults() -> Value {
    let mut f = fields_for(&NewDocument::default());
    f.insert("tab".into(), json!("recent"));
    f.insert("preset".into(), json!(DEFAULT_CARD));
    f.insert("name".into(), json!(""));
    f.insert("presetName".into(), json!(""));
    Value::Object(f)
}

fn units(d: &Dialog) -> Unit {
    d.fields.get("units").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(Unit::Picas)
}

/// A measure field in points: a number is points, text is read in the dialog's units.
fn measure(d: &Dialog, key: &str) -> Option<f64> {
    match d.fields.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => parse_measure(s, units(d)).ok(),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

fn count(d: &Dialog, key: &str) -> u64 {
    d.n(key).filter(|v| v.is_finite()).unwrap_or(1.0).clamp(1.0, 9999.0).round() as u64
}

/// Every measure field, for a change of units.
fn measure_keys() -> Vec<String> {
    let mut keys: Vec<String> = ["width", "height", "gutter"].into_iter().map(String::from).collect();
    keys.extend(GROUPS.into_iter().flat_map(|g| EDGES.into_iter().map(move |e| format!("{g}{e}"))));
    keys
}

/// `file.new` parameters for the dialog's fields.
pub(crate) fn params(d: &Dialog) -> Value {
    let m = |k: &str, default: f64| measure(d, k).unwrap_or(default).max(0.0);
    let edges = |group: &str| EDGES.map(|e| m(&format!("{group}{e}"), 0.0));
    // A control client may still set one `bleed` for every edge.
    let bleed = measure(d, "bleed").map_or_else(|| json!(edges("bleed")), |b| json!(b.max(0.0)));
    let margins = edges("margin");
    let mut p = json!({
        "intent": d.fields.get("intent").cloned().unwrap_or(json!("print")),
        "units": units(d),
        "width": measure(d, "width"),
        "height": measure(d, "height"),
        "pages": count(d, "pages"),
        "startPage": count(d, "startPage"),
        "facingPages": d.b("facingPages"),
        "columns": count(d, "columns"),
        "gutter": m("gutter", 12.0),
        "margins": {"top": margins[0], "bottom": margins[1], "inside": margins[2], "outside": margins[3]},
        "bleed": bleed,
        "slug": edges("slug"),
        "primaryTextFrame": d.b("primaryTextFrame"),
    });
    let name = d.s("name");
    if !name.trim().is_empty() {
        p["title"] = json!(name.trim());
    }
    p
}

/// The settings `file.new` was given, as a preset (no title).
fn as_preset(p: &Value) -> Option<NewDocument> {
    let mut v = p.clone();
    let o = v.as_object_mut()?;
    o.remove("title");
    o.retain(|_, v| !v.is_null());
    if let Some(b) = o.get("bleed").and_then(Value::as_f64) {
        o.insert("bleed".into(), json!([b, b, b, b]));
    }
    serde_json::from_value(v).ok()
}

/// Remember a document just made with `p` for the Recent tab.
pub(crate) fn remember(app: &mut DesignApp, p: &Value) {
    let Some(nd) = as_preset(p) else { return };
    let recent = &mut app.ui.recent_new_documents;
    recent.retain(|r| *r != nd);
    recent.insert(0, nd);
    recent.truncate(MAX_RECENT);
}

/// Save the details as the document preset `name` (`file.savePreset`, which replaces one of that
/// name) and show it on the Saved tab.
fn save_preset(app: &mut DesignApp, d: &mut Dialog, name: &str) -> Result<(), String> {
    let name = name.trim();
    let mut p = params(d);
    if let Some(o) = p.as_object_mut() {
        o.remove("title");
        o.retain(|_, v| !v.is_null());
        o.insert("name".into(), json!(name));
    }
    app.run("file.savePreset", p)?;
    d.fields.insert("tab".into(), json!("saved"));
    d.fields.insert("preset".into(), json!(format!("saved:{name}")));
    Ok(())
}

/// The built-in preset `nd` is the size of, if any.
fn preset_name(nd: &NewDocument) -> Option<&'static str> {
    PRESETS.iter().find(|p| p.intent == nd.intent && (p.width - nd.width).abs() < 0.01 && (p.height - nd.height).abs() < 0.01).map(|p| p.name)
}

struct Card {
    key: String,
    label: String,
    nd: NewDocument,
    /// Not a built-in size: drawn with crop marks.
    custom: bool,
}

fn cards(app: &DesignApp, tab: &str) -> Vec<Card> {
    let lang = &app.ui.language;
    let named = |key: String, nd: NewDocument| {
        let (label, custom) = match preset_name(&nd) {
            Some(n) => (crate::i18n::tr(lang, n).to_string(), false),
            None => (crate::i18n::tr(lang, "Custom").to_string(), true),
        };
        Card { key, label, nd, custom }
    };
    match tab {
        "recent" => {
            let default =
                Card { key: DEFAULT_CARD.into(), label: crate::i18n::tr(lang, DEFAULT_CARD).into(), nd: NewDocument::default(), custom: false };
            std::iter::once(default)
                .chain(app.ui.recent_new_documents.iter().enumerate().map(|(i, nd)| named(format!("recent:{i}"), nd.clone())))
                .collect()
        }
        "saved" => app
            .session
            .prefs
            .document_presets
            .iter()
            .map(|nd| Card { key: format!("saved:{}", nd.title), label: nd.title.clone(), nd: nd.clone(), custom: preset_name(nd).is_none() })
            .collect(),
        tab => {
            let intent = match tab {
                "web" => Intent::Web,
                "mobile" => Intent::Mobile,
                _ => Intent::Print,
            };
            PRESETS
                .iter()
                .filter(|p| p.intent == intent)
                .filter_map(|p| {
                    NewDocument::from_preset(p.name).map(|nd| Card {
                        key: p.name.into(),
                        label: crate::i18n::tr(lang, p.name).into(),
                        nd,
                        custom: false,
                    })
                })
                .collect()
        }
    }
}

/// `210 × 297 mm`, `51p0 × 66p0`.
fn size_text(nd: &NewDocument) -> String {
    let u = nd.units;
    let (w, h) = (format_measure(nd.width, u), format_measure(nd.height, u));
    let suffix = u.suffix();
    if suffix.starts_with(' ')
        && let (Some(a), Some(b)) = (w.strip_suffix(suffix), h.strip_suffix(suffix))
    {
        return format!("{a} × {b}{suffix}");
    }
    format!("{w} × {h}")
}

/// A page with a folded corner, its proportions those of the preset; crop marks for a custom size.
fn paint_page(p: &egui::Painter, area: Rect, nd: &NewDocument, custom: bool, color: Color32) {
    let aspect = if nd.height > 0.0 { (nd.width / nd.height).clamp(0.2, 5.0) as f32 } else { 1.0 };
    let size =
        if aspect >= area.width() / area.height() { vec2(area.width(), area.width() / aspect) } else { vec2(area.height() * aspect, area.height()) };
    let r = Rect::from_center_size(area.center(), size);
    let fold = (size.min_elem() * 0.24).clamp(5.0, 14.0);
    let stroke = Stroke::new(1.5, color);
    let corner = pos2(r.right() - fold, r.top());
    p.add(egui::Shape::line(vec![r.left_top(), corner, pos2(r.right(), r.top() + fold), r.right_bottom(), r.left_bottom(), r.left_top()], stroke));
    p.add(egui::Shape::line(vec![corner, pos2(corner.x, r.top() + fold), pos2(r.right(), r.top() + fold)], stroke));
    if custom {
        let (gap, len) = (5.0, 10.0);
        p.line_segment([pos2(r.left() - gap - len, r.top() - gap), pos2(r.left() - gap, r.top() - gap)], stroke);
        p.line_segment([pos2(r.left() - gap, r.top() - gap - len), pos2(r.left() - gap, r.top() - gap)], stroke);
    }
}

fn card(ui: &mut Ui, c: &Card, selected: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(CARD, Sense::click());
    if selected {
        ui.painter().rect(r.shrink(1.0), 3.0, t.hover, Stroke::new(2.0, t.accent), StrokeKind::Inside);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, t.hover);
    }
    let page = Rect::from_center_size(pos2(r.center().x, r.top() + 56.0), vec2(72.0, 72.0));
    paint_page(ui.painter(), page, &c.nd, c.custom, if selected { t.text_strong } else { t.icon });
    crate::rtl::paint(ui.painter(), pos2(r.center().x, r.top() + 114.0), Align2::CENTER_CENTER, &c.label, semibold(12.0), t.text_strong);
    crate::rtl::paint(
        ui.painter(),
        pos2(r.center().x, r.top() + 132.0),
        Align2::CENTER_CENTER,
        &size_text(&c.nd),
        FontId::proportional(11.0),
        t.text_dim,
    );
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &c.label));
    resp
}

/// A row that runs right to left in a right-to-left interface.
fn row<R>(ui: &mut Ui, rtl: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layout = if rtl { Layout::right_to_left(Align::TOP) } else { Layout::left_to_right(Align::TOP) };
    ui.with_layout(layout, add).inner
}

fn tabs(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let t = Tokens::get(ui.ctx());
    let rtl = crate::i18n::is_rtl(&app.ui.language);
    let current = d.s("tab");
    let strip = row(ui, rtl, |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (key, label) in TABS {
            let text = crate::i18n::tr(&app.ui.language, label);
            let on = current == key;
            let font = if on { semibold(13.0) } else { FontId::proportional(13.0) };
            let text_w = ui.painter().layout_no_wrap(text.to_string(), font.clone(), t.text).size().x;
            let icon_w = if key == "recent" { 20.0 } else { 0.0 };
            let (r, resp) = ui.allocate_exact_size(vec2(text_w + icon_w + 20.0, 34.0), Sense::click());
            let color = if on || resp.hovered() { t.text_strong } else { t.text_dim };
            let start = if rtl { r.right() - 10.0 - icon_w } else { r.left() + 10.0 };
            if key == "recent" {
                let icon = Rect::from_min_size(pos2(if rtl { r.right() - 26.0 } else { r.left() + 8.0 }, r.center().y - 9.0), vec2(16.0, 16.0));
                crate::icons::paint(ui.painter(), icon, "clock", color);
            }
            let anchor = if rtl { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER };
            let x = if rtl { start } else { start + icon_w };
            crate::rtl::paint(ui.painter(), pos2(x, r.center().y - 1.0), anchor, text, font, color);
            if on {
                ui.painter().rect_filled(
                    Rect::from_min_max(pos2(r.left() + 6.0, r.bottom() - 2.0), pos2(r.right() - 6.0, r.bottom())),
                    1.0,
                    t.text_strong,
                );
            }
            if resp.clicked() {
                d.fields.insert("tab".into(), json!(key));
            }
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, text));
        }
        ui.min_rect()
    });
    let y = strip.bottom();
    ui.painter().line_segment([pos2(ui.min_rect().left(), y), pos2(ui.max_rect().right(), y)], Stroke::new(1.0, t.divider));
}

/// The cards of the current tab; true when one was double-clicked (create at once).
fn presets(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let rtl = crate::i18n::is_rtl(&lang);
    let tab = d.s("tab");
    let list = cards(app, &tab);
    let heading = match tab.as_str() {
        "recent" => "Your Recent Items",
        "saved" => "Saved Presets",
        _ => "Blank Document Presets",
    };
    row(ui, rtl, |ui| {
        crate::rtl::label(
            ui,
            egui::RichText::new(format!("{} ({})", crate::i18n::tr(&lang, heading).to_uppercase(), list.len()))
                .font(semibold(11.5))
                .color(t.text_dim),
        );
    });
    ui.add_space(10.0);
    if list.is_empty() {
        crate::rtl::label(
            ui,
            egui::RichText::new(crate::i18n::tr(&lang, "No saved presets yet. Set the details, then click Save Document Preset.")).color(t.text_dim),
        );
        return false;
    }
    let current = d.s("preset");
    let mut create = false;
    let mut delete = None;
    let layout = if rtl { Layout::right_to_left(Align::TOP) } else { Layout::left_to_right(Align::TOP) }.with_main_wrap(true);
    ui.with_layout(layout, |ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        for c in &list {
            let resp = card(ui, c, current == c.key);
            if resp.clicked() || resp.double_clicked() {
                let name = d.s("name");
                d.fields.extend(fields_for(&c.nd));
                d.fields.insert("name".into(), json!(name));
                d.fields.insert("preset".into(), json!(c.key));
            }
            if resp.double_clicked() {
                create = true;
            }
            if tab == "saved" {
                resp.context_menu(|ui| {
                    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Delete Preset"))).clicked() {
                        delete = Some(c.nd.title.clone());
                        ui.close();
                    }
                });
            }
        }
    });
    if let Some(name) = delete
        && let Err(e) = app.run("file.deletePreset", json!({"name": name}))
    {
        app.status(e);
    }
    create
}

#[derive(Clone, Copy)]
enum Step {
    /// A measure no smaller than this many points.
    Measure(f64),
    /// A whole number from 1.
    Count,
}

/// One arrow click: a unit's usual increment.
fn increment(u: Unit) -> f64 {
    match u {
        Unit::Millimeters | Unit::Centimeters => PT_PER_MM,
        Unit::Inches | Unit::InchesDecimal => 4.5,
        _ => 1.0,
    }
}

fn nudge(d: &mut Dialog, key: &str, dir: f64, step: Step) {
    match step {
        Step::Count => {
            let v = (count(d, key) as f64 + dir).clamp(1.0, 9999.0);
            d.fields.insert(key.into(), json!(v as u64));
        }
        Step::Measure(min) => {
            let u = units(d);
            let v = (measure(d, key).unwrap_or(min) + dir * increment(u)).clamp(min, 15552.0);
            d.fields.insert(key.into(), json!(format_measure(v, u)));
        }
    }
}

/// A field with up/down arrows at its start; true when its value changed.
fn spin(ui: &mut Ui, d: &mut Dialog, key: &str, width: f32, step: Step) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(width, FIELD_H), Sense::hover());
    ui.painter().rect(r, 2.0, t.input, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    let seg = Rect::from_min_size(r.min, vec2(18.0, FIELD_H));
    ui.painter().line_segment([seg.right_top(), seg.right_bottom()], Stroke::new(1.0, t.field_border));
    let mut changed = false;
    for (dir, half) in
        [(1.0, Rect::from_min_max(seg.min, pos2(seg.max.x, seg.center().y))), (-1.0, Rect::from_min_max(pos2(seg.min.x, seg.center().y), seg.max))]
    {
        let resp = ui.interact(half, ui.id().with((key, dir < 0.0)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(half.shrink(1.0), 1.0, t.hover);
        }
        let c = half.center();
        let s = 3.0 * dir as f32;
        ui.painter().add(egui::Shape::line(
            vec![pos2(c.x - 3.5, c.y + s / 2.0), pos2(c.x, c.y - s / 2.0), pos2(c.x + 3.5, c.y + s / 2.0)],
            Stroke::new(1.2, t.icon),
        ));
        if resp.clicked() {
            nudge(d, key, dir, step);
            changed = true;
        }
    }
    let text_rect = Rect::from_min_max(pos2(seg.right() + 6.0, r.top() + 1.0), pos2(r.right() - 4.0, r.bottom() - 1.0));
    let mut s = d.s(key);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(Layout::left_to_right(Align::Center)));
    let resp = child.add(
        egui::TextEdit::singleline(&mut s)
            .id(ui.id().with((key, "text")))
            .frame(egui::Frame::NONE)
            .desired_width(text_rect.width())
            .horizontal_align(Align::Min)
            .vertical_align(Align::Center),
    );
    if resp.changed() {
        d.fields.insert(key.into(), Value::String(s));
        changed = true;
    }
    if resp.has_focus() {
        let (up, down) = child
            .input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)));
        for (pressed, dir) in [(up, 1.0), (down, -1.0)] {
            if pressed {
                nudge(d, key, dir, step);
                changed = true;
            }
        }
    }
    changed
}

/// A captioned control in one of the details' two columns.
fn cell<R>(ui: &mut Ui, rtl: bool, caption: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    cell_w(ui, rtl, COLUMN_W, caption, add)
}

fn cell_w<R>(ui: &mut Ui, rtl: bool, width: f32, caption: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layout = Layout::top_down(if rtl { Align::Max } else { Align::Min });
    ui.allocate_ui_with_layout(vec2(width, 2.0 * FIELD_H), layout, |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 3.0;
        let t = Tokens::get(ui.ctx());
        crate::rtl::label(ui, egui::RichText::new(caption).size(11.5).color(t.text));
        add(ui)
    })
    .inner
}

/// A captioned checkbox; the caption toggles it too.
fn check_cell(ui: &mut Ui, rtl: bool, d: &mut Dialog, key: &str, caption: &str) {
    let layout = Layout::top_down(if rtl { Align::Max } else { Align::Min });
    ui.allocate_ui_with_layout(vec2(COLUMN_W, 2.0 * FIELD_H), layout, |ui| {
        ui.set_width(COLUMN_W);
        ui.spacing_mut().item_spacing.y = 3.0;
        let t = Tokens::get(ui.ctx());
        let mut on = d.b(key);
        let label = ui.add(egui::Label::new(crate::rtl::widget(ui, egui::RichText::new(caption).size(11.5).color(t.text))).sense(Sense::click()));
        let boxed = ui.add_sized(vec2(FIELD_H, FIELD_H), egui::Checkbox::without_text(&mut on));
        if label.clicked() {
            on = !on;
        }
        if label.clicked() || boxed.changed() {
            d.fields.insert(key.into(), json!(on));
        }
    });
}

/// Four edges (`{group}Top`…) beside a link that keeps them equal.
fn edges(ui: &mut Ui, lang: &str, d: &mut Dialog, group: &str) {
    let rtl = crate::i18n::is_rtl(lang);
    let linked_key = format!("{group}Linked");
    let linked = d.b(&linked_key);
    let facing = d.b("facingPages");
    let labels = if facing { EDGES } else { ["Top", "Bottom", "Left", "Right"] };
    let (mut changed, mut source) = (None, None);
    let link_w = 30.0;
    let column = ((ui.available_width() - link_w - 3.0 * ui.spacing().item_spacing.x) / 2.0).clamp(60.0, COLUMN_W);
    row(ui, rtl, |ui| {
        ui.vertical(|ui| {
            for pair in [[0, 1], [2, 3]] {
                row(ui, rtl, |ui| {
                    for i in pair {
                        let key = format!("{group}{}", EDGES[i]);
                        if cell_w(ui, rtl, column, crate::i18n::tr(lang, labels[i]), |ui| spin(ui, d, &key, column - 8.0, Step::Measure(0.0))) {
                            changed = Some(key);
                        }
                    }
                });
            }
        });
        ui.add_space(2.0);
        ui.vertical(|ui| {
            ui.add_space(FIELD_H + 12.0);
            let icon = if linked { "link" } else { "link-broken" };
            if crate::widgets::icon_toggle_sized(ui, icon, linked, crate::i18n::tr(lang, "Make all settings the same"), vec2(link_w, link_w))
                .clicked()
            {
                d.fields.insert(linked_key.clone(), json!(!linked));
                // Linking makes every edge the top's.
                if !linked {
                    source = Some(format!("{group}Top"));
                }
            }
        });
    });
    if linked && source.is_none() {
        source = changed;
    }
    if let Some(key) = source {
        link_edges(d, group, &key);
    }
}

/// Every edge of `group` takes the value of the field `from`.
fn link_edges(d: &mut Dialog, group: &str, from: &str) {
    if let Some(v) = d.fields.get(from).cloned() {
        for e in EDGES {
            d.fields.insert(format!("{group}{e}"), v.clone());
        }
    }
}

/// Show the measures in `u` (same lengths).
fn set_units(d: &mut Dialog, u: Unit) {
    let values: Vec<(String, Option<f64>)> = measure_keys().into_iter().map(|k| (k.clone(), measure(d, &k))).collect();
    d.fields.insert("units".into(), json!(u));
    for (k, pt) in values {
        if let Some(pt) = pt {
            d.fields.insert(k, json!(format_measure(pt, u)));
        }
    }
}

fn section_rule(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, t.divider));
    ui.add_space(6.0);
}

fn details(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    let rtl = crate::i18n::is_rtl(&lang);
    ui.spacing_mut().item_spacing.y = 8.0;
    row(ui, rtl, |ui| {
        crate::rtl::label(ui, egui::RichText::new(tr("Preset Details").to_uppercase()).font(semibold(11.5)).color(t.text_dim));
    });
    // Document name, and Save Document Preset.
    row(ui, rtl, |ui| {
        let mut name = d.s("name");
        let hint = app.session.next_untitled_title();
        let resp = ui.add(egui::TextEdit::singleline(&mut name).hint_text(hint).desired_width(2.0 * COLUMN_W - 10.0).font(semibold(13.0)));
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr("Document Name")));
        if resp.changed() {
            d.fields.insert("name".into(), json!(name));
        }
        let save = crate::widgets::icon_toggle_sized(ui, "preset-save", false, tr("Save Document Preset"), vec2(30.0, 26.0));
        save.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr("Save Document Preset")));
        // It holds a text field: clicks inside it don't close it.
        egui::Popup::menu(&save).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(220.0);
            crate::rtl::label(ui, tr("Save Document Preset As:"));
            let mut preset = d.s("presetName");
            let field = ui.add(egui::TextEdit::singleline(&mut preset).hint_text(tr("Custom")).desired_width(200.0));
            field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tr("Preset Name")));
            // Type the name as soon as the popup opens.
            if save.clicked() {
                field.request_focus();
            }
            if field.changed() {
                d.fields.insert("presetName".into(), json!(preset));
            }
            // Enter in the name saves the preset (the dialog ignores keys while a popup is open).
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button(crate::rtl::widget(ui, tr("Save Preset"))).clicked() || enter {
                let name = if preset.trim().is_empty() { tr("Custom").to_string() } else { preset };
                match save_preset(app, d, &name) {
                    Ok(()) => ui.close(),
                    // Refused (a built-in name…): stay open to be corrected.
                    Err(e) => app.status(e),
                }
            }
        });
    });
    let u = units(d);
    row(ui, rtl, |ui| {
        cell(ui, rtl, tr("Width"), |ui| spin(ui, d, "width", COLUMN_W - 8.0, Step::Measure(1.0)));
        cell(ui, rtl, tr("Units"), |ui| {
            egui::ComboBox::from_id_salt("nd_units").selected_text(crate::rtl::widget(ui, tr(u.label()))).width(COLUMN_W - 8.0).show_ui(ui, |ui| {
                for v in Unit::ALL {
                    if ui.selectable_label(v == u, crate::rtl::widget(ui, tr(v.label()))).clicked() && v != u {
                        set_units(d, v);
                    }
                }
            });
        });
    });
    row(ui, rtl, |ui| {
        cell(ui, rtl, tr("Height"), |ui| spin(ui, d, "height", COLUMN_W - 8.0, Step::Measure(1.0)));
        cell(ui, rtl, tr("Orientation"), |ui| {
            let (w, h) = (measure(d, "width"), measure(d, "height"));
            let portrait = !matches!((w, h), (Some(w), Some(h)) if w > h);
            row(ui, rtl, |ui| {
                for (icon, tip, on) in [("orient-portrait", "Portrait", portrait), ("orient-landscape", "Landscape", !portrait)] {
                    if crate::widgets::icon_toggle_sized(ui, icon, on, tr(tip), vec2(30.0, FIELD_H)).clicked() && !on {
                        let (w, h) = (d.fields.get("width").cloned(), d.fields.get("height").cloned());
                        if let (Some(w), Some(h)) = (w, h) {
                            d.fields.insert("width".into(), h);
                            d.fields.insert("height".into(), w);
                        }
                    }
                }
            });
        });
    });
    row(ui, rtl, |ui| {
        cell(ui, rtl, tr("Pages"), |ui| spin(ui, d, "pages", COLUMN_W - 8.0, Step::Count));
        check_cell(ui, rtl, d, "facingPages", tr("Facing Pages"));
    });
    row(ui, rtl, |ui| {
        cell(ui, rtl, tr("Start #"), |ui| spin(ui, d, "startPage", COLUMN_W - 8.0, Step::Count));
        check_cell(ui, rtl, d, "primaryTextFrame", tr("Primary Text Frame"));
    });
    section_rule(ui);
    row(ui, rtl, |ui| {
        cell(ui, rtl, tr("Columns"), |ui| spin(ui, d, "columns", COLUMN_W - 8.0, Step::Count));
        cell(ui, rtl, tr("Column Gutter"), |ui| spin(ui, d, "gutter", COLUMN_W - 8.0, Step::Measure(0.0)));
    });
    section_rule(ui);
    egui::CollapsingHeader::new(crate::rtl::widget(ui, egui::RichText::new(tr("Margins")).size(12.0).color(t.text)))
        .id_salt("nd_margins")
        .default_open(true)
        .show(ui, |ui| edges(ui, &lang, d, "margin"));
    section_rule(ui);
    egui::CollapsingHeader::new(crate::rtl::widget(ui, egui::RichText::new(tr("Bleed and Slug")).size(12.0).color(t.text)))
        .id_salt("nd_bleed_slug")
        .default_open(true)
        .show(ui, |ui| {
            row(ui, rtl, |ui| {
                crate::rtl::label(ui, egui::RichText::new(tr("Bleed")).size(11.5).color(t.text_dim));
            });
            edges(ui, &lang, d, "bleed");
            row(ui, rtl, |ui| {
                crate::rtl::label(ui, egui::RichText::new(tr("Slug")).size(11.5).color(t.text_dim));
            });
            edges(ui, &lang, d, "slug");
        });
}

/// The window's body; true when a card was double-clicked (create the document now).
pub(crate) fn body(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog, width: f32, height: f32) -> bool {
    let t = Tokens::get(ui.ctx());
    let rtl = crate::i18n::is_rtl(&app.ui.language);
    let mut create = false;
    if width < TWO_PANES {
        // A small window: the details first, then the presets; the dialog body scrolls.
        ui.set_max_width(DETAILS_W.max(width));
        details(app, ui, d);
        ui.add_space(14.0);
        tabs(app, ui, d);
        ui.add_space(12.0);
        return presets(app, ui, d);
    }
    let pane_h = height.clamp(FIELD_H, 560.0);
    let presets_w = (width - DETAILS_W - 16.0).max(CARD.x + 24.0);
    row(ui, rtl, |ui| {
        ui.allocate_ui_with_layout(vec2(presets_w, pane_h), Layout::top_down(if rtl { Align::Max } else { Align::Min }), |ui| {
            ui.set_min_size(vec2(presets_w, pane_h));
            tabs(app, ui, d);
            ui.add_space(12.0);
            let rest = ui.available_height().max(FIELD_H);
            egui::Frame::NONE.fill(t.panel_darker).corner_radius(4.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_min_size(vec2(presets_w - 24.0, rest - 24.0));
                egui::ScrollArea::vertical().id_salt("nd_presets").max_height(rest - 24.0).auto_shrink([false, false]).show(ui, |ui| {
                    create = presets(app, ui, d);
                });
            });
        });
        ui.add_space(16.0);
        ui.allocate_ui_with_layout(vec2(DETAILS_W, pane_h), Layout::top_down(if rtl { Align::Max } else { Align::Min }), |ui| {
            ui.set_min_size(vec2(DETAILS_W, pane_h));
            egui::ScrollArea::vertical().id_salt("nd_details").max_height(pane_h).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(DETAILS_W - 12.0);
                details(app, ui, d);
            });
        });
    });
    create
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("app.newDocumentDialog", json!({})).unwrap();
        app
    }

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        crate::theme::apply(&ctx, &Tokens::for_brightness(crate::theme::Brightness::default()));
        ctx
    }

    /// One frame of the open dialog → the painted texts and where.
    fn frame(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<(String, Rect)> {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 900.0))),
            time: Some(ctx.input(|i| i.time) + 1.0 / 60.0),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| crate::dialogs::show(app, ui.ctx()));
        out.textures_delta.clear();
        fn collect(shape: &egui::Shape, texts: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Text(t) => texts.push((t.galley.job.text.clone(), t.visual_bounding_rect())),
                egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, texts)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        out.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
        texts
    }

    fn settle(app: &mut DesignApp, ctx: &egui::Context) -> Vec<(String, Rect)> {
        for _ in 0..3 {
            frame(app, ctx, vec![]);
        }
        frame(app, ctx, vec![])
    }

    fn click(app: &mut DesignApp, ctx: &egui::Context, pos: egui::Pos2, times: usize) {
        frame(app, ctx, vec![egui::Event::PointerMoved(pos)]);
        for _ in 0..times {
            for pressed in [true, false] {
                frame(
                    app,
                    ctx,
                    vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }],
                );
            }
        }
    }

    /// The card whose name is `name` (its page, above the name).
    fn card_at(texts: &[(String, Rect)], name: &str) -> egui::Pos2 {
        let (_, r) = texts.iter().find(|(t, _)| t == name).unwrap_or_else(|| panic!("card {name}"));
        r.center() - vec2(0.0, 50.0)
    }

    #[test]
    fn a_print_preset_fills_the_details_and_creates_its_document() {
        let mut app = app();
        let ctx = context();
        app.ui.dialog.as_mut().unwrap().fields.insert("tab".into(), json!("print"));
        let texts = settle(&mut app, &ctx);
        click(&mut app, &ctx, card_at(&texts, "A4"), 1);
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.s("preset"), d.s("width"), d.s("height"), units(d)), ("A4".into(), "210 mm".into(), "297 mm".into(), Unit::Millimeters));
        let d = app.ui.dialog.as_mut().unwrap();
        for (k, v) in [("startPage", json!(3)), ("bleedTop", json!("3 mm")), ("slugBottom", json!("10 mm")), ("name", json!("Flyer"))] {
            d.fields.insert(k.into(), v);
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let doc = &app.session.active().unwrap().doc;
        assert_eq!(doc.title, "Flyer");
        assert!((doc.settings.page_width - 210.0 * PT_PER_MM).abs() < 1e-6);
        assert_eq!(doc.settings.horizontal_units, Unit::Millimeters);
        assert_eq!(doc.page_name(0), "3");
        assert!((doc.settings.bleed[0] - 3.0 * PT_PER_MM).abs() < 1e-6 && doc.settings.bleed[1] == 0.0);
        assert!((doc.settings.slug[1] - 10.0 * PT_PER_MM).abs() < 1e-6);
        // The Recent tab now shows it, by its preset's name.
        let recent = cards(&app, "recent");
        assert_eq!(recent.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), [DEFAULT_CARD, "A4"]);
        assert_eq!(recent[1].nd.start_page, 3);
    }

    #[test]
    fn double_clicking_a_card_creates_the_document() {
        let mut app = app();
        let ctx = context();
        app.ui.dialog.as_mut().unwrap().fields.insert("tab".into(), json!("web"));
        let texts = settle(&mut app, &ctx);
        click(&mut app, &ctx, card_at(&texts, "Web 1366 \u{d7} 768"), 2);
        assert!(app.ui.dialog.is_none(), "the dialog closed");
        let doc = &app.session.active().unwrap().doc;
        assert_eq!((doc.settings.page_width, doc.settings.intent), (1366.0, Intent::Web));
        assert!(!doc.settings.facing_pages);
    }

    #[test]
    fn units_reformat_the_measures_and_linked_edges_follow() {
        let mut d = Dialog::new("newDocument", json!({}));
        set_units(&mut d, Unit::Millimeters);
        assert_eq!((d.s("width"), d.s("marginTop")), ("215.9 mm".into(), "12.7 mm".into()));
        d.fields.insert("marginInside".into(), json!("20 mm"));
        link_edges(&mut d, "margin", "marginInside");
        assert!(EDGES.iter().all(|e| d.s(&format!("margin{e}")) == "20 mm"));
        let p = params(&d);
        assert!((p["margins"]["outside"].as_f64().unwrap() - 20.0 * PT_PER_MM).abs() < 1e-9);
        // Typed nonsense falls back to the defaults instead of reaching the document.
        d.fields.insert("gutter".into(), json!("wide"));
        d.fields.insert("pages".into(), json!("-4"));
        let p = params(&d);
        assert_eq!((p["gutter"].as_f64(), p["pages"].as_u64()), (Some(12.0), Some(1)));
    }

    /// The app's window (with a document) with the New Document dialog and its Save Document Preset
    /// popup open.
    fn window_with_preset_popup() -> egui_kittest::Harness<'static, crate::test_window::Window> {
        use egui_kittest::kittest::Queryable as _;
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("app.newDocumentDialog", json!({})).unwrap();
        let mut h = crate::test_window::open(app, vec2(1440.0, 900.0));
        // The dialog takes a couple of frames to settle where it opens.
        h.run_steps(4);
        let button = h.get_by_label("Save Document Preset").rect().center();
        crate::test_window::click_at(&mut h, button);
        assert!(egui::Popup::is_any_open(&h.ctx), "the popup opened");
        h
    }

    #[test]
    fn enter_in_the_preset_popup_saves_the_preset_and_keeps_the_dialog() {
        use egui_kittest::kittest::Queryable as _;
        let mut h = window_with_preset_popup();
        let field = h.get_by_label("Preset Name").rect().center();
        crate::test_window::click_at(&mut h, field);
        h.get_by_label("Preset Name").type_text("Brochure");
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(4);
        let app = &mut h.state_mut().app;
        let d = app.ui.dialog.as_ref().expect("Enter saves the preset; it doesn't create the document");
        assert_eq!(app.session.documents().len(), 1, "no document was made (the window opens with one)");
        assert_eq!(app.session.prefs.document_presets.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Brochure"]);
        assert_eq!((d.s("tab"), d.s("preset")), ("saved".into(), "saved:Brochure".into()));
        // The saved preset makes documents by name, as file.new {preset} does from anywhere.
        app.run("file.new", json!({"preset": "Brochure"})).unwrap();
        assert!(!egui::Popup::is_any_open(&h.ctx), "the popup closed");
    }

    #[test]
    fn escape_in_the_preset_popup_closes_only_the_popup() {
        let mut h = window_with_preset_popup();
        h.key_press(egui::Key::Escape);
        h.run_steps(4);
        assert!(!egui::Popup::is_any_open(&h.ctx), "Escape closed the popup");
        assert!(h.state().app.ui.dialog.is_some(), "and left the dialog open");
        // With no popup open, Escape closes the dialog as before.
        h.key_press(egui::Key::Escape);
        h.run_steps(4);
        assert!(h.state().app.ui.dialog.is_none());
    }

    #[test]
    fn presets_are_saved_by_name_and_recent_documents_once() {
        let mut app = app();
        let mut d = app.ui.dialog.clone().unwrap();
        save_preset(&mut app, &mut d, " Newsletter ").unwrap();
        d.fields.insert("pages".into(), json!(8));
        save_preset(&mut app, &mut d, "Newsletter").unwrap();
        assert_eq!(app.session.prefs.document_presets.len(), 1, "the same name replaces the preset");
        assert!(save_preset(&mut app, &mut d, "Letter").is_err(), "a built-in name is refused");
        assert_eq!((d.s("tab"), d.s("preset")), ("saved".into(), "saved:Newsletter".into()));
        let saved = cards(&app, "saved");
        assert_eq!((saved.len(), saved[0].label.as_str(), saved[0].nd.pages), (1, "Newsletter", 8));
        let p = params(&d);
        remember(&mut app, &p);
        remember(&mut app, &p);
        assert_eq!(app.ui.recent_new_documents.len(), 1, "a repeat moves to the front");
    }
}
