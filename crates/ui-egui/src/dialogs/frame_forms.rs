//! The framework forms of the frame dialogs: Text Frame Options, Frame Fitting Options, Polygon Settings, Generate QR
//! Code and Object Layer Options. Each form edits the draft the native window edits and leaves OK, Cancel, the preview and
//! every command to the existing controller, so documents, undo and saved settings are what they were.
//!
//! The measure fields keep the draft as text in the document's unit: the framework's number field reads decimal picas
//! only, not `1p6`, and has no kind for ciceros, agates, Q or Ha, so a measure is a plain text field whose committed text
//! is resolved with `parse_measure` and written back with `format_measure`, as the native spinner fields do.

use super::Dialog;
use super::framework::Form;
use crate::DesignApp;
use craft_behavior::{Anchor9, EditEvent, Mixed};
use craft_ui_panels::sections::GroupBox;
use craft_ui_widgets::buttons::Button;
use craft_ui_widgets::choice::{Checkbox, Radio, RadioGroup};
use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _};
use craft_ui_widgets::display::{Shade, Text};
use craft_ui_widgets::form::{Field, FieldGroup};
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::pickers::{ReferencePoint, Select};
use craft_ui_widgets::text::{NumberField, TextField, link_toggle};
use designcraft_geom::{format_measure, units::parse_measure};
use egui::Ui;
use serde_json::{Value, json};

/// Text Frame Options, with its live preview.
pub(crate) const TEXT_FRAME_OPTIONS: Form = Form::new(text_frame_options);
/// Frame Fitting Options.
pub(crate) const FITTING_OPTIONS: Form = Form::new(fitting_options);
/// Polygon Settings.
pub(crate) const POLYGON_SETTINGS: Form = Form::new(polygon_settings);
/// Generate QR Code.
pub(crate) const QR_CODE: Form = Form::new(qr_code);
/// Object Layer Options (the layers of a placed PDF).
pub(crate) const LAYER_OPTIONS: Form = Form::new(layer_options);

/// What a spinner measure field holds: a length from nothing to the largest page.
const MEASURE_RANGE: (f64, f64) = (0.0, 15552.0);

/// The sides of the inset, in the order the dialog's fields run.
const SIDES: [&str; 4] = ["insetTop", "insetLeft", "insetBottom", "insetRight"];

// ---------------------------------------------------------------------------------------------------------------------
// Controls over the draft

/// `body` under the side label `label`, which names it.
pub(super) fn field(ui: &mut Ui, label: &str, salt: &str, mut body: impl FnMut(&mut Ui)) {
    Field::new(label.to_owned()).id_salt(("frame_form_field", salt.to_owned())).show(ui, |ui, _| body(ui));
}

/// A text field over the draft text of `key`. Every edit is written to the draft as it is typed, so OK always sees what
/// is on screen. Returns the text before and after an edit that ended in this pass.
pub(super) fn text(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, enabled: bool) -> Option<(Option<String>, String)> {
    let out =
        TextField::new(label.to_owned(), Bind::given(d.s(key))).id_salt(("frame_form_text", key)).limit(None::<usize>).enabled(enabled).show(ui);
    if let Some(next) = out.events.landed().filter(|next| **next != d.s(key)).cloned() {
        d.fields.insert(key.into(), json!(next));
    }
    match out.events.ended() {
        Some(EditEvent::Commit { before, after, .. } | EditEvent::Set { before, after, .. }) => {
            Some((if let Mixed::Same(before) = before { Some(before.clone()) } else { None }, after.clone()))
        }
        _ => None,
    }
}

/// A measure field: the committed text is worked out (typed arithmetic, any unit spelling) and written back in the
/// document's unit, kept inside `range` when there is one; a text that is no measure goes back to what the field had.
/// Returns the text it committed.
pub(super) fn resolved(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, enabled: bool, range: Option<(f64, f64)>) -> Option<String> {
    let (before, after) = text(ui, d, key, label, enabled)?;
    let Some(measure) = parse_measure(&after, d.unit).ok().filter(|v| v.is_finite()) else {
        if let Some(before) = before {
            d.fields.insert(key.into(), json!(before));
        }
        return None;
    };
    let measure = range.map_or(measure, |(least, most)| measure.clamp(least, most));
    let shown = format_measure(measure, d.unit);
    d.fields.insert(key.into(), json!(shown));
    Some(shown)
}

/// A measure field with the range of the spinner fields (`0` to the largest page).
pub(crate) fn measure(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, enabled: bool) -> Option<String> {
    resolved(ui, d, key, label, enabled, Some(MEASURE_RANGE))
}

/// A number field over the draft of `key`: the value it holds after an edit, kept inside `range`.
pub(super) fn number(ui: &mut Ui, d: &Dialog, key: &str, label: &str, range: (f64, f64), whole: bool, unit: Option<&'static str>) -> Option<f64> {
    let field = NumberField::new(label.to_owned(), Bind::given(d.n(key).filter(|v| v.is_finite())))
        .id_salt(("frame_form_number", key))
        .range(range.0..=range.1)
        .step(1.0);
    let field = if whole { field.snap_typed(true).fixed_decimals(0).commit_decimals(0) } else { field };
    let field = if let Some(unit) = unit { field.unit(unit) } else { field };
    let out = field.show(ui);
    let value = out.edit.events.landed().copied().flatten().filter(|v| v.is_finite())?;
    let value = value.clamp(range.0, range.1);
    let value = if whole { value.round() } else { value };
    (Some(value) != d.n(key)).then_some(value)
}

/// A check box over the draft flag `key`.
pub(super) fn check(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, enabled: bool) {
    let out = Checkbox::new(Bind::given(d.b(key))).label(label.to_owned()).id_salt(("frame_form_check", key)).enabled(enabled).show(ui);
    if let Some(on) = out.events.committed().copied().filter(|on| *on != d.b(key)) {
        d.fields.insert(key.into(), json!(on));
    }
}

/// A picker over the draft string of `key`, from `options` as (value, caption). A value none of them has shows as the
/// picker's placeholder.
pub(super) fn select(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, options: &[(String, String)], enabled: bool) {
    let current = d.s(key);
    let entries: Vec<Entry<String>> = options.iter().map(|(value, caption)| Entry::new(value.clone(), caption.clone())).collect();
    let mut picker =
        Select::new(label.to_owned(), Bind::given(current.clone()), &entries).keeps_name(true).id_salt(("frame_form_select", key)).enabled(enabled);
    if !options.iter().any(|(value, _)| *value == current) {
        picker = picker.placeholder(current.clone());
    }
    if let Some(next) = picker.show(ui).events.committed().cloned().filter(|next| *next != current) {
        d.fields.insert(key.into(), json!(next));
    }
}

/// `options` captioned in the interface language.
pub(super) fn captioned(lang: &str, options: &[(&str, &str)]) -> Vec<(String, String)> {
    options.iter().map(|(value, caption)| ((*value).to_owned(), crate::i18n::tr(lang, caption).to_string())).collect()
}

/// The nine-point picker over `current`; the point the user picked in this pass.
pub(super) fn points(ui: &mut Ui, label: &str, salt: &str, current: Option<Anchor9>) -> Option<Anchor9> {
    let out = ReferencePoint::new(label.to_owned(), Bind::given(current)).id_salt(("frame_form_points", salt.to_owned())).show(ui);
    out.events.committed().copied().flatten().filter(|picked| Some(*picked) != current)
}

/// A message under the form.
pub(super) fn message(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).shade(Shade::Muted).show(ui);
}

// ---------------------------------------------------------------------------------------------------------------------
// Text Frame Options

fn text_frame_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    super::seed_text_frame_options(app, d);
    if d.s("section").is_empty() {
        d.fields.insert("section".into(), json!("general"));
    }
    text_frame_body(app, ui, d);
    super::text_frame_preview(app, d);
    None
}

fn text_frame_body(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let sections = [("general", "General"), ("rules", "Column Rules"), ("baseline", "Baseline Options"), ("autosize", "Auto-Size")];
    Stack::new().id_salt("frame_options").fill_width().show(ui, |ui, _| {
        Row::new().id_salt("frame_options_layout").fill_width().show(ui, |ui, _| {
            let before = d.s("section");
            let shown = RadioGroup::new(tr("Text Frame Options"), Bind::given(before.clone())).id_salt("frame_options_sections").show(ui, |ui, _| {
                Stack::new().id_salt("frame_options_section_list").show(ui, |ui, _| {
                    for (id, caption) in sections {
                        Radio::new(id.to_owned()).label(tr(caption)).id_salt(("frame_options_section", id)).show(ui);
                    }
                });
            });
            if let Some(next) = shown.output.events.landed().filter(|next| **next != before).cloned() {
                d.fields.insert("section".into(), json!(next));
            }
            Stack::new().id_salt("frame_options_page").fill_width().show(ui, |ui, _| match d.s("section").as_str() {
                "baseline" => baseline_page(app, ui, d),
                "autosize" => auto_size_page(app, ui, d),
                "rules" => column_rules_page(app, ui, d),
                _ => general(app, ui, d),
            });
        });
        check(ui, d, "preview", &tr("Preview"), true);
        if let Some(status) = d.fields.get("status").and_then(Value::as_str).map(str::to_owned) {
            message(ui, &status);
        }
    });
}

/// Text Frame Options › General, as InDesign's: Columns (kind, number and width, gutter and maximum, Balance Columns),
/// Inset Spacing (the four sides and the link between them; one Inset for a non-rectangular frame), Vertical
/// Justification (align, paragraph spacing limit) and Ignore Text Wrap. Also the General page of Object Style Options.
pub(crate) fn general(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let number_caption = crate::i18n::tr_context(lang, "Number:", "count").to_string();
    let kinds = captioned(lang, &[("fixedNumber", "Fixed Number"), ("fixedWidth", "Fixed Width")]);
    let justifications = captioned(lang, &[("top", "Top"), ("center", "Center"), ("bottom", "Bottom"), ("justify", "Justify")]);
    Stack::new().id_salt("frame_general").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr("Columns")).id_salt("frame_general_columns").show(ui, |ui, _| {
            field(ui, &tr("Columns:"), "columns_kind", |ui| select(ui, d, "columnsKind", &tr("Columns:"), &kinds, true));
            field(ui, &number_caption, "columns", |ui| {
                if let Some(columns) = number(ui, d, "columns", &number_caption, (1.0, 40.0), true, None) {
                    d.fields.insert("columns".into(), json!(columns.round().to_string()));
                }
            });
            field(ui, &tr("Width:"), "column_width", |ui| {
                if d.s("columnsKind") == "fixedWidth" {
                    measure(ui, d, "columnWidth", &tr("Width:"), true);
                } else {
                    // Fixed Number: each column's width, from the frame, the insets and the gutter.
                    let frame = d.fields.get("_frameWidth").and_then(Value::as_f64);
                    let n = d.n("columns").unwrap_or(1.0).max(1.0);
                    // A non-rectangular frame keeps its sides until its one Inset is changed.
                    let (left, right) = if d.b("_shaped") && super::edited(d, "inset") { ("inset", "inset") } else { ("insetLeft", "insetRight") };
                    let width = frame.map(|w| {
                        let inner = w - d.m(left).unwrap_or(0.0) - d.m(right).unwrap_or(0.0);
                        ((inner - d.m("gutter").unwrap_or(0.0) * (n - 1.0)) / n).max(0.0)
                    });
                    let shown = width.map(|w| format_measure(w, d.unit)).unwrap_or_default();
                    TextField::new(tr("Width:"), Bind::given(shown)).id_salt("frame_form_auto_width").enabled(false).show(ui);
                }
            });
            field(ui, &tr("Gutter:"), "gutter", |ui| {
                measure(ui, d, "gutter", &tr("Gutter:"), true);
            });
            // Maximum is for Flexible Width columns, which aren't in the document model yet.
            field(ui, &tr("Maximum:"), "maximum", |ui| {
                NumberField::new(tr("Maximum:"), Bind::given(None::<f64>)).id_salt("frame_form_maximum").enabled(false).show(ui);
            });
            check(ui, d, "balanceColumns", &tr("Balance Columns"), true);
        });
        GroupBox::new(tr("Inset Spacing"), None).id_salt("frame_general_inset_box").show(ui, |ui, _| {
            FieldGroup::new(tr("Inset Spacing")).id_salt("frame_general_inset").show(ui, |ui, _| {
                // A non-rectangular frame: the four sides are dimmed and one Inset sets the distance kept from the
                // outline.
                let shaped = d.b("_shaped");
                for (caption, key) in [("Top:", SIDES[0]), ("Left:", SIDES[1]), ("Bottom:", SIDES[2]), ("Right:", SIDES[3])] {
                    field(ui, &tr(caption), key, |ui| {
                        Row::new().id_salt(("frame_general_side", key)).show(ui, |ui, _| {
                            // While linked, a side typed in sets all four ("Make all settings the same").
                            if let Some(side) = measure(ui, d, key, &tr(caption), !shaped)
                                && d.b("insetLinked")
                                && !shaped
                            {
                                for other in SIDES {
                                    d.fields.insert(other.into(), json!(side));
                                }
                            }
                            if key == SIDES[0] {
                                let out = link_toggle(tr("Make all settings the same"), Bind::given(d.b("insetLinked")))
                                    .id_salt("frame_general_link")
                                    .enabled(!shaped)
                                    .show(ui);
                                if let Some(linked) = out.events.committed().copied() {
                                    d.fields.insert("insetLinked".into(), json!(linked));
                                }
                            }
                        });
                    });
                }
                if shaped {
                    field(ui, &tr("Inset:"), "inset", |ui| {
                        measure(ui, d, "inset", &tr("Inset:"), true);
                    });
                }
            });
        });
        GroupBox::new(tr("Vertical Justification"), None).id_salt("frame_general_vertical_box").show(ui, |ui, _| {
            FieldGroup::new(tr("Vertical Justification")).id_salt("frame_general_vertical").show(ui, |ui, _| {
                field(ui, &tr("Align:"), "vertical_justification", |ui| select(ui, d, "verticalJustification", &tr("Align:"), &justifications, true));
                let justify = d.s("verticalJustification") == "justify";
                field(ui, &tr("Paragraph Spacing Limit:"), "paragraph_spacing_limit", |ui| {
                    measure(ui, d, "paragraphSpacingLimit", &tr("Paragraph Spacing Limit:"), justify);
                });
            });
        });
        check(ui, d, "ignoreWrap", &tr("Ignore Text Wrap"), true);
    });
}

/// Text Frame Options › Column Rules.
fn column_rules_page(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let colors: Vec<(String, String)> = app
        .session
        .active()
        .map(|s| {
            s.doc
                .swatches
                .iter()
                .filter(|w| !matches!(w.value, designcraft_color::swatch::SwatchValue::Gradient { .. }))
                .map(|w| (w.name.clone(), w.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    Stack::new().id_salt("frame_rules").fill_width().show(ui, |ui, _| {
        check(ui, d, "columnRule", &tr("Insert Column Rule"), true);
        FieldGroup::new(tr("Column Rules")).id_salt("frame_rules_fields").show(ui, |ui, _| {
            field(ui, &tr("Weight"), "rule_weight", |ui| {
                text(ui, d, "columnRuleWeight", &tr("Weight"), true);
            });
            field(ui, &tr("Color"), "rule_color", |ui| select(ui, d, "columnRuleColor", &tr("Color"), &colors, true));
            for (caption, key) in [
                ("Tint %", "columnRuleTint"),
                ("Top Inset", "columnRuleTopInset"),
                ("Bottom Inset", "columnRuleBottomInset"),
                ("Horizontal Offset", "columnRuleOffset"),
            ] {
                field(ui, &tr(caption), key, |ui| {
                    text(ui, d, key, &tr(caption), true);
                });
            }
        });
    });
}

/// Text Frame Options › Baseline Options.
fn baseline_page(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let offsets =
        captioned(lang, &[("ascent", "Ascent"), ("capHeight", "Cap Height"), ("leading", "Leading"), ("xHeight", "x Height"), ("fixed", "Fixed")]);
    let relative = captioned(lang, &[("page", "Top of Page"), ("margin", "Top Margin"), ("frame", "Top of Frame"), ("inset", "Top Inset")]);
    Stack::new().id_salt("frame_baseline").fill_width().show(ui, |ui, _| {
        GroupBox::new(tr("First Baseline"), None).id_salt("frame_baseline_first_box").show(ui, |ui, _| {
            FieldGroup::new(tr("First Baseline")).id_salt("frame_baseline_first").show(ui, |ui, _| {
                field(ui, &tr("Offset:"), "offset", |ui| select(ui, d, "firstBaseline", &tr("Offset:"), &offsets, true));
                field(ui, &tr("Min:"), "minimum", |ui| {
                    text(ui, d, "firstBaselineMin", &tr("Min:"), true);
                });
            });
        });
        GroupBox::new(tr("Baseline Grid"), None).id_salt("frame_baseline_grid_box").show(ui, |ui, _| {
            check(ui, d, "customGrid", &tr("Use Custom Baseline Grid"), true);
            let custom = d.b("customGrid");
            FieldGroup::new(tr("Baseline Grid")).id_salt("frame_baseline_grid").show(ui, |ui, _| {
                field(ui, &tr("Start:"), "grid_start", |ui| {
                    text(ui, d, "gridStart", &tr("Start:"), custom);
                });
                field(ui, &tr("Relative To:"), "grid_relative", |ui| select(ui, d, "gridRelativeTo", &tr("Relative To:"), &relative, custom));
                field(ui, &tr("Increment Every:"), "grid_increment", |ui| {
                    text(ui, d, "gridIncrement", &tr("Increment Every:"), custom);
                });
            });
        });
    });
}

/// Text Frame Options › Auto-Size.
fn auto_size_page(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let modes = captioned(
        lang,
        &[
            ("off", "Off"),
            ("heightOnly", "Height Only"),
            ("widthOnly", "Width Only"),
            ("heightAndWidth", "Height and Width"),
            ("heightAndWidthProportionally", "Height and Width (Keep Proportions)"),
        ],
    );
    Stack::new().id_salt("frame_auto_size").fill_width().show(ui, |ui, _| {
        GroupBox::new(tr("Auto-Size"), None).id_salt("frame_auto_size_box").show(ui, |ui, _| {
            FieldGroup::new(tr("Auto-Size")).id_salt("frame_auto_size_fields").show(ui, |ui, _| {
                field(ui, &tr("Auto-Sizing"), "mode", |ui| select(ui, d, "autoSize", &tr("Auto-Sizing"), &modes, true));
                field(ui, &tr("Reference Point"), "reference", |ui| {
                    let references = designcraft_engine::autosize::REFERENCES;
                    let current = references.iter().position(|name| *name == d.s("autoSizeReference")).map(|at| Anchor9::ALL[at]);
                    if let Some(picked) = points(ui, &tr("Reference Point"), "auto_size", current)
                        && let Some(at) = Anchor9::ALL.iter().position(|anchor| *anchor == picked)
                    {
                        d.fields.insert("autoSizeReference".into(), json!(references[at]));
                    }
                });
                for (caption, flag, key) in
                    [("Minimum Height", "useMinimumHeight", "minimumHeight"), ("Minimum Width", "useMinimumWidth", "minimumWidth")]
                {
                    Row::new().id_salt(("frame_auto_size_minimum", key)).show(ui, |ui, _| {
                        check(ui, d, flag, &tr(caption), true);
                        text(ui, d, key, &tr(caption), true);
                    });
                }
            });
            check(ui, d, "noLineBreaks", &tr("No Line Breaks"), true);
        });
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// Frame Fitting Options

fn fitting_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let fittings = captioned(
        lang,
        &[
            ("none", "None"),
            ("fillProportionally", "Fill Frame Proportionally"),
            ("fitProportionally", "Fit Content Proportionally"),
            ("fitContentToFrame", "Fit Content to Frame"),
            ("centerContent", "Center Content"),
        ],
    );
    Stack::new().id_salt("frame_fitting").fill_width().show(ui, |ui, _| {
        check(ui, d, "autoFit", &tr("Auto-Fit"), true);
        GroupBox::new(tr("Content Fitting"), None).id_salt("frame_fitting_box").show(ui, |ui, _| {
            FieldGroup::new(tr("Content Fitting")).id_salt("frame_fitting_fields").show(ui, |ui, _| {
                field(ui, &tr("Fitting:"), "fitting", |ui| select(ui, d, "fitting", &tr("Fitting:"), &fittings, true));
                field(ui, &tr("Align From:"), "align", |ui| {
                    let current = Anchor9::ALL.get(d.n("align").unwrap_or(4.0).max(0.0) as usize).copied();
                    if let Some(picked) = points(ui, &tr("Align From:"), "fitting", current)
                        && let Some(at) = Anchor9::ALL.iter().position(|anchor| *anchor == picked)
                    {
                        d.fields.insert("align".into(), json!(at));
                    }
                });
            });
        });
        GroupBox::new(tr("Crop Amount"), None).id_salt("frame_fitting_crop_box").show(ui, |ui, _| {
            FieldGroup::new(tr("Crop Amount")).id_salt("frame_fitting_crop").show(ui, |ui, _| {
                for (caption, key) in [("Top:", "cropTop"), ("Left:", "cropLeft"), ("Bottom:", "cropBottom"), ("Right:", "cropRight")] {
                    field(ui, &tr(caption), key, |ui| {
                        // Typed arithmetic ("0.4375 in*2") is worked out when the field is left.
                        resolved(ui, d, key, &tr(caption), true, None);
                    });
                }
            });
        });
    });
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Polygon Settings

fn polygon_settings(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    FieldGroup::new(tr("Polygon Settings")).id_salt("frame_polygon").show(ui, |ui, _| {
        field(ui, &tr("Number of Sides:"), "sides", |ui| {
            if let Some(sides) = number(ui, d, "sides", &tr("Number of Sides:"), (3.0, 100.0), true, None) {
                d.fields.insert("sides".into(), json!(sides as u64));
            }
        });
        field(ui, &tr("Star Inset:"), "star_inset", |ui| {
            if let Some(inset) = number(ui, d, "starInset", &tr("Star Inset:"), (0.0, 100.0), false, Some("%")) {
                d.fields.insert("starInset".into(), json!(inset));
            }
        });
    });
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Generate QR Code

fn qr_code(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let types =
        captioned(lang, &[("url", "Web Hyperlink"), ("text", "Plain Text"), ("sms", "Text Message"), ("email", "Email"), ("vcard", "Business Card")]);
    let kind = d.s("type");
    let fields: &[(&str, &str)] = match kind.as_str() {
        "sms" => &[("number", "Cell Number:"), ("message", "Message:")],
        "email" => &[("to", "Email Address:"), ("subject", "Subject:"), ("body", "Message:")],
        "vcard" => &[("name", "Name:"), ("org", "Organization:"), ("phone", "Phone:"), ("email", "Email:"), ("url", "URL:")],
        "text" => &[("content", "Text:")],
        _ => &[("content", "URL:")],
    };
    FieldGroup::new(tr("Generate QR Code")).id_salt("frame_qr").show(ui, |ui, _| {
        field(ui, &tr("Type:"), "type", |ui| select(ui, d, "type", &tr("Type:"), &types, true));
        for (key, caption) in fields.iter().copied() {
            field(ui, &tr(caption), key, |ui| {
                text(ui, d, key, &tr(caption), true);
            });
        }
        field(ui, &tr("Color:"), "color", |ui| {
            text(ui, d, "color", &tr("Color:"), true);
        });
    });
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Object Layer Options

fn layer_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let layers = d.fields.get("layers").and_then(Value::as_array).cloned().unwrap_or_default();
    Stack::new().id_salt("frame_layers").fill_width().show(ui, |ui, _| {
        if layers.is_empty() {
            message(ui, &tr("This PDF has no layers."));
        }
        let mut now = layers.clone();
        let mut changed = false;
        for (at, layer) in layers.iter().enumerate() {
            let visible = layer["visible"].as_bool().unwrap_or(true);
            let name = layer["name"].as_str().unwrap_or("").to_owned();
            let out = Checkbox::new(Bind::given(visible)).label(name).id_salt(("frame_layer", at)).show(ui);
            if let Some(on) = out.events.committed().copied().filter(|on| *on != visible)
                && let Some(o) = now[at].as_object_mut()
            {
                o.insert("visible".into(), json!(on));
                changed = true;
            }
        }
        if changed {
            d.fields.insert("layers".into(), json!(now));
            d.fields.insert("useDefaults".into(), json!(false));
        }
        if Button::new(tr("Use File Defaults")).id_salt("frame_layers_defaults").show(ui).clicked() {
            let mut defaults = layers.clone();
            for layer in &mut defaults {
                layer["visible"] = layer["default"].clone();
            }
            d.fields.insert("layers".into(), json!(defaults));
            d.fields.insert("useDefaults".into(), json!(true));
        }
    });
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Tokens;
    use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role, vec2};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    fn app_with_document() -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".to_owned();
        app.run("file.new", json!({})).unwrap();
        app
    }

    /// A document with one selected text frame.
    fn app_with_text_frame() -> (DesignApp, designcraft_doc::ItemId) {
        let mut app = app_with_document();
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "content": "text", "text": "Hello"})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        app.run("selection.set", json!({"ids": [id.0]})).unwrap();
        (app, id)
    }

    /// The dialogs in a window whose context has the framework installed, so each opens in the framework host.
    fn harness(mut app: DesignApp) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        app.framework_transform = false;
        app.framework_opacity = true;
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        assert!(app.framework_installed);
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(920.0, 1000.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                crate::dialogs::show(app, ui.ctx());
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn steps(h: &mut Harness<'_, DesignApp>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn event(h: &mut Harness<'_, DesignApp>, event: Event) {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(event);
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    fn key(h: &mut Harness<'_, DesignApp>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        event(h, Event::ModifiersChanged(Modifiers::NONE));
        steps(h, 3);
    }

    fn click_at(h: &mut Harness<'_, DesignApp>, at: egui::Pos2) {
        event(h, Event::PointerMoved(at));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        steps(h, 3);
    }

    fn click(h: &mut Harness<'_, DesignApp>, role: Role, label: &str) {
        let at = h.get_by_role_and_label(role, label).rect().center();
        click_at(h, at);
    }

    /// Types into the field named `label` as a person does: a click, select all, the text, then Tab to leave it.
    fn type_in(h: &mut Harness<'_, DesignApp>, role: Role, label: &str, text: &str) {
        click(h, role, label);
        assert!(h.get_by_role_and_label(role, label).is_focused(), "a click focuses {label}");
        key(h, Key::A, Modifiers::COMMAND);
        event(h, Event::Text(text.to_owned()));
        steps(h, 3);
        key(h, Key::Tab, Modifiers::NONE);
    }

    /// Opens the picker named `label` and picks `option`.
    fn pick(h: &mut Harness<'_, DesignApp>, label: &str, option: &str) {
        click(h, Role::ComboBox, label);
        let at = h.get_by_label(option).rect().center();
        click_at(h, at);
    }

    fn dialog<'a>(h: &'a Harness<'_, DesignApp>) -> &'a Dialog {
        h.state().ui.dialog.as_ref().expect("the dialog is open")
    }

    fn commands(h: &Harness<'_, DesignApp>, id: &str) -> Vec<Value> {
        h.state().session.journal.iter().filter(|(command, _)| command == id).map(|(_, params)| params.clone()).collect()
    }

    fn undo_steps(h: &Harness<'_, DesignApp>) -> usize {
        h.state().session.doc().unwrap().history.undo.len()
    }

    fn frame_options(h: &Harness<'_, DesignApp>, id: designcraft_doc::ItemId) -> designcraft_doc::TextFrameOptions {
        h.state().session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone()
    }

    /// Regression: the framework form must edit the same draft and end in the same single command and undo step as the
    /// native window, across all four pages and with the link keeping the four insets equal.
    #[test]
    fn text_frame_options_pages_edit_one_draft_and_ok_is_one_command_and_undo_step() {
        let (mut app, id) = app_with_text_frame();
        crate::menus::activate(&mut app, "object.textFrameOptions", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("textFrameOptions"));
        let mut h = harness(app);
        let _ = h.get_by_role_and_label(Role::Dialog, "Text Frame Options");
        let undo = undo_steps(&h);

        pick(&mut h, "Columns:", "Fixed Width");
        assert_eq!(dialog(&h).s("columnsKind"), "fixedWidth");
        type_in(&mut h, Role::TextInput, "Width:", "100 pt");
        assert_eq!(dialog(&h).m("columnWidth"), Some(100.0));
        // The link is on for a frame with even insets: one side typed sets all four.
        type_in(&mut h, Role::TextInput, "Top:", "6 pt");
        for side in SIDES {
            assert_eq!(dialog(&h).m(side), Some(6.0), "{side} follows the linked side");
        }

        click(&mut h, Role::RadioButton, "Column Rules");
        click(&mut h, Role::CheckBox, "Insert Column Rule");
        assert!(dialog(&h).b("columnRule"));

        click(&mut h, Role::RadioButton, "Baseline Options");
        click(&mut h, Role::CheckBox, "Use Custom Baseline Grid");
        type_in(&mut h, Role::TextInput, "Increment Every:", "14 pt");
        assert!(dialog(&h).b("customGrid"));

        click(&mut h, Role::RadioButton, "Auto-Size");
        pick(&mut h, "Auto-Sizing", "Height Only");
        click(&mut h, Role::RadioButton, "Bottom right");
        click(&mut h, Role::CheckBox, "Minimum Height");
        assert_eq!((dialog(&h).s("autoSize"), dialog(&h).s("autoSizeReference")), ("heightOnly".to_owned(), "bottomRight".to_owned()));
        assert_eq!(undo_steps(&h), undo, "editing the draft changes no document");

        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none(), "OK closes the dialog");
        let sent = commands(&h, "object.textFrameOptions");
        assert_eq!(sent.len(), 1, "one command: {sent:?}");
        let p = &sent[0];
        assert_eq!(p["columnsKind"], "fixedWidth");
        assert_eq!(p["columnWidth"], 100.0);
        assert_eq!(p["inset"], json!([6.0, 6.0, 6.0, 6.0]));
        assert_eq!(p["columnRule"], true);
        assert_eq!(p["baselineGrid"]["increment"], 14.0);
        assert_eq!((&p["autoSize"], &p["autoSizeReference"], &p["useMinimumHeight"]), (&json!("heightOnly"), &json!("bottomRight"), &json!(true)));
        let o = frame_options(&h, id);
        assert_eq!((o.column_width, o.inset, o.column_rule), (100.0, [6.0; 4], true));
        assert_eq!(undo_steps(&h), undo + 1, "one undo step");
        h.state_mut().run("edit.undo", json!({})).unwrap();
        let o = frame_options(&h, id);
        assert_eq!((o.column_rule, o.inset), (false, [0.0; 4]), "Undo takes the whole dialog back");
    }

    /// Regression: with Preview on the page shows the draft, and Cancel puts the frame back without an undo step.
    #[test]
    fn text_frame_options_preview_shows_the_draft_and_cancel_puts_the_frame_back() {
        let (mut app, id) = app_with_text_frame();
        crate::menus::activate(&mut app, "object.textFrameOptions", &Value::Null);
        let mut h = harness(app);
        let before = frame_options(&h, id);
        let undo = undo_steps(&h);
        type_in(&mut h, Role::TextInput, "Gutter:", "9 pt");
        assert_eq!(frame_options(&h, id).gutter, before.gutter, "no preview until it is on");
        click(&mut h, Role::CheckBox, "Preview");
        steps(&mut h, 3);
        assert_eq!(frame_options(&h, id).gutter, 9.0, "the page shows the draft");
        assert_eq!(undo_steps(&h), undo, "a preview is no undo step");
        type_in(&mut h, Role::TextInput, "Gutter:", "11 pt");
        assert_eq!(frame_options(&h, id).gutter, 11.0, "redone when a field changes");
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(frame_options(&h, id).gutter, before.gutter, "Cancel: the frame as it was");
        assert_eq!(undo_steps(&h), undo);
        assert!(commands(&h, "object.textFrameOptions").is_empty());
    }

    /// Regression: a measure that is no measure goes back to what the field had, so OK never sees it.
    #[test]
    fn a_measure_field_keeps_its_last_valid_text_when_a_non_measure_is_committed() {
        let (mut app, _) = app_with_text_frame();
        crate::menus::activate(&mut app, "object.textFrameOptions", &Value::Null);
        let mut h = harness(app);
        let shown = dialog(&h).s("gutter");
        type_in(&mut h, Role::TextInput, "Gutter:", "wide");
        assert_eq!(dialog(&h).s("gutter"), shown);
    }

    /// Regression: Frame Fitting Options ends in the same command with the same params.
    #[test]
    fn fitting_options_pick_edit_and_ok_run_the_fitting_command() {
        let mut app = app_with_document();
        app.run("document.preferences", json!({"horizontalUnits": "points"})).unwrap();
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
        let f = app.run("frame.create", json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
        app.run("file.place", json!({"base64": designcraft_engine::cmd::base64_encode(&png), "name": "a.png", "frame": f})).unwrap();
        app.run("selection.set", json!({"ids": [f]})).unwrap();
        crate::menus::activate(&mut app, "app.fittingOptions", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("fittingOptions"));
        let mut h = harness(app);
        let _ = h.get_by_role_and_label(Role::Dialog, "Frame Fitting Options");
        click(&mut h, Role::CheckBox, "Auto-Fit");
        pick(&mut h, "Fitting:", "Fit Content to Frame");
        click(&mut h, Role::RadioButton, "Bottom right");
        type_in(&mut h, Role::TextInput, "Top:", "3 pt");
        assert_eq!((dialog(&h).b("autoFit"), dialog(&h).s("fitting"), dialog(&h).n("align")), (true, "fitContentToFrame".to_owned(), Some(8.0)));
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let sent = commands(&h, "object.fittingOptions");
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0]["autoFit"], true);
        assert_eq!(sent[0]["fitting"], "fitContentToFrame");
        assert_eq!(sent[0]["align"], 8);
        assert_eq!(sent[0]["crop"][0], 3.0);
    }

    /// Regression: Polygon Settings keeps the sides and star inset a person types and sends them to the tool setting.
    #[test]
    fn polygon_settings_ok_sets_the_sides_and_star_inset() {
        let mut app = app_with_document();
        app.ui.dialog = Some(Dialog::new("polygonSettings", json!({"sides": 6, "starInset": 0})));
        let mut h = harness(app);
        let _ = h.get_by_role_and_label(Role::Dialog, "Polygon Settings");
        type_in(&mut h, Role::SpinButton, "Number of Sides:", "8");
        type_in(&mut h, Role::SpinButton, "Star Inset:", "25");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let sent = commands(&h, "tool.polygonSettings");
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0]["sides"], 8);
        assert_eq!(sent[0]["starInset"].as_f64(), Some(25.0));
    }

    /// Regression: Generate QR Code shows the fields of the chosen type and OK sends the draft as it is.
    #[test]
    fn qr_code_type_fields_follow_the_choice_and_ok_sends_the_draft() {
        let mut app = app_with_document();
        crate::menus::activate(&mut app, "app.qrCode", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("qrCode"));
        let mut h = harness(app);
        let _ = h.get_by_role_and_label(Role::Dialog, "Generate QR Code");
        let _ = h.get_by_role_and_label(Role::TextInput, "URL:");
        pick(&mut h, "Type:", "Plain Text");
        assert!(h.query_by_role_and_label(Role::TextInput, "URL:").is_none(), "the URL field goes with the type");
        type_in(&mut h, Role::TextInput, "Text:", "hello");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let sent = commands(&h, "object.qrCode");
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!((&sent[0]["type"], &sent[0]["content"]), (&json!("text"), &json!("hello")));
    }

    /// Regression: unticking a layer makes the choice explicit, Use File Defaults gives it back, and neither changes
    /// anything until OK.
    #[test]
    fn layer_options_tick_layers_and_use_file_defaults_edit_only_the_draft() {
        let mut app = app_with_document();
        let layers = json!([{"name": "Background", "visible": true, "default": true}, {"name": "Notes", "visible": true, "default": false}]);
        app.ui.dialog = Some(Dialog::new("layerOptions", json!({"useDefaults": true, "layers": layers})));
        let mut h = harness(app);
        let _ = h.get_by_role_and_label(Role::Dialog, "Object Layer Options");
        click(&mut h, Role::CheckBox, "Background");
        assert_eq!(dialog(&h).fields["layers"][0]["visible"], false);
        assert_eq!(dialog(&h).fields["layers"][1]["visible"], true);
        assert_eq!(dialog(&h).b("useDefaults"), false);
        click(&mut h, Role::Button, "Use File Defaults");
        assert_eq!(dialog(&h).fields["layers"][0]["visible"], true);
        assert_eq!(dialog(&h).fields["layers"][1]["visible"], false, "the file's own default for Notes");
        assert!(dialog(&h).b("useDefaults"));
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert!(commands(&h, "object.layerOptions").is_empty());
    }

    /// Regression: a PDF without layers says so, and OK keeps the file defaults as the native window does.
    #[test]
    fn layer_options_without_layers_say_so_and_ok_keeps_the_file_defaults() {
        let mut app = app_with_document();
        let pdf = app.run("file.exportPdf", json!({})).unwrap()["base64"].clone();
        let placed = app.run("file.place", json!({"base64": pdf, "name": "plain.pdf", "x": 0, "y": 0, "width": 100})).unwrap();
        app.run("selection.set", json!({"ids": [placed["id"]]})).unwrap();
        app.ui.dialog = Some(Dialog::new("layerOptions", json!({})));
        let mut h = harness(app);
        let _ = h.get_by_label("This PDF has no layers.");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let sent = commands(&h, "object.layerOptions");
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0]["useDefaults"], true);
    }
}
