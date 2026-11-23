//! The Object Styles panel's New Object Style and Object Style Options dialog, laid out as
//! InDesign's: the style's name on top; on the left its Basic Attributes (a checkbox includes one
//! in the style, a click shows its settings) and its effects; on the right the settings of the
//! attribute shown (General sums the style up); Apply Style to Selection for a new style.
//!
//! Paragraph Styles nests a paragraph style in the object style (the course's chapter 39), and
//! Text Frame General Options is the Text Frame Options dialog's General page.

use serde_json::{Value, json};

use crate::DesignApp;
use crate::dialogs::Dialog;

/// The dialog's id.
pub const ID: &str = "objectStyle";

/// Basic Attributes: (key, label), in InDesign's order. General isn't an attribute: it sums the
/// style up.
const ATTRIBUTES: [(&str, &str); 6] = [
    ("general", "General"),
    ("fill", "Fill"),
    ("stroke", "Stroke"),
    ("corners", "Stroke & Corner Options"),
    ("paragraph", "Paragraph Styles"),
    ("textFrame", "Text Frame General Options"),
];

/// The fields of the Paragraph Styles and Text Frame General Options pages: the style's paragraph
/// style (`""`: none), Apply Next Style, and text frame options `tf` as the Text Frame Options
/// dialog shows them (`tfBase` keeps the options as they are, `tfShown` the fields as first shown).
fn text_fields(app: &DesignApp, fields: &mut Value, paragraph: Option<&str>, next: bool, tf: &designcraft_doc::TextFrameOptions) {
    fields["paragraphStyle"] = json!(paragraph.unwrap_or(""));
    fields["applyNext"] = json!(next);
    fields["tfBase"] = serde_json::to_value(tf).unwrap_or_default();
    fields["insetLinked"] = json!(tf.inset.iter().all(|v| (v - tf.inset[0]).abs() < 1e-9));
    let shown = crate::dialogs::text_frame_fields(tf, doc_units(app), None, &crate::dialogs::document_grid(app));
    fields["tfShown"] = shown.clone();
    if let (Value::Object(f), Value::Object(shown)) = (fields, shown) {
        f.extend(shown);
    }
}

/// The active document's ruler unit, which a bare number in a measurement field is in.
fn doc_units(app: &DesignApp) -> designcraft_geom::Unit {
    app.session.active().map_or(designcraft_geom::Unit::Picas, |s| s.doc.settings.horizontal_units)
}

/// The dialog with `fields`, reading bare numbers in the document's ruler unit.
fn dialog(app: &DesignApp, fields: Value) -> Dialog {
    let mut d = Dialog::new(ID, fields);
    d.unit = doc_units(app);
    d
}

/// The active document as the dialog records it: its uid and the epoch that File › Revert
/// changes.
fn document_key(app: &DesignApp) -> Value {
    app.session.active().map_or(Value::Null, |st| json!([st.uid, st.identity_epoch]))
}

/// The document the dialog was opened on is still the active one and was not reverted since, so
/// OK never gives its settings to another document's style of the same name.
fn same_document(app: &DesignApp, d: &Dialog) -> Result<(), String> {
    let key = document_key(app);
    if !key.is_null() && d.fields.get("doc") == Some(&key) { Ok(()) } else { Err(format!("the active document changed after {} opened", title(d))) }
}

/// The dialog for a new style from the selected object (its fill, stroke, corners and effects,
/// as InDesign's New Object Style), named "Object Style n".
pub fn open_new(app: &mut DesignApp) {
    let doc = document_key(app);
    let Some(st) = app.session.active() else { return };
    // Apply Style to Selection applies the style to the objects selected now.
    let targets: Vec<u64> = st.selection.items.iter().map(|i| i.0).collect();
    let it = st.selection.items.first().and_then(|i| st.doc.item(*i)).cloned();
    let taken = |n: &str| st.doc.styles.object_style(n).is_some();
    let name = (1..).map(|i| format!("Object Style {i}")).find(|n| !taken(n)).unwrap_or_default();
    let from = it.is_some();
    // A text frame's options come along; its paragraph style is left to choose, as in InDesign.
    let frame = it.as_ref().and_then(|i| i.text_frame()).map(|t| t.options.clone());
    let mut fields = json!({
        "new": true,
        "doc": doc,
        "targets": targets,
        "name": name,
        "basedOn": designcraft_doc::NO_OBJECT_STYLE,
        "fill": it.as_ref().map(|i| i.fill.clone()).unwrap_or_default(),
        "stroke": it.as_ref().map(|i| i.stroke.clone()).unwrap_or_default(),
        // A style holds named corners (top left, top right, …), the item its path's.
        "corners": it.as_ref().map(|i| i.corners.path_to_named(&i.path)).unwrap_or_default(),
        "fx": crate::effects_dialog::effects_json(&it.map(|i| i.effects).unwrap_or_default()),
        "hadEffects": from,
        "inc": {"fill": from, "stroke": from, "corners": from, "paragraph": from, "textFrame": frame.is_some()},
        "page": "general",
        "apply": from,
    });
    text_fields(app, &mut fields, None, false, &frame.unwrap_or_default());
    app.ui.dialog = Some(dialog(app, fields));
}

/// Object Style Options for style `name`; refused for a style the document doesn't have and for
/// `[None]`, which has no options.
pub fn open_edit(app: &mut DesignApp, name: &str) -> Result<(), String> {
    let Some(os) = app.session.active().and_then(|st| st.doc.styles.object_style(name).cloned()) else {
        return Err(format!("no object style `{name}`"));
    };
    if name == designcraft_doc::NO_OBJECT_STYLE {
        return Err(format!("`{name}` has no options"));
    }
    let mut fields = json!({
        "new": false,
        "doc": document_key(app),
        "orig": name,
        "name": name,
        "basedOn": os.based_on.as_deref().unwrap_or(designcraft_doc::NO_OBJECT_STYLE),
        "fill": os.fill.clone().unwrap_or_default(),
        "stroke": os.stroke.clone().unwrap_or_default(),
        "corners": os.corners.unwrap_or_default(),
        "fx": crate::effects_dialog::effects_json(&os.effects.clone().unwrap_or_default()),
        "hadEffects": os.effects.is_some(),
        "inc": {
            "fill": os.fill.is_some(), "stroke": os.stroke.is_some(), "corners": os.corners.is_some(),
            "paragraph": os.paragraph_style.is_some(), "textFrame": os.text_frame.is_some(),
        },
        "page": "general",
        "apply": false,
    });
    text_fields(app, &mut fields, os.paragraph_style.as_deref(), os.apply_next_style, &os.text_frame.clone().unwrap_or_default());
    app.ui.dialog = Some(dialog(app, fields));
    Ok(())
}

/// The dialog's title: New Object Style or Object Style Options.
pub fn title(d: &Dialog) -> &'static str {
    if d.b("new") { "New Object Style" } else { "Object Style Options" }
}

fn included(d: &Dialog, key: &str) -> bool {
    d.fields.get("inc").and_then(|i| i[key].as_bool()).unwrap_or(false)
}

/// Whether the style sets effects: one is on now, or (editing) the style set them already. A new
/// style with every effect off sets none, so applying it leaves the objects' own effects.
fn sets_effects(d: &Dialog) -> bool {
    let fx = d.fields.get("fx").cloned().unwrap_or(Value::Null);
    (!d.b("new") && d.b("hadEffects")) || crate::effects_dialog::PAGES.iter().skip(1).any(|(k, _)| crate::effects_dialog::is_on(&fx, k))
}

/// The style's attributes as the commands take them (null: not in the style).
fn attrs(d: &Dialog) -> serde_json::Map<String, Value> {
    let mut p = serde_json::Map::new();
    for k in ["fill", "stroke", "corners"] {
        p.insert(k.into(), if included(d, k) { d.fields.get(k).cloned().unwrap_or(Value::Null) } else { Value::Null });
    }
    p.insert("effects".into(), if sets_effects(d) { d.fields.get("fx").cloned().unwrap_or(Value::Null) } else { Value::Null });
    let paragraph = d.s("paragraphStyle");
    p.insert("paragraphStyle".into(), if included(d, "paragraph") && !paragraph.is_empty() { json!(paragraph) } else { Value::Null });
    p.insert("applyNextStyle".into(), json!(d.b("applyNext")));
    p
}

/// The Text Frame General Options page as the style's text frame options (the options it doesn't
/// show as they were), or `None` when the style doesn't set them.
fn frame_options(d: &Dialog) -> Result<Option<designcraft_doc::TextFrameOptions>, String> {
    if !included(d, "textFrame") {
        return Ok(None);
    }
    let mut o: designcraft_doc::TextFrameOptions = d.fields.get("tfBase").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let bad = |what: &str| format!("Text Frame General Options: {what}");
    // A field still as first shown keeps the option's value, which the field shows rounded.
    let shown = d.fields.get("tfShown").cloned().unwrap_or(Value::Null);
    let measure = |key: &str, was: f64| {
        if d.fields.get(key).is_some_and(|v| *v == shown[key]) {
            return Some(was);
        }
        d.m(key).filter(|v| v.is_finite() && (0.0..=15552.0).contains(v))
    };
    o.columns = d
        .n("columns")
        .filter(|n| (1.0..=40.0).contains(n) && n.fract() == 0.0)
        .ok_or_else(|| bad("the number of columns is a whole number from 1 to 40"))? as u32;
    o.gutter = measure("gutter", o.gutter).ok_or_else(|| bad("the gutter is not a measure"))?;
    o.columns_kind = serde_json::from_value(json!(d.s("columnsKind"))).unwrap_or(o.columns_kind);
    if o.columns_kind == designcraft_doc::ColumnsKind::FixedWidth {
        o.column_width = measure("columnWidth", o.column_width).ok_or_else(|| bad("the column width is not a measure"))?;
    }
    o.balance_columns = d.b("balanceColumns");
    for (side, key) in o.inset.iter_mut().zip(["insetTop", "insetLeft", "insetBottom", "insetRight"]) {
        *side = measure(key, *side).ok_or_else(|| bad("an inset is not a measure"))?;
    }
    o.vertical_justification = serde_json::from_value(json!(d.s("verticalJustification"))).unwrap_or(o.vertical_justification);
    if o.vertical_justification == designcraft_doc::VerticalJustification::Justify {
        o.vj_paragraph_spacing_limit =
            measure("paragraphSpacingLimit", o.vj_paragraph_spacing_limit).ok_or_else(|| bad("the paragraph spacing limit is not a measure"))?;
    }
    o.ignore_wrap = d.b("ignoreWrap");
    Ok(Some(o))
}

/// The General options of `o` as the `textFrame` parameter of `style.object.create` and `edit`.
fn general_json(o: &designcraft_doc::TextFrameOptions) -> Value {
    let mut v = serde_json::to_value(o).unwrap_or_default();
    if let Value::Object(m) = &mut v {
        m.retain(|k, _| designcraft_doc::TextFrameOptions::GENERAL_KEYS.contains(&k.as_str()));
    }
    v
}

/// OK: the new style (applied to the selection when asked), or the edited one.
pub fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    same_document(app, d)?;
    let name = d.s("name").trim().to_string();
    if name.is_empty() {
        return Err(crate::i18n::tr(&app.ui.language, "Enter a style name.").to_string());
    }
    let mut p = attrs(d);
    p.insert("textFrame".into(), frame_options(d)?.map_or(Value::Null, |o| general_json(&o)));
    if d.b("new") {
        // A name another style has is refused, as Object Style Options refuses it.
        if app.session.active().is_some_and(|st| st.doc.styles.object_style(&name).is_some()) {
            return Err(format!("an object style is already named `{name}`"));
        }
        p.insert("name".into(), json!(name));
        p.insert("fromSelection".into(), json!(false));
        let r = app.run("style.object.create", Value::Object(p)).map_err(|e| e.to_string())?;
        let made = r["name"].as_str().unwrap_or(&name).to_string();
        // The style exists now, so OK again mustn't make a second one: a refused apply (locked
        // objects) goes to the status line and the dialog closes. The style goes to the objects
        // selected when the dialog opened that still exist, not to a selection made since.
        let targets: Vec<u64> = app.session.active().map_or_else(Vec::new, |st| {
            d.fields
                .get("targets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .filter(|id| st.doc.item(designcraft_doc::ItemId(*id)).is_some())
                .collect()
        });
        if d.b("apply")
            && !targets.is_empty()
            && let Err(e) = app.run("style.object.apply", json!({"name": made, "ids": targets}))
        {
            app.status(e.to_string());
        }
        Ok(r)
    } else {
        let orig = d.s("orig");
        p.insert("name".into(), json!(orig));
        if name != orig {
            p.insert("rename".into(), json!(name));
        }
        // A style that didn't set effects and still doesn't: nothing to say about them.
        if p.get("effects").is_some_and(Value::is_null) && !d.b("hadEffects") {
            p.remove("effects");
        }
        // Apply Next Style means nothing without a paragraph style.
        if p.get("paragraphStyle").is_some_and(Value::is_null) {
            p.remove("applyNextStyle");
        }
        app.run("style.object.edit", Value::Object(p)).map_err(|e| e.to_string())
    }
}

fn set(d: &mut Dialog, path: &[&str], v: Value) {
    let Some((first, rest)) = path.split_first() else { return };
    let mut cur = d.fields.entry(first.to_string()).or_insert(Value::Null);
    for k in rest {
        if !cur.is_object() {
            *cur = json!({});
        }
        cur = &mut cur[*k];
    }
    *cur = v;
}

fn get<'a>(d: &'a Dialog, path: &[&str]) -> &'a Value {
    let Some((first, rest)) = path.split_first() else { return &Value::Null };
    let mut v = d.fields.get(*first).unwrap_or(&Value::Null);
    for k in rest {
        v = &v[*k];
    }
    v
}

/// One line per attribute the style sets (InDesign's Style Settings).
fn settings(app: &DesignApp, d: &Dialog) -> Vec<String> {
    let tr = |s: &str| crate::i18n::tr(&app.ui.language, s).to_string();
    let mut out = Vec::new();
    if included(d, "fill") {
        let f = get(d, &["fill"]);
        out.push(format!("{}: {} {:.0}%", tr("Fill"), f["swatch"].as_str().unwrap_or(""), f["tint"].as_f64().unwrap_or(1.0) * 100.0));
    }
    if included(d, "stroke") {
        let s = get(d, &["stroke"]);
        out.push(format!("{}: {}, {} pt", tr("Stroke"), s["swatch"].as_str().unwrap_or(""), s["weight"].as_f64().unwrap_or(0.0)));
    }
    if included(d, "corners") {
        let c = &get(d, &["corners"])["corners"][0];
        let shape: designcraft_geom::corners::CornerShape = serde_json::from_value(c["shape"].clone()).unwrap_or_default();
        out.push(format!("{}: {} {} pt", tr("Corner Options"), tr(shape.label()), c["size"].as_f64().unwrap_or(0.0)));
    }
    let paragraph = d.s("paragraphStyle");
    if included(d, "paragraph") && !paragraph.is_empty() {
        let next = if d.b("applyNext") { format!(", {}", tr("Apply Next Style")) } else { String::new() };
        out.push(format!("{}: {}{next}", tr("Paragraph Styles"), crate::i18n::style_name(&app.ui.language, &paragraph)));
    }
    if included(d, "textFrame") {
        let insets: Vec<String> = ["insetTop", "insetLeft", "insetBottom", "insetRight"].iter().map(|k| d.s(k)).collect();
        let inset = if insets.iter().all(|i| *i == insets[0]) { insets[0].clone() } else { insets.join(", ") };
        out.push(format!(
            "{}: {} {}, {} {}, {}: {inset}",
            tr("Text Frame General Options"),
            tr("Columns:"),
            d.s("columns"),
            tr("Gutter:"),
            d.s("gutter"),
            tr("Inset Spacing")
        ));
    }
    if sets_effects(d) {
        let fx = d.fields.get("fx").cloned().unwrap_or(Value::Null);
        let on: Vec<String> =
            crate::effects_dialog::PAGES.iter().skip(1).filter(|(k, _)| crate::effects_dialog::is_on(&fx, k)).map(|(_, l)| tr(l)).collect();
        out.push(format!("{}: {}", tr("Effects"), if on.is_empty() { tr("(no effects)") } else { on.join(", ") }));
    }
    out
}

/// The dialog's form under the framework's dialog host.
pub(crate) const FORM: crate::dialogs::framework::Form = crate::dialogs::framework::Form::new(framework::show).width(760.0);

/// The dialog on the framework's components: the same draft fields as the native body, with OK, Cancel, Enter and
/// Escape the host's.
mod framework {
    use super::{ATTRIBUTES, get, included, set, settings};
    use crate::DesignApp;
    use crate::dialogs::Dialog;
    use crate::effects_dialog::form::{check, choose, edited, level, number, page_row};
    use craft_behavior::{ColumnWidth, Selection};
    use craft_ui_panels::sections::GroupBox;
    use craft_ui_widgets::choice::RadioGroup;
    use craft_ui_widgets::contract::{Bind, Entry, Props as _, SelectMode, Show as _, ShowCollection as _, ShowContainer as _, Sizing};
    use craft_ui_widgets::display::{Shade, Text};
    use craft_ui_widgets::form::Field;
    use craft_ui_widgets::layout::CellAlign;
    use craft_ui_widgets::layout_components::{Grid, Row, Stack};
    use craft_ui_widgets::list::{List, ListRow};
    use craft_ui_widgets::pickers::Select;
    use craft_ui_widgets::primitives::ContentPainterCx;
    use craft_ui_widgets::runtime::env::UnitKind;
    use craft_ui_widgets::style::{DataColor, TypeRole};
    use craft_ui_widgets::text::TextField;
    use egui::{Color32, CornerRadius, Rect, Ui};
    use serde_json::{Value, json};

    /// The page list beside the page shown, each at the top of its column.
    const COLUMNS: [ColumnWidth; 2] = [ColumnWidth::Content, ColumnWidth::Content];

    /// The form: the style's name (and why OK was refused), its pages beside the page shown, and Apply Style to
    /// Selection for a new style.
    pub(super) fn show(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
        let app: &DesignApp = app;
        let tr = |s: &str| crate::i18n::tr(&app.ui.language, s).to_owned();
        let page = d.fields.get("page").and_then(Value::as_str).unwrap_or("general").to_owned();
        Stack::new().id_salt("object_style_form").fill_width().show(ui, |ui, _| {
            Field::new(tr("Style Name:")).id_salt("object_style_name_field").show(ui, |ui, _| {
                let name = d.s("name");
                let out = TextField::new(tr("Style Name:"), Bind::given(name.clone()))
                    .id_salt("object_style_name")
                    .sizing(Sizing::FillWidth)
                    .limit(None::<usize>)
                    .show(ui);
                if let Some(name) = edited(out.events, name) {
                    d.fields.insert("name".into(), json!(name));
                }
            });
            // Why OK was refused (an empty or taken name), until the next try.
            let status = d.s("status");
            if !status.is_empty() {
                Text::new(status).show(ui);
            }
            Grid::new(&COLUMNS).id_salt("object_style_columns").cells(CellAlign::Top).show(ui, |ui, _| {
                Stack::new().id_salt("object_style_list").show(ui, |ui, _| navigation(app, ui, d));
                Stack::new().id_salt("object_style_page").show(ui, |ui, _| page_form(app, ui, d, &page));
            });
            if d.b("new")
                && let Some(on) = check(ui, "object_style_apply", &tr("Apply Style to Selection"), d.b("apply"))
            {
                d.fields.insert("apply".into(), json!(on));
            }
        });
        None
    }

    /// Basic Attributes and the effects: a box includes one in the style, its name shows its page.
    fn navigation(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
        let tr = |s: &str| crate::i18n::tr(&app.ui.language, s).to_owned();
        let before = d.s("page");
        let shown = RadioGroup::new(tr("Basic Attributes"), Bind::given(before.clone())).id_salt("object_style_pages").show(ui, |ui, _| {
            Stack::new().show(ui, |ui, _| {
                Text::new(tr("Basic Attributes")).role(TypeRole::BodyStrong).show(ui);
                for (key, label) in ATTRIBUTES {
                    let check = (key != "general").then(|| included(d, key));
                    if let Some(on) = page_row(ui, "object_style", key, &tr(label), check) {
                        set(d, &["inc", key], json!(on));
                    }
                }
                Field::new(tr("Effects for:")).id_salt("object_style_fx_level").show(ui, |ui, _| {
                    level(ui, "os_fx_level", &tr("Effects for:"), &tr("Object"));
                });
                for (key, label) in crate::effects_dialog::PAGES.iter().skip(1) {
                    let on = crate::effects_dialog::is_on(d.fields.get("fx").unwrap_or(&Value::Null), key);
                    if let Some(on) = page_row(ui, "object_style", key, &tr(label), Some(on))
                        && let Some(f) = d.fields.get_mut("fx")
                    {
                        crate::effects_dialog::switch(f, key, on);
                    }
                }
            });
        });
        if let Some(page) = edited(shown.output.events, before) {
            d.fields.insert("page".into(), json!(page));
        }
    }

    /// The settings of the attribute shown.
    fn page_form(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, page: &str) {
        let lang = app.ui.language.as_str();
        let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
        if let Some((_, title)) = ATTRIBUTES.iter().find(|a| a.0 == page) {
            Text::new(tr(title)).role(TypeRole::Heading).show(ui);
        }
        match page {
            "general" => {
                Field::new(tr("Based On:")).id_salt("os_based_on_field").show(ui, |ui, _| {
                    // Based On isn't editable yet: the style's parent, as it is.
                    let based = d.s("basedOn");
                    let entries = [Entry::new(based.clone(), crate::i18n::style_name(lang, &based).to_owned())];
                    Select::new(tr("Based On:"), Bind::given(based.clone()), &entries).id_salt("os_based_on").enabled(false).show(ui);
                });
                let lines = settings(app, d);
                GroupBox::new(tr("Style Settings:"), None).id_salt("os_style_settings").show(ui, |ui, _| {
                    if lines.is_empty() {
                        Text::new(tr("(no attributes)")).shade(Shade::Muted).show(ui);
                    }
                    for line in &lines {
                        Text::new(line.clone()).show(ui);
                    }
                });
            }
            "fill" | "stroke" => {
                let key = page;
                ui.add_enabled_ui(included(d, key), |ui| {
                    swatches(app, ui, d, key);
                    Row::new().id_salt(("os_paint", key.to_owned())).show(ui, |ui, _| {
                        if key == "stroke" {
                            Field::new(tr("Weight:")).id_salt("os_weight_field").show(ui, |ui, _| {
                                let weight = get(d, &["stroke", "weight"]).as_f64().unwrap_or(0.0);
                                if let Some(v) = number(ui, "os_stroke_weight", &tr("Weight:"), weight, UnitKind::Points, 0.0..=1000.0, 2) {
                                    set(d, &["stroke", "weight"], json!(v));
                                }
                            });
                        }
                        Field::new(tr("Tint:")).id_salt(("os_tint_field", key.to_owned())).show(ui, |ui, _| {
                            let tint = (get(d, &[key, "tint"]).as_f64().unwrap_or(0.0) * 100.0).round();
                            if let Some(v) = number(ui, ("os_tint", key.to_owned()), &tr("Tint:"), tint, UnitKind::Percent, 0.0..=100.0, 0) {
                                set(d, &[key, "tint"], json!(v / 100.0));
                            }
                        });
                    });
                });
            }
            "corners" => {
                ui.add_enabled_ui(included(d, "corners"), |ui| {
                    Text::new(tr("Corner Options")).role(TypeRole::BodyStrong).show(ui);
                    Row::new().id_salt("os_corners").show(ui, |ui, _| {
                        Field::new(tr("Size:")).id_salt("os_corner_size_field").show(ui, |ui, _| {
                            let size = get(d, &["corners", "corners"])[0]["size"].as_f64().unwrap_or(0.0);
                            if let Some(v) = number(ui, "os_corner_size", &tr("Size:"), size, UnitKind::Points, 0.0..=10_000.0, 2) {
                                every_corner(d, "size", json!(v));
                            }
                        });
                        Field::new(tr("Shape:")).id_salt("os_corner_shape_field").show(ui, |ui, _| {
                            let cur: designcraft_geom::corners::CornerShape =
                                serde_json::from_value(get(d, &["corners", "corners"])[0]["shape"].clone()).unwrap_or_default();
                            let entries: Vec<Entry<designcraft_geom::corners::CornerShape>> =
                                designcraft_geom::corners::CornerShape::ALL.iter().map(|s| Entry::new(*s, tr(s.label()))).collect();
                            if let Some(s) = choose(ui, "os_corner_shape", &tr("Shape:"), cur, &entries) {
                                every_corner(d, "shape", json!(s));
                            }
                        });
                    });
                });
            }
            "paragraph" => {
                let Some(doc) = app.session.active().map(|st| &st.doc) else { return };
                ui.add_enabled_ui(included(d, "paragraph"), |ui| {
                    Field::new(tr("Paragraph Style:")).id_salt("os_paragraph_style_field").show(ui, |ui, _| {
                        let cur = d.s("paragraphStyle");
                        let entries: Vec<Entry<String>> = doc
                            .styles
                            .paragraph
                            .iter()
                            .map(|s| Entry::new(s.name.clone(), crate::i18n::style_name(lang, &s.name).to_owned()))
                            .collect();
                        let out = Select::new(tr("Paragraph Style:"), Bind::given(cur.clone()), &entries)
                            .id_salt("os_paragraph_style")
                            .sizing(Sizing::FillWidth)
                            .show(ui);
                        crate::control::object_style_witness::chooser(ui, &out.response);
                        if let Some(name) = edited(out.events, cur) {
                            d.fields.insert("paragraphStyle".into(), json!(name));
                        }
                    });
                    if let Some(on) = check(ui, "os_apply_next", &tr("Apply Next Style"), d.b("applyNext")) {
                        d.fields.insert("applyNext".into(), json!(on));
                    }
                });
            }
            "textFrame" => {
                // Text Frame Options › General on the framework: `frame_forms` comes with the Text Frame Options port
                // (wave2/dialogs-frame), merged together with this dialog.
                ui.add_enabled_ui(included(d, "textFrame"), |ui| crate::dialogs::frame_forms::general(app, ui, d));
            }
            p => crate::effects_dialog::form::page(app, ui, d, p),
        }
    }

    /// Sets `field` of every corner of the style's corner options.
    fn every_corner(d: &mut Dialog, field: &str, value: Value) {
        let mut c = get(d, &["corners"]).clone();
        if let Some(cs) = c["corners"].as_array_mut() {
            cs.iter_mut().for_each(|k| k[field] = value.clone());
        }
        d.fields.insert("corners".into(), c);
    }

    /// The swatches a Fill or Stroke page lists: the document's, hidden ones but the current one left out.
    struct Swatches(Vec<String>);

    impl craft_behavior::DisplayOrder<usize> for Swatches {
        fn len(&self) -> usize {
            self.0.len()
        }

        fn key(&self, at: usize) -> usize {
            at
        }

        fn index_of(&self, key: &usize) -> Option<usize> {
            (*key < self.0.len()).then_some(*key)
        }

        fn text(&self, at: usize) -> Option<std::borrow::Cow<'_, str>> {
            self.0.get(at).map(|name| std::borrow::Cow::Borrowed(name.as_str()))
        }
    }

    impl craft_behavior::ItemSource<usize> for Swatches {}

    /// The document's swatches as a list to pick from (InDesign's Fill and Stroke pages), each with its chip.
    fn swatches(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, key: &str) {
        let Some(doc) = app.session.active().map(|st| &st.doc) else { return };
        let cur = get(d, &[key, "swatch"]).as_str().unwrap_or("").to_owned();
        let names = Swatches(doc.swatches.iter().filter(|w| !w.hidden || w.name == cur).map(|w| w.name.clone()).collect());
        let mut selected = names.0.iter().position(|name| *name == cur).map_or_else(Selection::default, Selection::single);
        let label = crate::i18n::tr(&app.ui.language, if key == "fill" { "Fill" } else { "Stroke" }).to_owned();
        List::<usize>::new(label)
            .id_salt(("os_swatches", key.to_owned()))
            .select(SelectMode::Single)
            .arrows_select(true)
            .max_rows(10)
            .sizing(Sizing::FillWidth)
            .selection(Bind::bound(&mut selected))
            .show_items(ui, &names, &mut |at, _| {
                let name = names.0.get(*at).cloned().unwrap_or_default();
                let (color, gradient) = crate::widgets::swatch_colors(doc, &name, 1.0);
                ListRow::new(name).lead_painter(move |cx: &ContentPainterCx<'_>, slot: Rect| chip(cx, slot, color, gradient))
            });
        if let Some(name) = selected.items.first().and_then(|at| names.0.get(*at))
            && *name != cur
        {
            set(d, &[key, "swatch"], json!(name));
        }
    }

    /// A swatch's chip in a row's leading slot: its colour, the empty well of [None], or its gradient's ramp.
    fn chip(cx: &ContentPainterCx<'_>, slot: Rect, color: Option<Color32>, gradient: Option<(Color32, Color32)>) {
        let well = Rect::from_center_size(slot.center(), egui::Vec2::splat(slot.width().min(slot.height())));
        match gradient {
            Some((start, end)) => {
                cx.ramp(well, CornerRadius::ZERO, &[(0.0, DataColor::new(start)), (1.0, DataColor::new(end))], craft_behavior::Axis::X);
            }
            None => cx.with_draw(|draw| draw.swatch_well(well, CornerRadius::ZERO, color.map(craft_theme::Srgba::from))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new style made from an object with no effect on sets no effects, so applying it keeps
    /// other objects' own.
    #[test]
    fn a_new_style_without_effects_on_sets_none() {
        let (mut app, _) = app_with_frame();
        open_new(&mut app);
        let d = app.ui.dialog.clone().unwrap();
        assert!(d.b("hadEffects"));
        assert_eq!(attrs(&d)["effects"], Value::Null);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Plain"));
            d.fields.insert("apply".into(), json!(false));
        }
        crate::dialogs::confirm(&mut app).unwrap();
        assert!(app.session.doc().unwrap().doc.styles.object_style("Plain").unwrap().effects.is_none());
    }

    fn app_with_frame() -> (DesignApp, u64) {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 172, 172], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        (app, id)
    }

    /// Text Frame General Options › Vertical Justification: Justify with a Paragraph Spacing Limit
    /// sets both on the style and its frame, and the frame keeps its auto-size setting.
    #[test]
    fn the_general_options_page_sets_the_paragraph_spacing_limit() {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 336], "content": "text", "caret": false})).unwrap();
        let id = r["id"].as_u64().unwrap();
        app.run("object.textFrameOptions", json!({"ids": [id], "autoSize": "heightOnly"})).unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        open_new(&mut app);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Justified"));
            d.fields.insert("verticalJustification".into(), json!("justify"));
            d.fields.insert("paragraphSpacingLimit".into(), json!("6 pt"));
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let doc = app.session.active().unwrap().doc.clone();
        let tf = doc.styles.object_style("Justified").and_then(|s| s.text_frame.clone()).unwrap();
        assert_eq!((tf.vertical_justification, tf.vj_paragraph_spacing_limit), (designcraft_doc::VerticalJustification::Justify, 6.0));
        app.run("style.object.apply", json!({"name": "Justified", "ids": [id]})).unwrap();
        let o = app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().text_frame().unwrap().options.clone();
        assert_eq!((o.vj_paragraph_spacing_limit, o.auto_size), (6.0, designcraft_doc::AutoSize::HeightOnly));
    }

    /// `objectStyle.dialog` with a style the document lacks, or `[None]`, is an error and opens
    /// nothing; without a name it opens New Object Style.
    #[test]
    fn the_dialog_command_refuses_a_style_with_no_options() {
        let (mut app, _) = app_with_frame();
        for name in ["Nope", designcraft_doc::NO_OBJECT_STYLE] {
            assert!(app.run("objectStyle.dialog", json!({"name": name})).is_err(), "{name}");
            assert!(app.ui.dialog.is_none(), "{name}");
        }
        app.run("objectStyle.dialog", json!({})).unwrap();
        assert!(app.ui.dialog.as_ref().is_some_and(|d| d.b("new")));
    }

    fn app_with_text_frame(units: &str) -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("document.preferences", json!({"horizontalUnits": units, "verticalUnits": units})).unwrap();
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 336], "content": "text", "caret": false})).unwrap();
        app.run("selection.set", json!({"ids": [r["id"]]})).unwrap();
        app
    }

    /// Object Style Options keeps a General measurement the user did not edit exactly as the style
    /// has it, though the field shows it rounded.
    #[test]
    fn general_measurements_left_alone_keep_their_value() {
        let mut app = app_with_text_frame("points");
        let tf = json!({"gutter": 2.123456, "inset": [1.23456, 2.34567, 3.45678, 4.56789]});
        app.run("style.object.create", json!({"name": "Precise", "fromSelection": false, "textFrame": tf})).unwrap();
        open_edit(&mut app, "Precise").unwrap();
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Precise Too"));
            d.fields.insert("insetTop".into(), json!("7 pt"));
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let o = app.session.active().unwrap().doc.styles.object_style("Precise Too").and_then(|s| s.text_frame.clone()).unwrap();
        assert_eq!((o.gutter, o.inset), (2.123456, [7.0, 2.34567, 3.45678, 4.56789]));
    }

    /// Object Style Options and New Object Style act only on the document they opened for. With
    /// another document active, after their document closed, or after it was reverted, OK keeps
    /// the dialog open with the reason and changes no style.
    #[test]
    fn the_dialog_acts_only_on_the_document_it_opened_for() {
        let name = designcraft_doc::BASIC_GRAPHICS_FRAME;
        let weights = |app: &DesignApp| -> Vec<f64> {
            app.session
                .documents()
                .iter()
                .map(|st| st.doc.styles.object_style(name).and_then(|s| s.stroke.as_ref()).map_or(-1.0, |s| s.weight))
                .collect()
        };
        let edit = |app: &mut DesignApp| {
            open_edit(app, name).unwrap();
            if let Some(d) = app.ui.dialog.as_mut() {
                set(d, &["stroke", "weight"], json!(9.0));
            }
        };
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"title": "A"})).unwrap();
        app.run("file.new", json!({"title": "B"})).unwrap();
        app.session.set_active(0);
        let before = weights(&app);
        assert_eq!(before.len(), 2);

        // Options opened on A, OK with B active: B has a style of the same name.
        edit(&mut app);
        app.session.set_active(1);
        let e = crate::dialogs::confirm(&mut app).unwrap_err();
        assert!(e.contains("active document changed"), "{e}");
        assert!(app.ui.dialog.as_ref().is_some_and(|d| d.s("status") == e), "the dialog stays open with the reason");
        assert_eq!(weights(&app), before, "neither document's style changed");

        // New Object Style opened on A makes nothing in B, and OK works once A is active again.
        app.ui.dialog = None;
        app.session.set_active(0);
        open_new(&mut app);
        app.session.set_active(1);
        assert!(crate::dialogs::confirm(&mut app).is_err());
        assert!(app.session.documents().iter().all(|st| st.doc.styles.object_style("Object Style 1").is_none()));
        app.session.set_active(0);
        crate::dialogs::confirm(&mut app).unwrap();
        assert!(app.session.documents()[0].doc.styles.object_style("Object Style 1").is_some());
        assert!(app.session.documents()[1].doc.styles.object_style("Object Style 1").is_none());

        // Options opened on A, A closed: B is active and keeps its style.
        edit(&mut app);
        app.session.execute("file.close", &json!({"index": 0})).unwrap();
        assert!(crate::dialogs::confirm(&mut app).is_err());
        assert_eq!(weights(&app), vec![before[1]]);
        app.ui.dialog = None;

        // Options opened on B, B reverted: the reverted document keeps its style.
        {
            let dir = tempfile::tempdir().unwrap();
            app.run("file.saveAs", json!({"path": dir.path().join("revert.designcraft")})).unwrap();
            app.run("frame.create", json!({"rect": [72, 72, 144, 144], "content": "graphic"})).unwrap();
            edit(&mut app);
            app.run("file.revert", json!({})).unwrap();
            assert!(crate::dialogs::confirm(&mut app).is_err());
            assert_eq!(weights(&app), vec![before[1]]);
        }
    }

    /// Apply Style to Selection applies the new style to the objects selected when the dialog
    /// opened, not to a selection made while it was open.
    #[test]
    fn apply_style_to_selection_uses_the_selection_the_dialog_opened_with() {
        let (mut app, a) = app_with_frame();
        let b = app.run("frame.create", json!({"rect": [200, 72, 300, 172], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [a]})).unwrap();
        open_new(&mut app);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Picked"));
        }
        app.run("selection.set", json!({"ids": [b]})).unwrap();
        crate::dialogs::confirm(&mut app).unwrap();
        let style = |id: u64| app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().object_style.clone();
        assert_eq!(style(a), "Picked");
        assert_ne!(style(b), "Picked");
    }

    /// Real installed controls, fed through the shipping raw-input owner and dialog dispatcher.
    /// These source-authored scenarios are not a native execution receipt.
    mod framework_workflow {
        use super::*;
        use craft_ui_widgets::runtime::inspect::{Recorded, record};
        use egui::accesskit::{NodeId, Role, TreeUpdate};
        use egui::{Event, Key, Modifiers, PointerButton};
        use std::sync::Arc;

        type History = Vec<(String, Arc<designcraft_doc::Document>, designcraft_doc::Selection)>;
        #[derive(Clone, Debug, PartialEq)]
        struct DocumentState {
            uid: u64,
            epoch: u64,
            revision: u64,
            doc: Arc<designcraft_doc::Document>,
            saved: Arc<designcraft_doc::Document>,
            selected: designcraft_doc::Selection,
            undo: History,
            redo: History,
        }
        #[derive(Clone, Debug, PartialEq)]
        struct Effects {
            documents: Vec<DocumentState>,
            active: Option<u64>,
            journal: Vec<(String, Value)>,
        }
        fn effects(app: &DesignApp) -> Effects {
            let history = |entries: &[designcraft_engine::HistoryEntry]| -> History {
                entries.iter().map(|e| (e.label.clone(), e.doc.clone(), e.selection.clone())).collect()
            };
            Effects {
                documents: app.session.documents().iter().map(|st| DocumentState {
                    uid: st.uid, epoch: st.identity_epoch, revision: st.revision, doc: st.doc.clone(),
                    saved: st.saved_doc.clone(), selected: st.selection.clone(),
                    undo: history(&st.history.undo), redo: history(&st.history.redo),
                }).collect(),
                active: app.session.active().map(|st| st.uid),
                journal: app.session.journal.clone(),
            }
        }
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Pass { Host, Body, Measure, Sizing, Disabled }
        struct Workflow {
            app: DesignApp,
            ctx: egui::Context,
            recorded: Option<Arc<Recorded>>,
            ax: Option<TreeUpdate>,
            time: f64,
            pass: Pass,
            guarded: usize,
        }
        impl Workflow {
            fn new(mut app: DesignApp) -> Self {
                app.ui.language = "en".into();
                let ctx = egui::Context::default();
                ctx.enable_accesskit();
                app.install_framework(&ctx);
                app.raw_input_hook(&ctx, &mut egui::RawInput::default());
                // Initialize fonts in an actual headless frame and explicitly dispose its GPU deltas.
                let out = ctx.run_ui(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1100.0))),
                    max_texture_side: Some(8192), ..Default::default()
                }, |ui| { crate::theme::install_fonts(ui.ctx(), ""); });
                out.drop_without_applying_deltas();
                let mut w = Self { app, ctx, recorded: None, ax: None, time: 1.0, pass: Pass::Host, guarded: 0 };
                w.idle();
                w
            }
            fn frame(&mut self, events: Vec<Event>) {
                self.time += 0.05;
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1100.0))),
                    max_texture_side: Some(8192), time: Some(self.time), events, ..Default::default()
                };
                self.app.raw_input_hook(&self.ctx, &mut input);
                let ctx = self.ctx.clone();
                let mut out = ctx.run_ui(input, |ui| {
                    self.recorded = record(ui, egui::Id::unique("object-style-workflow"), |ui| {
                        if self.pass == Pass::Host {
                            crate::dialogs::show(&mut self.app, &ctx);
                        } else {
                            let builder = if matches!(self.pass, Pass::Measure | Pass::Sizing) { egui::UiBuilder::new().sizing_pass() } else { egui::UiBuilder::new() };
                            ui.scope_builder(builder, |ui| {
                                ui.add_enabled_ui(self.pass != Pass::Disabled, |ui| {
                                    let mut d = self.app.ui.dialog.clone().unwrap();
                                    let before = effects(&self.app);
                                    let fields = d.fields.clone();
                                    let guarded = matches!(self.pass, Pass::Measure | Pass::Sizing | Pass::Disabled);
                                    if self.pass == Pass::Measure {
                                        craft_ui_widgets::runtime::measure::run(&ctx, || {
                                            assert!(craft_ui_widgets::runtime::measure::active(&ctx));
                                            assert!(ui.is_sizing_pass(), "SDK measurement pairs its context guard with a sizing Ui");
                                            framework::show(&mut self.app, ui, &mut d);
                                            assert_eq!(d.fields, fields, "measurement must not consume draft events inside the guarded callback");
                                            assert_eq!(effects(&self.app), before, "measurement callback must not mutate any document/history/journal");
                                            self.guarded += 1;
                                        });
                                    } else {
                                        if self.pass == Pass::Sizing { assert!(ui.is_sizing_pass()); }
                                        if self.pass == Pass::Disabled { assert!(!ui.is_enabled()); }
                                        framework::show(&mut self.app, ui, &mut d);
                                        if guarded {
                                            assert_eq!(d.fields, fields, "{:#?}: guard must refuse edits during the callback", self.pass);
                                            assert_eq!(effects(&self.app), before);
                                            self.guarded += 1;
                                        }
                                    }
                                    self.app.ui.dialog = Some(d);
                                });
                            });
                        }
                    }).1;
                });
                self.ax = out.platform_output.accesskit_update.take();
                out.drop_without_applying_deltas();
            }
            fn idle(&mut self) { for _ in 0..8 { self.frame(Vec::new()); } }
            fn key_with(&mut self, key: Key, modifiers: Modifiers) {
                for pressed in [true, false] {
                    self.frame(vec![Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }]);
                }
                self.frame(Vec::new());
            }
            fn key(&mut self, key: Key) { self.key_with(key, Modifiers::NONE); }
            fn sdk(&self, role: Role, name: &str) -> (egui::Id, egui::Rect) {
                let (ax_id, _) = self.ax_control(role, name);
                let role = format!("{role:?}");
                let matches: Vec<_> = self.recorded.as_ref().unwrap().elements.iter().filter(|e| {
                    // Hidden Field labels name the actual AX node, not the flat recorded facts.
                    e.facts.as_ref().is_some_and(|f| f.role == role) && e.id.accesskit_id() == ax_id
                }).collect();
                assert_eq!(matches.len(), 1, "exactly one real SDK owner for {role} / {name}");
                (matches[0].id, matches[0].rect)
            }
            fn ax_control(&self, role: Role, name: &str) -> (NodeId, egui::Rect) {
                let matches: Vec<_> = self.ax.as_ref().unwrap().nodes.iter().filter(|(_, n)| n.role() == role && n.label() == Some(name)).collect();
                assert_eq!(matches.len(), 1, "exactly one real AX owner for {role:?} / {name}");
                let bounds = matches[0].1.bounds().unwrap();
                (matches[0].0, egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32), egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                ))
            }
            fn pointer(&mut self, at: egui::Pos2, pressed: bool) {
                self.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
            }
            fn click_at(&mut self, at: egui::Pos2) {
                self.frame(vec![Event::PointerMoved(at)]);
                self.pointer(at, true);
                self.pointer(at, false);
                self.idle();
            }
            fn click(&mut self, role: Role, name: &str) {
                let (id, rect) = self.sdk(role, name);
                let (_, ax_rect) = self.ax_control(role, name);
                assert!(rect.intersects(ax_rect), "recorded and actual AX bounds must refer to the same visible owner");
                self.click_at(rect.center());
                if self.app.ui.dialog.is_some() {
                    assert_eq!(self.sdk(role, name).0, id, "ordinary interaction keeps the control's identity");
                }
            }
            fn footer(&mut self, name: &str) {
                let (_, rect) = self.ax_control(Role::Button, name);
                self.click_at(rect.center());
            }
            fn tab_to(&mut self, role: Role, name: &str) {
                let id = self.ax_control(role, name).0;
                let mut visited = std::collections::HashSet::new();
                while self.ax.as_ref().unwrap().focus != id {
                    assert!(visited.insert(self.ax.as_ref().unwrap().focus), "natural Tab must reach {role:?} / {name}; no forced focus");
                    self.key(Key::Tab);
                }
            }
            fn name(&mut self, text: &str) {
                self.click(Role::TextInput, "Style Name:");
                self.key_with(Key::A, Modifiers::COMMAND);
                self.key(Key::Backspace);
                if !text.is_empty() { self.frame(vec![Event::Text(text.into())]); }
                self.idle();
                assert_eq!(self.app.ui.dialog.as_ref().unwrap().s("name"), text, "the real text owner updates only the draft");
            }
            fn open(&mut self, name: Option<&str>) {
                let p = name.map_or(json!({}), |name| json!({"name": name}));
                self.app.run("objectStyle.dialog", p).unwrap();
                self.pass = Pass::Host;
                self.idle();
            }
            fn quiet(&mut self, before: &Effects) {
                self.idle();
                assert_eq!(&effects(&self.app), before, "no deferred command or document/history change on ordinary idle");
            }
        }
        fn object_style(app: &DesignApp, name: &str) -> designcraft_doc::ObjectStyle {
            app.session.active().unwrap().doc.styles.object_style(name).unwrap().clone()
        }
        fn item_style(app: &DesignApp, id: u64) -> String {
            app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().object_style.clone()
        }

        #[test]
        fn create_apply_and_rename_use_the_real_controls_captured_targets_and_two_undo_steps() {
            let (mut app, a) = app_with_frame();
            let b = app.run("frame.create", json!({"rect": [200, 72, 300, 172], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
            app.run("selection.set", json!({"ids": [a]})).unwrap();
            let mut w = Workflow::new(app);
            w.open(None);
            let before = effects(&w.app);
            let base = w.app.session.active().unwrap().doc.clone();
            let old_saved = w.app.session.active().unwrap().saved_doc.clone();
            let undo = w.app.session.active().unwrap().history.undo.len();
            let journal = w.app.session.journal.len();
            w.name("Workflow");
            w.tab_to(Role::CheckBox, "Fill");
            w.key(Key::Space);
            assert!(!included(w.app.ui.dialog.as_ref().unwrap(), "fill"));
            w.click(Role::RadioButton, "Stroke");
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("page"), "stroke");
            w.tab_to(Role::RadioButton, "Stroke");
            w.key(Key::Enter);
            assert!(w.app.ui.dialog.is_some(), "the page radio, not the footer, owns Enter");
            w.click(Role::CheckBox, "Drop Shadow");
            assert!(crate::effects_dialog::is_on(&w.app.ui.dialog.as_ref().unwrap().fields["fx"], "dropShadow"));
            w.click(Role::RadioButton, "General");
            assert_eq!(effects(&w.app), before, "name/includes/navigation are strictly draft-only");
            w.click(Role::CheckBox, "Apply Style to Selection");
            assert!(!w.app.ui.dialog.as_ref().unwrap().b("apply"));
            w.tab_to(Role::CheckBox, "Apply Style to Selection");
            w.key(Key::Space);
            assert!(w.app.ui.dialog.as_ref().unwrap().b("apply"));
            assert_eq!(effects(&w.app), before);
            // A new selection while the dialog is open is deliberately not an apply target.
            w.app.run("selection.set", json!({"ids": [b]})).unwrap();
            let applying = effects(&w.app);
            w.quiet(&applying);
            w.footer("OK");
            assert!(w.app.ui.dialog.is_none());
            let os = object_style(&w.app, "Workflow");
            assert!(os.fill.is_none() && os.stroke.is_some() && os.effects.as_ref().unwrap().drop_shadow.on);
            assert_eq!(item_style(&w.app, a), "Workflow");
            assert_ne!(item_style(&w.app, b), "Workflow");
            let st = w.app.session.active().unwrap();
            assert_eq!(st.history.undo.len(), undo + 2, "create and optional apply remain two real engine commands");
            assert!(st.history.redo.is_empty());
            assert_eq!(st.saved_doc.as_ref(), old_saved.as_ref());
            assert!(st.revision > applying.documents[0].revision);
            let commands: Vec<_> = w.app.session.journal[journal..].iter().map(|(id, _)| id.as_str()).collect();
            assert_eq!(commands, ["selection.set", "style.object.create", "style.object.apply"]);
            assert_eq!(w.app.session.journal.last().unwrap().1["ids"], json!([a]));
            let made = w.app.session.active().unwrap().doc.clone();
            let settled = effects(&w.app);
            w.quiet(&settled);
            w.app.run("edit.undo", json!({})).unwrap();
            assert!(w.app.session.active().unwrap().doc.styles.object_style("Workflow").is_some());
            assert_eq!(item_style(&w.app, a), base.item(designcraft_doc::ItemId(a)).unwrap().object_style);
            assert_eq!(w.app.session.active().unwrap().history.redo.len(), 1);
            w.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(w.app.session.active().unwrap().doc.as_ref(), base.as_ref());
            assert_eq!(w.app.session.active().unwrap().history.redo.len(), 2);
            for _ in 0..2 { w.app.run("edit.redo", json!({})).unwrap(); }
            assert_eq!(w.app.session.active().unwrap().doc.as_ref(), made.as_ref());
            w.open(Some("Workflow"));
            let edit_before = effects(&w.app);
            w.name("Renamed Workflow");
            assert_eq!(effects(&w.app), edit_before);
            w.key(Key::Enter); // A field submit goes through the existing host/controller.
            assert!(w.app.ui.dialog.is_none());
            assert!(w.app.session.active().unwrap().doc.styles.object_style("Workflow").is_none());
            assert_eq!(item_style(&w.app, a), "Renamed Workflow");
            assert_eq!(w.app.session.active().unwrap().history.undo.len(), edit_before.documents[0].undo.len() + 1);
            let renamed = w.app.session.active().unwrap().doc.clone();
            w.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(w.app.session.active().unwrap().doc.as_ref(), made.as_ref());
            w.app.run("edit.redo", json!({})).unwrap();
            assert_eq!(w.app.session.active().unwrap().doc.as_ref(), renamed.as_ref());
            assert_eq!(w.app.session.active().unwrap().saved_doc.as_ref(), old_saved.as_ref());
            let final_state = effects(&w.app);
            w.quiet(&final_state);
        }

        #[test]
        fn empty_duplicate_names_and_pointer_and_keyboard_cancel_leave_documents_and_history_untouched() {
            let (mut app, _) = app_with_frame();
            app.run("style.object.create", json!({"name": "Taken", "fromSelection": false})).unwrap();
            app.run("style.object.create", json!({"name": "Mine", "fromSelection": false})).unwrap();
            let mut w = Workflow::new(app);
            w.open(None);
            let before = effects(&w.app);
            w.name("");
            w.footer("OK");
            let d = w.app.ui.dialog.as_ref().unwrap();
            assert_eq!(d.s("name"), "");
            assert_eq!(d.s("status"), "Enter a style name.");
            w.quiet(&before);
            w.name("Taken");
            w.click(Role::CheckBox, "Stroke");
            let draft = w.app.ui.dialog.as_ref().unwrap().fields.clone();
            // The checkbox still owns Enter; submit through the visible footer instead.
            w.footer("OK");
            let d = w.app.ui.dialog.as_ref().unwrap();
            assert!(d.s("status").contains("already named"));
            assert_eq!(d.fields["inc"], draft["inc"]);
            assert_eq!(d.s("name"), "Taken");
            w.quiet(&before);
            w.footer("Cancel");
            assert!(w.app.ui.dialog.is_none());
            w.quiet(&before);
            w.open(None);
            w.name("Canceled by keyboard");
            w.tab_to(Role::Button, "Cancel");
            w.key(Key::Space);
            assert!(w.app.ui.dialog.is_none());
            w.quiet(&before);
            w.open(None);
            w.name("Canceled field draft");
            // One Escape cancels the field's draft and the dialog with it (the framework's dialog rule, as in InDesign).
            w.key(Key::Escape);
            assert!(w.app.ui.dialog.is_none(), "one Escape over a text draft cancels the draft and the dialog");
            w.quiet(&before);
            w.open(None);
            w.click(Role::TextInput, "Style Name:");
            w.key(Key::Escape);
            assert!(w.app.ui.dialog.is_none(), "an idle naturally focused SDK field passes one Escape to its host");
            w.quiet(&before);
            // An edit rename collision follows the same retained-status/draft path.
            w.open(Some("Mine"));
            w.name("Taken");
            w.footer("OK");
            assert!(w.app.ui.dialog.as_ref().unwrap().s("status").contains("already named"));
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("name"), "Taken");
            w.quiet(&before);
            w.footer("Cancel");
            w.quiet(&before);
        }

        #[test]
        fn switched_and_reverted_documents_refuse_ok_and_surviving_open_targets_ignore_a_later_selection() {
            let (mut app, a) = app_with_frame();
            app.run("file.new", json!({"title": "Other document"})).unwrap();
            app.session.set_active(0);
            let mut w = Workflow::new(app);
            w.open(None);
            w.name("Stale uid");
            w.app.session.set_active(1);
            let switched = effects(&w.app);
            w.idle();
            w.footer("OK");
            assert!(w.app.ui.dialog.as_ref().unwrap().s("status").contains("active document changed"));
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("name"), "Stale uid");
            w.quiet(&switched);
            w.footer("Cancel");
            w.app.session.set_active(0);
            let dir = tempfile::tempdir().unwrap();
            w.app.run("file.saveAs", json!({"path": dir.path().join("object-style-identity.designcraft")})).unwrap();
            // Real Revert requires a saved dirty document; dialog drafts do not dirty it.
            w.app.run("frame.create", json!({"rect": [500, 72, 600, 172], "content": "graphic"})).unwrap();
            assert!(w.app.session.active().unwrap().is_dirty());
            w.open(None);
            w.name("Stale epoch");
            let opened = w.app.ui.dialog.as_ref().unwrap().fields["doc"].clone();
            w.app.run("file.revert", json!({})).unwrap();
            assert_ne!(document_key(&w.app), opened, "real revert changes the captured identity epoch");
            let reverted = effects(&w.app);
            w.idle();
            w.footer("OK");
            assert!(w.app.ui.dialog.as_ref().unwrap().s("status").contains("active document changed"));
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("name"), "Stale epoch");
            w.quiet(&reverted);
            w.footer("Cancel");
            let b = w.app.run("frame.create", json!({"rect": [200, 72, 300, 172], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
            let c = w.app.run("frame.create", json!({"rect": [350, 72, 450, 172], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
            w.app.run("selection.set", json!({"ids": [a, b]})).unwrap();
            w.open(None);
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().fields["targets"], json!([a, b]));
            w.name("Surviving targets");
            w.app.run("edit.clear", json!({"ids": [b]})).unwrap();
            w.app.run("selection.set", json!({"ids": [c]})).unwrap();
            let before_apply = w.app.session.active().unwrap().doc.clone();
            let undo = w.app.session.active().unwrap().history.undo.len();
            w.idle();
            w.footer("OK");
            assert!(w.app.ui.dialog.is_none());
            assert_eq!(item_style(&w.app, a), "Surviving targets");
            assert_ne!(item_style(&w.app, c), "Surviving targets");
            assert!(w.app.session.active().unwrap().doc.item(designcraft_doc::ItemId(b)).is_none());
            assert_eq!(w.app.session.journal.last().unwrap().1["ids"], json!([a]));
            assert_eq!(w.app.session.active().unwrap().history.undo.len(), undo + 2);
            assert_eq!(w.app.session.active().unwrap().selection.items, vec![designcraft_doc::ItemId(c)]);
            for _ in 0..2 { w.app.run("edit.undo", json!({})).unwrap(); }
            assert_eq!(w.app.session.active().unwrap().doc.as_ref(), before_apply.as_ref(), "undoing create/apply never resurrects a target deleted before OK");
        }

        #[test]
        fn locked_open_target_characterizes_the_existing_public_apply_path_without_recreating_on_idle() {
            let (app, a) = app_with_frame();
            let mut w = Workflow::new(app);
            w.open(None);
            w.name("Locked target characterization");
            w.app.run("object.lock", json!({"ids": [a]})).unwrap();
            assert!(w.app.session.active().unwrap().doc.item(designcraft_doc::ItemId(a)).unwrap().locked);
            let undo = w.app.session.active().unwrap().history.undo.len();
            let journal = w.app.session.journal.len();
            w.idle();
            w.footer("OK");
            assert!(w.app.ui.dialog.is_none());
            // Source-derived baseline behavior, not a new lock policy: apply_object uses explicit
            // targets and Session::edit has no lock rejection. The historical dialog comment is
            // not evidence of a refusal. This scenario still needs actual native execution.
            assert_eq!(item_style(&w.app, a), "Locked target characterization");
            assert!(w.app.session.active().unwrap().doc.item(designcraft_doc::ItemId(a)).unwrap().locked);
            assert_eq!(w.app.session.active().unwrap().history.undo.len(), undo + 2);
            assert_eq!(w.app.session.journal[journal..].iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["style.object.create", "style.object.apply"]);
            assert_eq!(w.app.session.active().unwrap().doc.styles.object.iter().filter(|s| s.name == "Locked target characterization").count(), 1);
            let settled = effects(&w.app);
            w.quiet(&settled);
            w.key(Key::Enter);
            w.quiet(&settled);
        }

        #[test]
        fn real_press_measurement_native_sizing_and_disabled_release_are_inert_then_same_owner_works() {
            let (app, _) = app_with_frame();
            let mut w = Workflow::new(app);
            w.open(None);
            w.name("Guarded draft");
            w.pass = Pass::Body;
            w.idle();
            for guard in [Pass::Measure, Pass::Sizing, Pass::Disabled] {
                let before = effects(&w.app);
                let fields = w.app.ui.dialog.as_ref().unwrap().fields.clone();
                let (owner, rect) = w.sdk(Role::CheckBox, "Fill");
                let callbacks = w.guarded;
                w.frame(vec![Event::PointerMoved(rect.center())]);
                w.pointer(rect.center(), true); // A real ordinary press on this same owner.
                assert_eq!(w.app.ui.dialog.as_ref().unwrap().fields, fields, "press alone does not toggle");
                w.pass = guard;
                w.pointer(rect.center(), false); // Release is delivered during the actual public guard.
                assert!(w.guarded > callbacks, "the guarded body really ran");
                assert_eq!(w.app.ui.dialog.as_ref().unwrap().fields, fields);
                assert_eq!(effects(&w.app), before);
                w.pass = Pass::Body;
                w.quiet(&before);
                assert_eq!(w.app.ui.dialog.as_ref().unwrap().fields, fields, "guarded release cannot replay on an ordinary idle frame");
                assert_eq!(w.sdk(Role::CheckBox, "Fill").0, owner);
                w.click(Role::CheckBox, "Fill");
                assert_eq!(included(w.app.ui.dialog.as_ref().unwrap(), "fill"), !fields["inc"]["fill"].as_bool().unwrap());
                assert_eq!(effects(&w.app), before, "same-owner positive changes only the draft");
                w.click(Role::CheckBox, "Fill");
                assert_eq!(w.app.ui.dialog.as_ref().unwrap().fields, fields, "the ordinary positive can restore the exact draft");
            }
            w.pass = Pass::Host;
            w.idle();
            let before = effects(&w.app);
            let fields = w.app.ui.dialog.as_ref().unwrap().fields.clone();
            let at = w.sdk(Role::CheckBox, "Fill").1.center();
            w.frame(vec![Event::PointerMoved(at)]);
            w.pointer(at, true);
            let away = egui::pos2(at.x + 300.0, at.y + 70.0);
            w.frame(vec![Event::PointerMoved(away)]);
            w.pointer(away, false);
            w.quiet(&before);
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().fields, fields, "dragging a press away cancels inclusion");
            w.click(Role::CheckBox, "Fill");
            assert_ne!(w.app.ui.dialog.as_ref().unwrap().fields["inc"]["fill"], fields["inc"]["fill"]);
            w.footer("Cancel");
            assert!(w.app.ui.dialog.is_none());
            w.quiet(&before);
        }

        #[test]
        fn closing_dropdown_returns_anchor_enter_before_its_exit_fade() {
            let mut app = app_with_text_frame("points");
            app.run("style.paragraph.create", json!({"name": "Workflow Body", "fromSelection": false})).unwrap();
            let mut w = Workflow::new(app);
            w.open(None);
            w.click(Role::RadioButton, "Paragraph Styles");
            assert!(included(w.app.ui.dialog.as_ref().unwrap(), "paragraph"));
            let before = effects(&w.app);
            let initial_style = w.app.ui.dialog.as_ref().unwrap().s("paragraphStyle");
            w.click(Role::ComboBox, "Paragraph Style:");
            w.key(Key::ArrowDown);
            w.key(Key::Escape);
            assert!(w.app.ui.dialog.is_some(), "the popup owns Escape, not its host");
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("paragraphStyle"), initial_style);
            assert_eq!(effects(&w.app), before);
            let combo_id = w.ax_control(Role::ComboBox, "Paragraph Style:").0;
            assert_eq!(w.ax.as_ref().unwrap().nodes.iter().find(|(id, _)| *id == combo_id).unwrap().1.is_expanded(), Some(false));
            // No idle, recovery Tab, synthetic focus, or second opening click before Enter.
            w.key(Key::Enter);
            assert_eq!(w.ax.as_ref().unwrap().nodes.iter().find(|(id, _)| *id == combo_id).unwrap().1.is_expanded(), Some(true), "a closing dropdown must yield fresh Enter to its own anchor immediately");
            assert!(w.app.ui.dialog.is_some());
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("paragraphStyle"), initial_style);
            assert_eq!(effects(&w.app), before);
            w.key(Key::Escape);
            w.idle();
            assert!(!w.ax.as_ref().unwrap().nodes.iter().any(|(_, n)| n.role() == Role::ListBox));
            w.footer("Cancel");
            assert!(w.app.ui.dialog.is_none());
            w.quiet(&before);
        }

        #[test]
        fn paragraph_popup_apply_next_disabled_inclusion_and_precise_omitted_options_survive_the_workflow() {
            let mut app = app_with_text_frame("points");
            app.run("style.paragraph.create", json!({"name": "Workflow Body", "fromSelection": false})).unwrap();
            let tf = json!({"gutter": 2.123456, "inset": [1.23456, 2.34567, 3.45678, 4.56789]});
            app.run("style.object.create", json!({"name": "Precise", "fromSelection": false, "applyNextStyle": true, "textFrame": tf})).unwrap();
            let original = object_style(&app, "Precise");
            assert!(original.effects.is_none() && original.paragraph_style.is_none() && original.apply_next_style);
            let mut w = Workflow::new(app);
            w.open(None);
            w.name("Paragraph workflow");
            let before = effects(&w.app);
            w.click(Role::RadioButton, "Paragraph Styles");
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("page"), "paragraph");
            w.click(Role::CheckBox, "Paragraph Styles");
            assert!(!included(w.app.ui.dialog.as_ref().unwrap(), "paragraph"));
            let fields = w.app.ui.dialog.as_ref().unwrap().fields.clone();
            let next_node = w.ax_control(Role::CheckBox, "Apply Next Style").0;
            assert!(w.ax.as_ref().unwrap().nodes.iter().find(|(id, _)| *id == next_node).unwrap().1.is_disabled());
            w.click(Role::CheckBox, "Apply Next Style");
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().fields, fields, "a visible disabled editor cannot change any draft");
            w.quiet(&before);
            w.click(Role::CheckBox, "Paragraph Styles");
            assert!(included(w.app.ui.dialog.as_ref().unwrap(), "paragraph"));
            let initial_style = w.app.ui.dialog.as_ref().unwrap().s("paragraphStyle");
            w.click(Role::ComboBox, "Paragraph Style:");
            assert!(w.ax.as_ref().unwrap().nodes.iter().any(|(_, n)| n.role() == Role::ListBox));
            w.key(Key::ArrowDown);
            w.key(Key::Escape);
            assert!(w.app.ui.dialog.is_some(), "the popup owns Escape before the host");
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("paragraphStyle"), initial_style);
            assert_eq!(effects(&w.app), before);
            let combo_id = w.ax_control(Role::ComboBox, "Paragraph Style:").0;
            assert_eq!(w.ax.as_ref().unwrap().nodes.iter().find(|(id, _)| *id == combo_id).unwrap().1.is_expanded(), Some(false), "Escape logically collapses the combo before its paint fade ends");
            // A closed layer retains hidden raw AX descendants during its exit fade.
            w.idle();
            assert!(!w.ax.as_ref().unwrap().nodes.iter().any(|(_, n)| n.role() == Role::ListBox));
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("paragraphStyle"), initial_style);
            w.quiet(&before);
            w.click(Role::ComboBox, "Paragraph Style:");
            let option = w.ax_control(Role::ListBoxOption, "Workflow Body").1.center();
            w.click_at(option);
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("paragraphStyle"), "Workflow Body");
            w.tab_to(Role::CheckBox, "Apply Next Style");
            w.key(Key::Space);
            assert!(w.app.ui.dialog.as_ref().unwrap().b("applyNext"));
            assert_eq!(effects(&w.app), before);
            w.footer("OK");
            let created = object_style(&w.app, "Paragraph workflow");
            assert_eq!(created.paragraph_style.as_deref(), Some("Workflow Body"));
            assert!(created.apply_next_style);
            w.open(Some("Precise"));
            let before_rename = effects(&w.app);
            w.click(Role::RadioButton, "Text Frame General Options");
            w.idle();
            w.click(Role::RadioButton, "General");
            w.name("Precise renamed");
            assert_eq!(effects(&w.app), before_rename);
            w.footer("OK");
            assert!(w.app.ui.dialog.is_none());
            let mut expected = original;
            expected.name = "Precise renamed".into();
            let actual = object_style(&w.app, "Precise renamed");
            assert_eq!(actual, expected, "rename preserves omitted effects/paragraph/apply-next and every exact text-frame value");
            assert_eq!(w.app.session.active().unwrap().history.undo.len(), before_rename.documents[0].undo.len() + 1);
            let renamed = w.app.session.active().unwrap().doc.clone();
            w.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(w.app.session.active().unwrap().doc.as_ref(), before_rename.documents[0].doc.as_ref());
            w.app.run("edit.redo", json!({})).unwrap();
            assert_eq!(w.app.session.active().unwrap().doc.as_ref(), renamed.as_ref());
            let settled = effects(&w.app);
            w.quiet(&settled);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod successor_tests;
