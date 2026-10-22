//! Cell strokes: the edge proxy (a drawn 2 × 2 cell grid whose lines choose the cell edges a change
//! applies to) and the stroke fields shared by the Stroke panel and Cell Options › Strokes and Fills.
//!
//! The proxy's lines are the target range's top, bottom, left and right sides and the lines
//! between its rows (inner horizontal) and columns (inner vertical). A click toggles a line; a
//! double-click on a line chooses all outer or all inner lines; a triple-click chooses all lines,
//! or none when all were chosen.

use designcraft_doc::StrokeType;
use egui::{Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::i18n::tr;
use crate::theme::Tokens;
use crate::widgets::{NumField, caption};

/// Edge names of the proxy lines, in proxy order; `table.setCell` takes them as `stroke.edges`.
pub const EDGE_NAMES: [&str; 6] = ["top", "bottom", "left", "right", "innerHorizontal", "innerVertical"];

/// Proxy line choice, in [`EDGE_NAMES`] order.
pub type Edges = [bool; 6];

pub const PROXY_SIZE: f32 = 64.0;
const PROXY_INSET: f32 = 8.0;

const WEIGHTS: &[f64] = &[0.0, 0.25, 0.5, 0.75, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 20.0, 30.0, 40.0, 50.0, 100.0];

/// Stroke types offered for cell edges (all of them draw on table edges).
const TYPES: &[&str] =
    &["solid", "dashed", "dotted", "thickThin", "thinThick", "thinThin", "thickThick", "thinThickThin", "thickThinThick", "wavy", "hashed"];

/// Whether the selection is table cells (or the text cursor is in a cell).
pub fn in_cells(app: &DesignApp) -> bool {
    app.session.active().is_some_and(|d| d.selection.cells.is_some() || d.selection.text.is_some_and(|t| t.cell.is_some()))
}

/// The chosen edges as the `edges` parameter.
pub fn edges_param(edges: &Edges) -> Value {
    json!(EDGE_NAMES.iter().zip(edges).filter(|(_, on)| **on).map(|(name, _)| *name).collect::<Vec<_>>())
}

/// Edges from an `edges` parameter (names); anything else chooses all.
pub fn edges_from(v: &Value) -> Edges {
    match v.as_array() {
        Some(names) => std::array::from_fn(|i| names.iter().any(|n| n.as_str() == EDGE_NAMES.get(i).copied())),
        None => [true; 6],
    }
}

/// The proxy's lines (start, end) inside its rect, in [`EDGE_NAMES`] order.
pub fn proxy_lines(r: Rect) -> [(Pos2, Pos2); 6] {
    let i = r.shrink(PROXY_INSET);
    let c = i.center();
    [
        (i.left_top(), i.right_top()),
        (i.left_bottom(), i.right_bottom()),
        (i.left_top(), i.left_bottom()),
        (i.right_top(), i.right_bottom()),
        (pos2(i.left(), c.y), pos2(i.right(), c.y)),
        (pos2(c.x, i.top()), pos2(c.x, i.bottom())),
    ]
}

/// The line under `p`, if any is within reach.
fn line_at(r: Rect, p: Pos2) -> Option<usize> {
    proxy_lines(r)
        .iter()
        .enumerate()
        .map(|(i, (a, b))| {
            let t = if a == b { 0.0 } else { ((p - *a).dot(*b - *a) / (*b - *a).length_sq()).clamp(0.0, 1.0) };
            (i, p.distance(*a + (*b - *a) * t))
        })
        .filter(|(_, d)| *d <= PROXY_INSET - 1.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// A click on the proxy.
struct ProxyClick {
    /// The choice before the first click of this click sequence.
    start: Edges,
    /// The line clicked, and the line of the click before.
    hit: Option<usize>,
    last: Option<usize>,
    double: bool,
    triple: bool,
}

/// Apply a proxy click to `edges`. Returns whether the choice changed.
fn proxy_click(edges: &mut Edges, click: ProxyClick) -> bool {
    if click.triple {
        let all = click.start.iter().all(|on| *on);
        *edges = [!all; 6];
        return true;
    }
    let Some(h) = click.hit else { return false };
    if click.double && click.last == Some(h) {
        let outer = h < 4;
        for (i, on) in edges.iter_mut().enumerate() {
            *on = (i < 4) == outer;
        }
    } else if let Some(on) = edges.get_mut(h) {
        *on = !*on;
    }
    true
}

/// The edge proxy. Returns whether the choice changed.
pub fn edge_proxy(ui: &mut Ui, id: &str, edges: &mut Edges, lang: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(PROXY_SIZE, PROXY_SIZE), Sense::click());
    let label = tr(lang, "Cell edges").to_string();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
    let resp = resp.on_hover_text(tr(
        lang,
        "Click a line to include or exclude it. Double-click selects all outer or all inner lines; triple-click selects or clears all.",
    ));
    let hit = resp.interact_pointer_pos().or_else(|| resp.hover_pos()).and_then(|p| line_at(r, p));
    let mut changed = false;
    if resp.clicked() {
        let (double, triple) = (resp.double_clicked(), resp.triple_clicked());
        // A double-click counts only on the line of the click before it; a triple-click
        // decides from the choice before the sequence's first click.
        let last_key = ui.id().with((id, "proxy_last"));
        let start_key = ui.id().with((id, "proxy_start"));
        let last: Option<usize> = ui.data(|d| d.get_temp(last_key)).flatten();
        if !double && !triple {
            ui.data_mut(|d| d.insert_temp(start_key, *edges));
        }
        let start: Edges = ui.data(|d| d.get_temp(start_key)).unwrap_or(*edges);
        changed = proxy_click(edges, ProxyClick { start, hit, last, double, triple });
        ui.data_mut(|d| d.insert_temp(last_key, hit));
    }
    let p = ui.painter();
    p.rect_filled(r, 0.0, t.input);
    p.rect_filled(r.shrink(PROXY_INSET), 0.0, egui::Color32::from_gray(0xf2));
    p.rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    for (i, (a, b)) in proxy_lines(r).into_iter().enumerate() {
        let on = edges.get(i).copied().unwrap_or(false);
        let w = if hit == Some(i) {
            3.0
        } else if on {
            2.5
        } else {
            1.0
        };
        p.line_segment([a, b], Stroke::new(w, if on { t.accent } else { egui::Color32::from_gray(0x9a) }));
    }
    changed
}

/// A built-in type by its `type` parameter name.
fn builtin(kind: &str) -> StrokeType {
    match kind {
        "dashed" => StrokeType::Dashed { pattern: Vec::new() },
        "dotted" => StrokeType::Dotted,
        "thickThin" => StrokeType::ThickThin,
        "thinThick" => StrokeType::ThinThick,
        "thinThin" => StrokeType::ThinThin,
        "thickThick" => StrokeType::ThickThick,
        "thinThickThin" => StrokeType::ThinThickThin,
        "thickThinThick" => StrokeType::ThickThinThick,
        "wavy" => StrokeType::Wavy,
        "hashed" => StrokeType::Hashed,
        _ => StrokeType::Solid,
    }
}

/// The type's menu label (named styles by name).
fn type_label(lang: &str, v: &Value) -> String {
    match serde_json::from_value::<StrokeType>(v.clone()) {
        Ok(StrokeType::Style { name }) => name,
        Ok(k) => tr(lang, k.label()).to_string(),
        Err(_) => String::new(),
    }
}

/// Weight, type, colour, tint, gap colour and gap tint for the stroke attributes in `v`
/// (`table.getCellStroke` keys; null shows as mixed). Returns the attribute the user changed.
pub fn stroke_fields(app: &mut DesignApp, ui: &mut Ui, id: &str, v: &Value) -> Option<(&'static str, Value)> {
    let lang = app.ui.language.clone();
    let custom: Vec<String> = app.session.active().map(|d| d.doc.stroke_styles.iter().map(|x| x.name.clone()).collect()).unwrap_or_default();
    let mut out = None;
    let percent = |key: &str| v[key].as_f64().map(|t| (t * 100.0).round());
    egui::Grid::new((id, "cell_stroke")).num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        caption(ui, tr(&lang, "Weight:"));
        let wid = format!("{id}_weight");
        if let Some(w) = NumField::number(&wid, v["weight"].as_f64(), " pt", 3).width(86.0).spinner().presets(WEIGHTS).range(0.0, 1000.0).show(ui) {
            out = Some(("weight", json!(w)));
        }
        ui.end_row();
        caption(ui, tr(&lang, "Type:"));
        let shown = type_label(&lang, &v["type"]);
        egui::ComboBox::from_id_salt((id, "type")).selected_text(crate::rtl::widget(ui, &shown)).width(140.0).show_ui(ui, |ui| {
            for kind in TYPES {
                let text = tr(&lang, builtin(kind).label());
                if ui.selectable_label(shown == text, crate::rtl::widget(ui, text)).clicked() {
                    out = Some(("type", json!(kind)));
                }
            }
            if !custom.is_empty() {
                ui.separator();
            }
            for n in &custom {
                if ui.selectable_label(shown == *n, n).clicked() {
                    out = Some(("type", json!({"kind": "style", "name": n})));
                }
            }
        });
        ui.end_row();
        for (label, key, tint_label, tint_key) in [("Color:", "color", "Tint:", "tint"), ("Gap Color:", "gapColor", "Gap Tint:", "gapTint")] {
            caption(ui, tr(&lang, label));
            let mut picked = None;
            super::swatch_picker(app, ui, &format!("{id}_{key}"), v[key].as_str().map(str::to_string), |_, n| picked = Some(n));
            if let Some(n) = picked {
                out = Some((key, json!(n)));
            }
            ui.end_row();
            caption(ui, tr(&lang, tint_label));
            let tid = format!("{id}_{tint_key}");
            if let Some(t) = NumField::number(&tid, percent(tint_key), "%", 0).width(60.0).range(0.0, 100.0).show(ui) {
                out = Some((tint_key, json!(t / 100.0)));
            }
            ui.end_row();
        }
    });
    out
}

/// The shown stroke attributes of `edges` of the target cells (null where they differ).
pub fn current(app: &mut DesignApp, edges: &Edges) -> Value {
    if !edges.iter().any(|on| *on) {
        return Value::Null;
    }
    app.session.execute("table.getCellStroke", &json!({"edges": edges_param(edges)})).unwrap_or(Value::Null)
}

/// The Stroke panel for table cells: the proxy and the stroke fields, applied at once.
pub fn panel(app: &mut DesignApp, ui: &mut Ui) {
    let key = egui::Id::new("cell_stroke_edges");
    let mut edges: Edges = ui.data(|d| d.get_temp(key)).unwrap_or([true; 6]);
    let lang = app.ui.language.clone();
    if edge_proxy(ui, "stroke_panel", &mut edges, &lang) {
        ui.data_mut(|d| d.insert_temp(key, edges));
    }
    ui.add_space(6.0);
    let values = current(app, &edges);
    let any = edges.iter().any(|on| *on);
    let changed = ui.add_enabled_ui(any, |ui| stroke_fields(app, ui, "csp", &values)).inner;
    if let Some((k, v)) = changed
        && let Err(e) = app.run("table.setCell", json!({"stroke": {k: v, "edges": edges_param(&edges)}}))
    {
        app.status(format!("Stroke: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::{Harness, kittest::Queryable};

    use super::*;

    #[test]
    fn proxy_triple_click_clears_all_lines_when_all_were_chosen_and_chooses_all_otherwise() {
        for (before, after) in [([true; 6], [false; 6]), ([true, false, true, false, false, false], [true; 6]), ([false; 6], [true; 6])] {
            let mut edges = before;
            let click = |hit, double, triple| ProxyClick { start: before, hit, last: Some(0), double, triple };
            proxy_click(&mut edges, click(Some(0), false, false));
            proxy_click(&mut edges, click(Some(0), true, false));
            proxy_click(&mut edges, click(Some(0), false, true));
            assert_eq!(edges, after, "triple-click from {before:?}");
        }
    }

    #[test]
    fn stroke_panel_proxy_limits_a_weight_change_to_the_chosen_cell_edges() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
        let tid = app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        app.run("table.select", json!({"what": "table"})).unwrap();
        let mut h = Harness::builder().with_size(vec2(320.0, 600.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::new("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                crate::panels::properties::stroke_panel(app, ui);
            },
            app,
        );
        h.run_steps(3);
        // Leave only the inner horizontal line chosen: click the others off.
        let lines = proxy_lines(h.get_by_label("Cell edges").rect());
        for i in [0, 1, 2, 3, 5] {
            let (a, b) = lines[i];
            let at = a + (b - a) * 0.3;
            h.hover_at(at);
            h.drag_at(at);
            h.drop_at(at);
            h.run_steps(3);
        }
        let weight = h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("1 pt")).rect();
        h.hover_at(weight.center());
        h.drag_at(weight.center());
        h.drop_at(weight.center());
        h.run_steps(2);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("4".into()));
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        let st = h.state().session.active().unwrap();
        let t = st.doc.stories.values().find_map(|s| s.tables.get(&tid)).unwrap();
        let w = |r: usize, c: usize, side: usize| t.cell(r, c).unwrap().strokes[side].weight;
        assert_eq!([w(0, 0, 2), w(0, 1, 2), w(1, 0, 0), w(1, 1, 0)], [4.0; 4], "inner horizontal edges");
        assert_eq!([w(0, 0, 0), w(0, 0, 1), w(0, 0, 3), w(0, 1, 1), w(1, 1, 2), w(1, 1, 3)], [1.0; 6], "the other edges keep their weight");
    }
}
