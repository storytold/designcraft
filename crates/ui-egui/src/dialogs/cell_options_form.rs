//! Table › Cell Options on the craft framework's dialog host: Strokes and Fills (`cellOptions`) and Diagonal Lines
//! (`diagonalLines`). The form edits the same `Dialog` draft fields the native body does, with the same edge reseeding,
//! so OK and Cancel go through the existing `confirm_cell_options` and `confirm_diagonal_lines`.

use super::Dialog;
use super::framework::Form;
use crate::DesignApp;
use crate::panels::cell_stroke::{self, framework as stroke};
use craft_behavior::Mixed;
use craft_ui_panels::sections::GroupBox;
use craft_ui_widgets::choice::Segmented;
use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _};
use craft_ui_widgets::form::{Field, FieldGroup};
use craft_ui_widgets::layout_components::Stack;
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::primitives::{Glyph, IconSpec};
use craft_ui_widgets::style::UiTokens;
use egui::{Color32, Rect, Ui};
use serde_json::{Value, json};

pub(crate) const FORM: Form = Form::new(show);

fn show(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    if d.id == "diagonalLines" {
        diagonal_lines(app, ui, d);
    } else {
        cell_options(app, ui, d);
    }
    None
}

/// Strokes and Fills: the edge proxy and the stroke fields in Cell Stroke, then the cell fill.
fn cell_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    Stack::new().id_salt("cell_options_form").fill_width().show(ui, |ui, _| {
        GroupBox::new(tr("Cell Stroke"), None).id_salt("cell_options_stroke").show(ui, |ui, _| {
            let mut edges = cell_stroke::edges_from(d.fields.get("edges").unwrap_or(&Value::Null));
            if stroke::edge_proxy(ui, "cell_options", &mut edges, &lang) {
                super::cell_options_edges_changed(app, d, &edges);
            }
            let values = Value::Object(d.fields.clone());
            if let Some((k, v)) = stroke::stroke_fields(app, ui, "cell_options", &values, edges.iter().any(|on| *on)) {
                d.fields.insert(k.into(), v);
            }
        });
        GroupBox::new(tr("Cell Fill"), None).id_salt("cell_options_fill").show(ui, |ui, _| {
            FieldGroup::new(tr("Cell Fill")).id_salt("cell_fill_fields").show(ui, |ui, _| {
                Field::new(tr("Color:")).id_salt("cell_fill_color_field").show(ui, |ui, _| {
                    let current = d.fields.get("fill").and_then(Value::as_str).map(str::to_owned);
                    if let Some(name) = stroke::swatch_select(app, ui, "cell_fill_color", tr("Color"), current.as_deref()) {
                        d.fields.insert("fill".into(), json!(name));
                    }
                });
                Field::new(tr("Tint:")).id_salt("cell_fill_tint_field").show(ui, |ui, _| {
                    let tint = d.fields.get("fillTint").and_then(Value::as_f64);
                    if let Some(t) = stroke::percent_field(ui, "cell_fill_tint", tr("Tint"), tint) {
                        d.fields.insert("fillTint".into(), json!(t / 100.0));
                    }
                });
            });
        });
    });
}

/// The cell and diagonal a Diagonal Lines segment shows, by its `diagonal.lines` name.
#[derive(craft_ui::contract::MemoProps)]
struct DiagonalGlyph(&'static str);

impl Glyph for DiagonalGlyph {
    fn paint(&self, painter: &egui::Painter, tokens: &UiTokens, rect: Rect, color: Color32, _mirrored: bool) {
        let ink = egui::Stroke::new(tokens.stroke.thin, color);
        painter.rect_stroke(rect, 0.0, ink, egui::StrokeKind::Inside);
        if matches!(self.0, "topLeft" | "crossed") {
            painter.line_segment([rect.left_top(), rect.right_bottom()], ink);
        }
        if matches!(self.0, "topRight" | "crossed") {
            painter.line_segment([rect.right_top(), rect.left_bottom()], ink);
        }
    }
}

/// The glyphs of the line segments, in `DIAGONAL_LINES` order.
static DIAGONALS: [DiagonalGlyph; 4] = [DiagonalGlyph("none"), DiagonalGlyph("topLeft"), DiagonalGlyph("topRight"), DiagonalGlyph("crossed")];

/// Diagonal Lines: the four line choices, the Line Stroke fields and Draw.
fn diagonal_lines(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    Stack::new().id_salt("diagonal_lines_form").fill_width().show(ui, |ui, _| {
        let entries: Vec<Entry<&'static str>> = super::DIAGONAL_LINES
            .iter()
            .zip(&DIAGONALS)
            .map(|(&(name, label), glyph)| Entry::new(name, tr(label).to_owned()).icon(IconSpec::glyph(glyph)))
            .collect();
        let held = d
            .fields
            .get("lines")
            .and_then(Value::as_str)
            .and_then(|held| super::DIAGONAL_LINES.iter().map(|(name, _)| *name).find(|name| *name == held));
        let lines = Segmented::new(tr("Diagonal Lines"), Bind::given(held.map_or(Mixed::Mixed, Mixed::Same)), &entries)
            .id_salt("diagonal_lines_kind")
            .show(ui);
        if let Some(name) = lines.events.committed() {
            d.fields.insert("lines".into(), json!(name));
        }
        GroupBox::new(tr("Line Stroke"), None).id_salt("diagonal_line_stroke").show(ui, |ui, _| {
            let values = Value::Object(d.fields.clone());
            if let Some((k, v)) = stroke::stroke_fields(app, ui, "diagonal_lines", &values, true) {
                d.fields.insert(k.into(), v);
            }
        });
        FieldGroup::new(tr("Draw:")).id_salt("diagonal_draw_fields").show(ui, |ui, _| {
            Field::new(tr("Draw:")).id_salt("diagonal_draw_field").show(ui, |ui, _| {
                let orders = [Entry::new(true, tr("Diagonal In Front").to_owned()), Entry::new(false, tr("Content In Front").to_owned())];
                let front = d.fields.get("inFront").and_then(Value::as_bool).map_or(Mixed::Mixed, Mixed::Same);
                let order = Select::new(tr("Draw"), Bind::given(front), &orders).id_salt("diagonal_draw_order").show(ui);
                if let Some(front) = order.events.committed() {
                    d.fields.insert("inFront".into(), json!(front));
                }
            });
        });
    });
}
