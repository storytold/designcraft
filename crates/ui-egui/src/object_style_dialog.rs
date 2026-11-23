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
use crate::theme::semibold;

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

/// Native controls consume SDK events into this dialog's existing draft only.
fn framework(app: &DesignApp) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    { app.framework_installed }
    #[cfg(target_arch = "wasm32")]
    { let _ = app; false }
}

fn draft_check(ui: &mut egui::Ui, native: bool, label: &str, key: &str, value: &mut bool) {
    #[cfg(not(target_arch = "wasm32"))]
    if native {
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
        let out = craft_ui_widgets::choice::Checkbox::new(Bind::given(*value))
            .label(label.to_owned()).id_salt(("object_style_check", key)).show(ui);
        for event in out.events { event.apply(value); }
        return;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (native, key);
    ui.checkbox(value, crate::rtl::widget(ui, label));
}

/// A row of the left-hand lists: a checkbox (when `check` is given) and the label, which shows
/// `page`.
fn list_row(ui: &mut egui::Ui, d: &mut Dialog, page: &str, label: &str, check: Option<bool>, native: bool) -> Option<bool> {
    #[cfg(not(target_arch = "wasm32"))]
    if native {
        use craft_ui_widgets::contract::{Bind, LabelKind, Props as _, Show as _, ShowContainer as _, SideLabel, Sizing};
        use craft_ui_widgets::choice::{Checkbox, Radio};
        use craft_ui_widgets::form::Field;
        use craft_ui_widgets::layout_components::Row;
        let mut result = None;
        Row::new().id_salt(("object_style_row", page)).show(ui, |ui, _| {
            if let Some(before) = check {
                // The hidden Field label names the mark without duplicating the visible page caption.
                Field::new(label.to_owned()).side(SideLabel::new().kind(LabelKind::Hidden))
                    .sizing(Sizing::Content).id_salt(("object_style_include", page)).show(ui, |ui, _| {
                        let mut next = before;
                        let out = Checkbox::new(Bind::given(before)).show(ui);
                        for event in out.events { event.apply(&mut next); }
                        if next != before { result = Some(next); }
                    });
            } else { ui.add_space(24.0); }
            Radio::new(page.to_owned()).label(label.to_owned()).id_salt(("object_style_page", page)).show(ui);
        });
        return result;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = native;
    let mut out = None;
    ui.horizontal(|ui| {
        match check {
            Some(mut on) => {
                if ui.checkbox(&mut on, "").changed() {
                    out = Some(on);
                }
            }
            None => ui.add_space(24.0),
        }
        let cur = d.fields.get("page").and_then(Value::as_str) == Some(page);
        if ui.selectable_label(cur, crate::rtl::widget(ui, label)).clicked() {
            d.fields.insert("page".into(), json!(page));
        }
    });
    out
}

/// The document's swatches, as a list to pick from (InDesign's Fill and Stroke pages).
fn swatch_list(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, key: &str) {
    let Some(doc) = app.session.active().map(|st| &st.doc) else { return };
    let cur = get(d, &[key, "swatch"]).as_str().unwrap_or("").to_string();
    let t = crate::theme::Tokens::get(ui.ctx());
    egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.divider)).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt(("os_swatches", key.to_string())).max_height(220.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            for w in doc.swatches.iter().filter(|w| !w.hidden || w.name == cur) {
                let picked = ui
                    .horizontal(|ui| {
                        let (c, g) = crate::widgets::swatch_colors(doc, &w.name, 1.0);
                        let r = crate::widgets::swatch_chip(ui, 14.0, c, g);
                        ui.selectable_label(w.name == cur, &w.name).clicked() || r.clicked()
                    })
                    .inner;
                if picked {
                    set(d, &[key, "swatch"], json!(w.name));
                }
            }
        });
    });
}

/// A number field of the dialog at `path` (a percentage of 0–1 with `percent`).
fn number(ui: &mut egui::Ui, d: &mut Dialog, path: &[&str], suffix: &str, range: (f64, f64), percent: bool) {
    let v = get(d, path).as_f64().unwrap_or(0.0);
    let shown = if percent { (v * 100.0).round() } else { v };
    let id = format!("os_{}", path.join("_"));
    let decimals = if percent { 0 } else { 2 };
    if let Some(n) = crate::widgets::NumField::number(&id, Some(shown), suffix, decimals).width(90.0).spinner().range(range.0, range.1).show(ui) {
        set(d, path, json!(if percent { n / 100.0 } else { n }));
    }
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

fn legacy_paragraph_menu(ui: &mut egui::Ui, lang: &str, styles: &[designcraft_doc::ParagraphStyle], cur: &str, shown: &str, picked: &mut Option<String>) {
    craft_ui_legacy::ComboBox::from_id_salt("os_paragraph_style").selected_text(shown).width(240.0).show_ui(ui, |ui| {
        for s in styles {
            if ui.selectable_label(s.name == cur, crate::i18n::style_name(lang, &s.name)).clicked() {
                *picked = Some(s.name.clone());
            }
        }
    });
}

/// The settings of the attribute shown.
fn page_ui(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, page: &str) {
    let tr = |s: &str| crate::i18n::tr(&app.ui.language, s).to_string();
    let title = ATTRIBUTES.iter().find(|a| a.0 == page).map(|a| a.1);
    if let Some(title) = title {
        crate::rtl::label(ui, egui::RichText::new(tr(title)).font(semibold(14.0)));
        ui.add_space(6.0);
    }
    match page {
        "general" => {
            ui.horizontal(|ui| {
                crate::rtl::label(ui, tr("Based On:"));
                // Based On isn't editable yet: the style's parent, as it is.
                ui.add_enabled_ui(false, |ui| {
                    let based = d.s("basedOn");
                    craft_ui_legacy::ComboBox::from_id_salt("os_based_on")
                        .selected_text(crate::i18n::style_name(&app.ui.language, &based))
                        .width(220.0)
                        .show_ui(ui, |_| {});
                });
            });
            ui.add_space(10.0);
            crate::rtl::label(ui, egui::RichText::new(tr("Style Settings:")).font(semibold(12.0)));
            let t = crate::theme::Tokens::get(ui.ctx());
            egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.divider)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(160.0);
                let lines = settings(app, d);
                if lines.is_empty() {
                    crate::rtl::label(ui, egui::RichText::new(tr("(no attributes)")).color(t.text_dim));
                }
                for l in lines {
                    crate::rtl::label(ui, l);
                }
            });
        }
        "fill" | "stroke" => {
            let key = page;
            ui.add_enabled_ui(included(d, key), |ui| {
                swatch_list(app, ui, d, key);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if key == "stroke" {
                        crate::rtl::label(ui, tr("Weight:"));
                        number(ui, d, &["stroke", "weight"], " pt", (0.0, 1000.0), false);
                        ui.add_space(12.0);
                    }
                    crate::rtl::label(ui, tr("Tint:"));
                    number(ui, d, &[key, "tint"], "%", (0.0, 100.0), true);
                });
            });
        }
        "corners" => {
            ui.add_enabled_ui(included(d, "corners"), |ui| {
                crate::rtl::label(ui, egui::RichText::new(tr("Corner Options")).font(semibold(12.0)));
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, tr("Size:"));
                    let size = get(d, &["corners", "corners"])[0]["size"].as_f64().unwrap_or(0.0);
                    if let Some(v) =
                        crate::widgets::NumField::number("os_corner_size", Some(size), " pt", 2).width(90.0).spinner().range(0.0, 10_000.0).show(ui)
                    {
                        let mut c = get(d, &["corners"]).clone();
                        if let Some(cs) = c["corners"].as_array_mut() {
                            cs.iter_mut().for_each(|k| k["size"] = json!(v));
                        }
                        d.fields.insert("corners".into(), c);
                    }
                    ui.add_space(12.0);
                    crate::rtl::label(ui, tr("Shape:"));
                    let cur: designcraft_geom::corners::CornerShape =
                        serde_json::from_value(get(d, &["corners", "corners"])[0]["shape"].clone()).unwrap_or_default();
                    let mut picked = None;
                    craft_ui_legacy::ComboBox::from_id_salt("os_corner_shape").selected_text(tr(cur.label())).width(140.0).show_ui(ui, |ui| {
                        for s in designcraft_geom::corners::CornerShape::ALL {
                            if ui.selectable_label(s == cur, tr(s.label())).clicked() {
                                picked = Some(s);
                            }
                        }
                    });
                    if let Some(s) = picked {
                        let mut c = get(d, &["corners"]).clone();
                        if let Some(cs) = c["corners"].as_array_mut() {
                            cs.iter_mut().for_each(|k| k["shape"] = json!(s));
                        }
                        d.fields.insert("corners".into(), c);
                    }
                });
            });
        }
        "paragraph" => {
            let Some(doc) = app.session.active().map(|st| &st.doc) else { return };
            ui.add_enabled_ui(included(d, "paragraph"), |ui| {
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, tr("Paragraph Style:"));
                    let cur = d.s("paragraphStyle");
                    let shown = if cur.is_empty() { String::new() } else { crate::i18n::style_name(&app.ui.language, &cur).to_string() };
                    let mut picked = None;
                    #[cfg(not(target_arch = "wasm32"))]
                    if framework(app) {
                        use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _};
                        let entries: Vec<_> = doc.styles.paragraph.iter().map(|s| {
                            Entry::new(s.name.clone(), crate::i18n::style_name(&app.ui.language, &s.name).to_owned())
                        }).collect();
                        let mut next = cur.clone();
                        let out = ui.allocate_ui(egui::vec2(240.0, 0.0), |ui| {
                            craft_ui_widgets::pickers::Select::new(tr("Paragraph Style:"), Bind::given(cur.clone()), &entries)
                                .id_salt("os_paragraph_style").sizing(craft_ui_widgets::contract::Sizing::FillWidth).show(ui)
                        }).inner;
                        crate::control::object_style_witness::chooser(ui, &out.response);
                        for event in out.events { event.apply(&mut next); }
                        if next != cur { picked = Some(next); }
                    } else {
                        legacy_paragraph_menu(ui, &app.ui.language, &doc.styles.paragraph, &cur, &shown, &mut picked);
                    }
                    #[cfg(target_arch = "wasm32")]
                    legacy_paragraph_menu(ui, &app.ui.language, &doc.styles.paragraph, &cur, &shown, &mut picked);
                    if let Some(name) = picked { d.fields.insert("paragraphStyle".into(), json!(name)); }
                });
                let mut next = d.b("applyNext");
                draft_check(ui, framework(app), &tr("Apply Next Style"), "applyNext", &mut next);
                if next != d.b("applyNext") { d.fields.insert("applyNext".into(), json!(next)); }
            });
        }
        "textFrame" => {
            ui.add_enabled_ui(included(d, "textFrame"), |ui| crate::dialogs::general(&app.ui.language, ui, d));
        }
        p => crate::effects_dialog::page_ui(app, ui, d, p),
    }
}

fn navigation_contents(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, native: bool) {
    let tr = |s: &str| crate::i18n::tr(&app.ui.language, s).to_string();
    crate::rtl::label(ui, egui::RichText::new(tr("Basic Attributes")).font(semibold(12.0)));
    for (key, label) in ATTRIBUTES {
        let check = (key != "general").then(|| included(d, key));
        if let Some(on) = list_row(ui, d, key, &tr(label), check, native) {
            set(d, &["inc", key], json!(on));
        }
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        crate::rtl::label(ui, tr("Effects for:"));
        // Stroke, Fill and Text levels aren't in the document model yet: the object only.
        ui.add_enabled_ui(false, |ui| {
            craft_ui_legacy::ComboBox::from_id_salt("os_fx_level").selected_text(tr("Object")).width(100.0).show_ui(ui, |_| {});
        });
    });
    for (key, label) in crate::effects_dialog::PAGES.iter().skip(1) {
        let fx = d.fields.get("fx").cloned().unwrap_or(Value::Null);
        if let Some(on) = list_row(ui, d, key, &tr(label), Some(crate::effects_dialog::is_on(&fx, key)), native)
            && let Some(f) = d.fields.get_mut("fx")
        {
            if on {
                crate::effects_dialog::turn_on(f, key);
            } else if *key == "feather" {
                f["feather"] = json!(0.0);
            } else {
                f[*key]["on"] = json!(false);
            }
        }
    }
}

fn navigation(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let native = framework(app);
    #[cfg(not(target_arch = "wasm32"))]
    if native {
        use craft_ui_widgets::contract::{Bind, Props as _, ShowContainer as _};
        let before = d.s("page");
        let mut page = before.clone();
        let shown = craft_ui_widgets::choice::RadioGroup::new(
            crate::i18n::tr(&app.ui.language, "Basic Attributes").to_owned(), Bind::given(before.clone()),
        ).id_salt("object_style_pages").show(ui, |ui, _| {
            // RadioGroup owns navigation; Stack preserves the existing vertical page list.
            craft_ui_widgets::layout_components::Stack::new().show(ui, |ui, _| navigation_contents(app, ui, d, true));
        });
        for event in shown.output.events { event.apply(&mut page); }
        if page != before { d.fields.insert("page".into(), json!(page)); }
        return;
    }
    navigation_contents(app, ui, d, native);
}

/// The dialog's body.
pub fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    ui.horizontal(|ui| {
        crate::rtl::label(ui, tr("Style Name:"));
        let mut name = d.s("name");
        #[cfg(not(target_arch = "wasm32"))]
        if app.framework_installed {
            use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
            let out = ui.allocate_ui(egui::vec2(300.0, 0.0), |ui| {
                craft_ui_widgets::text::TextField::new(tr("Style Name:"), Bind::given(name.clone()))
                    .id_salt("object_style_name")
                    .sizing(craft_ui_widgets::contract::Sizing::FillWidth)
                    .limit(None::<usize>)
                    .show(ui)
            }).inner;
            for event in out.events {
                event.apply(&mut name);
            }
        } else {
            ui.add(egui::TextEdit::singleline(&mut name).desired_width(300.0));
        }
        #[cfg(target_arch = "wasm32")]
        ui.add(egui::TextEdit::singleline(&mut name).desired_width(300.0));
        if name != d.s("name") {
            d.fields.insert("name".into(), json!(name));
        }
    });
    // Why OK was refused (an empty or taken name), until the next try.
    let status = d.s("status");
    if !status.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(status).color(crate::theme::Tokens::get(ui.ctx()).text_strong));
    }
    ui.add_space(8.0);
    let page = d.fields.get("page").and_then(Value::as_str).unwrap_or("general").to_string();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(230.0);
            navigation(app, ui, d);
        });
        ui.add_space(16.0);
        ui.vertical(|ui| {
            ui.set_min_width(440.0);
            page_ui(app, ui, d, &page);
        });
    });
    ui.add_space(10.0);
    if d.b("new") {
        let mut apply = d.b("apply");
        draft_check(ui, framework(app), &tr("Apply Style to Selection"), "apply", &mut apply);
        if apply != d.b("apply") { d.fields.insert("apply".into(), json!(apply)); }
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

    /// The texts the dialog paints, once its size has settled (it grows over a few frames).
    fn frame(app: &mut DesignApp, ctx: &egui::Context) -> Vec<String> {
        let mut texts = Vec::new();
        for _ in 0..8 {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()));
            fn walk(s: egui::epaint::Shape, texts: &mut Vec<String>) {
                match s {
                    egui::epaint::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                    egui::epaint::Shape::Vec(v) => v.into_iter().for_each(|s| walk(s, texts)),
                    _ => {}
                }
            }
            texts.clear();
            let shapes = std::mem::take(&mut out.shapes);
            out.drop_without_applying_deltas();
            for c in shapes { walk(c.shape, &mut texts); }
        }
        texts
    }

    /// The course's chapter 15: New Object Style from a frame with rounded corners, a paper
    /// stroke and a drop shadow, named and applied to the selection; then Object Style Options
    /// with a thicker stroke: the frames using the style follow.
    #[test]
    fn a_new_object_style_from_the_selection_and_its_options() {
        let (mut app, a) = app_with_frame();
        app.run("object.cornerOptions", json!({"shape": "rounded", "size": 12, "ids": [a]})).unwrap();
        app.run("object.stroke", json!({"swatch": "[Paper]", "weight": 2.5, "ids": [a]})).unwrap();
        app.run("object.dropShadow", json!({"on": true, "ids": [a]})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        open_new(&mut app);
        let texts = frame(&mut app, &ctx);
        for t in [
            "New Object Style",
            "Style Name:",
            "Basic Attributes",
            "Fill",
            "Stroke & Corner Options",
            "Effects for:",
            "Drop Shadow",
            "Style Settings:",
            "Apply Style to Selection",
        ] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        assert!(texts.iter().any(|x| x.starts_with("Stroke: [Paper], 2.5 pt")), "{texts:?}");
        assert!(texts.iter().any(|x| x.starts_with("Effects: Drop Shadow")), "{texts:?}");
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Rounded Corner Image, White Stroke"));
            d.fields.insert("page".into(), json!("corners"));
        }
        let texts = frame(&mut app, &ctx);
        assert!(texts.iter().any(|x| x.trim() == "Rounded"), "{texts:?}");
        crate::dialogs::confirm(&mut app).unwrap();
        let name = "Rounded Corner Image, White Stroke";
        let doc = |app: &DesignApp| app.session.active().unwrap().doc.clone();
        let item = |app: &DesignApp, id: u64| doc(app).item(designcraft_doc::ItemId(id)).unwrap().clone();
        assert_eq!(item(&app, a).object_style, name, "applied to the selection");
        let os = doc(&app).styles.object_style(name).cloned().unwrap();
        assert!(os.corners.is_some() && os.effects.as_ref().unwrap().drop_shadow.on && os.stroke.as_ref().unwrap().weight == 2.5);

        let b = app.run("frame.create", json!({"rect": [200, 72, 300, 172], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        app.run("style.object.apply", json!({"name": name, "ids": [b]})).unwrap();
        open_edit(&mut app, name).unwrap();
        if let Some(d) = app.ui.dialog.as_mut() {
            assert_eq!(title(d), "Object Style Options");
            set(d, &["stroke", "weight"], json!(5.0));
            set(d, &["inc", "fill"], json!(false));
        }
        frame(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();
        assert_eq!((item(&app, a).stroke.weight, item(&app, b).stroke.weight), (5.0, 5.0));
        assert!(doc(&app).styles.object_style(name).unwrap().fill.is_none(), "Fill unchecked: no longer in the style");
        assert!(doc(&app).styles.object_style(name).unwrap().effects.as_ref().unwrap().drop_shadow.on, "effects kept");
    }

    /// The course's chapter 39: a new object style from a text frame nests the "Drop Cap First
    /// Paragraph" style (Paragraph Styles page), so the frame's text takes it; Object Style
    /// Options › Text Frame General Options then gives the style's frames insets and two columns.
    #[test]
    fn an_object_style_nests_a_paragraph_style_and_sets_text_frame_options() {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Body Text"})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Drop Cap First Paragraph", "nextStyle": "Body Text", "para": {"dropCapLines": 3}}))
            .unwrap();
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 336], "content": "text", "caret": false})).unwrap();
        let id = r["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        open_new(&mut app);
        let d = app.ui.dialog.as_ref().unwrap();
        assert!(included(d, "paragraph") && included(d, "textFrame"), "a text frame's categories are in a new style");
        assert_eq!(d.s("paragraphStyle"), "", "the paragraph style is left to choose");
        let texts = frame(&mut app, &ctx);
        for t in ["Paragraph Styles", "Text Frame General Options"] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Text Box Drop Cap"));
            d.fields.insert("page".into(), json!("paragraph"));
            d.fields.insert("paragraphStyle".into(), json!("Drop Cap First Paragraph"));
        }
        let texts = frame(&mut app, &ctx);
        for t in ["Paragraph Style:", "Apply Next Style", "Drop Cap First Paragraph"] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let doc = |app: &DesignApp| app.session.active().unwrap().doc.clone();
        let story = |app: &DesignApp| {
            let d = doc(app);
            let sid = d.item(designcraft_doc::ItemId(id)).unwrap().text_frame().unwrap().story;
            d.story(sid).unwrap().clone()
        };
        assert_eq!(doc(&app).item(designcraft_doc::ItemId(id)).unwrap().object_style, "Text Box Drop Cap");
        assert_eq!(story(&app).paras[0].style, "Drop Cap First Paragraph", "the frame's text takes the nested style");
        app.run("type.fillWithPlaceholder", json!({"frame": id})).unwrap();
        assert!(story(&app).paras.iter().all(|p| p.style == "Drop Cap First Paragraph"));

        open_edit(&mut app, "Text Box Drop Cap").unwrap();
        let texts = frame(&mut app, &ctx);
        assert!(texts.iter().any(|x| x.starts_with("Paragraph Styles: Drop Cap First Paragraph")), "Style Settings: {texts:?}");
        if let Some(d) = app.ui.dialog.as_mut() {
            assert_eq!(d.s("paragraphStyle"), "Drop Cap First Paragraph");
            d.fields.insert("page".into(), json!("textFrame"));
            for k in ["insetTop", "insetLeft", "insetBottom", "insetRight"] {
                d.fields.insert(k.into(), json!("4 pt"));
            }
            d.fields.insert("columns".into(), json!("2"));
        }
        let texts = frame(&mut app, &ctx);
        for t in ["Inset Spacing", "Vertical Justification", "Ignore Text Wrap"] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let o = doc(&app).item(designcraft_doc::ItemId(id)).unwrap().text_frame().unwrap().options.clone();
        assert_eq!((o.columns, o.inset), (2, [4.0; 4]));
        let os = doc(&app).styles.object_style("Text Box Drop Cap").cloned().unwrap();
        assert_eq!((os.paragraph_style.as_deref(), os.text_frame.map(|t| t.columns)), (Some("Drop Cap First Paragraph"), Some(2)));

        // A word for the gutter, or a fraction of a column, keeps the dialog open with the reason.
        for (field, value, reason) in [("gutter", "wide", "gutter"), ("columns", "2.5", "columns")] {
            open_edit(&mut app, "Text Box Drop Cap").unwrap();
            if let Some(d) = app.ui.dialog.as_mut() {
                d.fields.insert(field.into(), json!(value));
            }
            let e = crate::dialogs::confirm(&mut app).unwrap_err();
            assert!(e.contains(reason), "{e}");
            assert!(app.ui.dialog.is_some());
            app.ui.dialog = None;
        }
        let os = doc(&app).styles.object_style("Text Box Drop Cap").cloned().unwrap();
        assert_eq!(os.text_frame.map(|t| t.columns), Some(2), "2.5 columns changed nothing");
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

    /// A refused OK (an empty name, a name another style has, in Object Style Options and in New
    /// Object Style) keeps the dialog and its edits, with the reason shown.
    #[test]
    fn a_refused_ok_keeps_the_dialog_open_with_the_reason() {
        let (mut app, _) = app_with_frame();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        app.run("style.object.create", json!({"name": "Taken", "fromSelection": false})).unwrap();
        app.run("style.object.create", json!({"name": "Mine", "fromSelection": false})).unwrap();
        open_new(&mut app);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("  "));
            set(d, &["stroke", "weight"], json!(7.0));
        }
        assert!(crate::dialogs::confirm(&mut app).is_err());
        let d = app.ui.dialog.as_ref().expect("still open");
        assert_eq!(d.s("status"), "Enter a style name.");
        assert_eq!(get(d, &["stroke", "weight"]), &json!(7.0), "the edits are kept");
        assert!(frame(&mut app, &ctx).iter().any(|t| t.trim() == "Enter a style name."));
        open_edit(&mut app, "Mine").unwrap();
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Taken"));
        }
        let e = crate::dialogs::confirm(&mut app).unwrap_err();
        assert!(e.contains("already named"), "{e}");
        assert!(app.ui.dialog.as_ref().is_some_and(|d| d.s("status") == e));
        // New Object Style with a name another style has makes no style under another name.
        app.ui.dialog = None;
        let count = |app: &DesignApp| app.session.active().unwrap().doc.styles.object.len();
        let before = count(&app);
        open_new(&mut app);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Taken"));
        }
        let e = crate::dialogs::confirm(&mut app).unwrap_err();
        assert!(e.contains("already named"), "{e}");
        assert!(app.ui.dialog.as_ref().is_some_and(|d| d.s("status") == e));
        assert_eq!(count(&app), before, "no style was made");
    }

    fn app_with_text_frame(units: &str) -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("document.preferences", json!({"horizontalUnits": units, "verticalUnits": units})).unwrap();
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 336], "content": "text", "caret": false})).unwrap();
        app.run("selection.set", json!({"ids": [r["id"]]})).unwrap();
        app
    }

    /// In a document measured in millimeters, a bare number in a Text Frame General Options field
    /// is millimeters, in New Object Style and in Object Style Options.
    #[test]
    fn bare_numbers_are_in_the_documents_ruler_unit() {
        let mut app = app_with_text_frame("millimeters");
        let mm = 72.0 / 25.4;
        let frame = |app: &DesignApp| app.session.active().unwrap().doc.styles.object_style("Metric").and_then(|s| s.text_frame.clone()).unwrap();
        open_new(&mut app);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Metric"));
            for k in ["insetTop", "insetLeft", "insetBottom", "insetRight"] {
                d.fields.insert(k.into(), json!("6"));
            }
        }
        crate::dialogs::confirm(&mut app).unwrap();
        assert!(frame(&app).inset.iter().all(|v| (v - 6.0 * mm).abs() < 1e-6), "{:?}", frame(&app).inset);
        open_edit(&mut app, "Metric").unwrap();
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("columns".into(), json!("2"));
            d.fields.insert("gutter".into(), json!("5"));
        }
        crate::dialogs::confirm(&mut app).unwrap();
        assert!((frame(&app).gutter - 5.0 * mm).abs() < 1e-6, "{}", frame(&app).gutter);
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
        #[cfg(not(target_arch = "wasm32"))]
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
    #[cfg(not(target_arch = "wasm32"))]
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
                app.framework_opacity = true;
                app.install_framework(&ctx);
                app.raw_input_hook(&ctx, &mut egui::RawInput::default());
                assert!(app.framework_installed);
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
                                            show(&mut self.app, ui, &mut d);
                                            assert_eq!(d.fields, fields, "measurement must not consume draft events inside the guarded callback");
                                            assert_eq!(effects(&self.app), before, "measurement callback must not mutate any document/history/journal");
                                            self.guarded += 1;
                                        });
                                    } else {
                                        if self.pass == Pass::Sizing { assert!(ui.is_sizing_pass()); }
                                        if self.pass == Pass::Disabled { assert!(!ui.is_enabled()); }
                                        show(&mut self.app, ui, &mut d);
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
                let caption = if name == "OK" { "  OK  " } else { name };
                let (_, rect) = self.ax_control(Role::Button, caption);
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
            let original_name = w.app.ui.dialog.as_ref().unwrap().s("name");
            w.name("Canceled field draft");
            w.key(Key::Escape);
            assert!(w.app.ui.dialog.is_some(), "the active SDK text draft owns the first Escape");
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("name"), original_name);
            w.quiet(&before);
            w.key(Key::Escape);
            assert!(w.app.ui.dialog.is_none(), "after its draft is canceled, the host owns Escape");
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
