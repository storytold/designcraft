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
/// dialog shows them (`tfBase` keeps the options the page doesn't show).
fn text_fields(app: &DesignApp, fields: &mut Value, paragraph: Option<&str>, next: bool, tf: &designcraft_doc::TextFrameOptions) {
    let units = app.session.active().map_or(designcraft_geom::Unit::Picas, |s| s.doc.settings.horizontal_units);
    fields["paragraphStyle"] = json!(paragraph.unwrap_or(""));
    fields["applyNext"] = json!(next);
    fields["tfBase"] = serde_json::to_value(tf).unwrap_or_default();
    fields["insetLinked"] = json!(tf.inset.iter().all(|v| (v - tf.inset[0]).abs() < 1e-9));
    if let (Value::Object(f), Value::Object(shown)) =
        (fields, crate::dialogs::text_frame_fields(tf, units, None, &crate::dialogs::document_grid(app)))
    {
        f.extend(shown);
    }
}

/// The dialog for a new style from the selected object (its fill, stroke, corners and effects,
/// as InDesign's New Object Style), named "Object Style n".
pub fn open_new(app: &mut DesignApp) {
    let Some(st) = app.session.active() else { return };
    let it = st.selection.items.first().and_then(|i| st.doc.item(*i)).cloned();
    let taken = |n: &str| st.doc.styles.object_style(n).is_some();
    let name = (1..).map(|i| format!("Object Style {i}")).find(|n| !taken(n)).unwrap_or_default();
    let from = it.is_some();
    // A text frame's options come along; its paragraph style is left to choose, as in InDesign.
    let frame = it.as_ref().and_then(|i| i.text_frame()).map(|t| t.options.clone());
    let mut fields = json!({
        "new": true,
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
    app.ui.dialog = Some(Dialog::new(ID, fields));
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
    app.ui.dialog = Some(Dialog::new(ID, fields));
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
    let measure = |key: &str| d.m(key).filter(|v| v.is_finite() && (0.0..=15552.0).contains(v));
    o.columns = d
        .n("columns")
        .filter(|n| (1.0..=40.0).contains(n) && n.fract() == 0.0)
        .ok_or_else(|| bad("the number of columns is a whole number from 1 to 40"))? as u32;
    o.gutter = measure("gutter").ok_or_else(|| bad("the gutter is not a measure"))?;
    o.columns_kind = serde_json::from_value(json!(d.s("columnsKind"))).unwrap_or(o.columns_kind);
    if o.columns_kind == designcraft_doc::ColumnsKind::FixedWidth {
        o.column_width = measure("columnWidth").ok_or_else(|| bad("the column width is not a measure"))?;
    }
    o.balance_columns = d.b("balanceColumns");
    for (side, key) in o.inset.iter_mut().zip(["insetTop", "insetLeft", "insetBottom", "insetRight"]) {
        *side = measure(key).ok_or_else(|| bad("an inset is not a measure"))?;
    }
    o.vertical_justification = serde_json::from_value(json!(d.s("verticalJustification"))).unwrap_or(o.vertical_justification);
    o.ignore_wrap = d.b("ignoreWrap");
    Ok(Some(o))
}

/// OK: the new style (applied to the selection when asked), or the edited one.
pub fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let name = d.s("name").trim().to_string();
    if name.is_empty() {
        return Err(crate::i18n::tr(&app.ui.language, "Enter a style name.").to_string());
    }
    let mut p = attrs(d);
    p.insert("textFrame".into(), frame_options(d)?.map_or(Value::Null, |o| serde_json::to_value(o).unwrap_or_default()));
    if d.b("new") {
        p.insert("name".into(), json!(name));
        p.insert("fromSelection".into(), json!(false));
        let r = app.run("style.object.create", Value::Object(p)).map_err(|e| e.to_string())?;
        let made = r["name"].as_str().unwrap_or(&name).to_string();
        // The style exists now, so OK again mustn't make a second one: a refused apply (locked
        // objects) goes to the status line and the dialog closes.
        if d.b("apply")
            && app.session.active().is_some_and(|st| !st.selection.items.is_empty())
            && let Err(e) = app.run("style.object.apply", json!({"name": made}))
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

/// A row of the left-hand lists: a checkbox (when `check` is given) and the label, which shows
/// `page`.
fn list_row(ui: &mut egui::Ui, d: &mut Dialog, page: &str, label: &str, check: Option<bool>) -> Option<bool> {
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
                    craft_ui::ComboBox::from_id_salt("os_based_on")
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
                    craft_ui::ComboBox::from_id_salt("os_corner_shape").selected_text(tr(cur.label())).width(140.0).show_ui(ui, |ui| {
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
                    craft_ui::ComboBox::from_id_salt("os_paragraph_style").selected_text(shown).width(240.0).show_ui(ui, |ui| {
                        for s in &doc.styles.paragraph {
                            if ui.selectable_label(s.name == cur, crate::i18n::style_name(&app.ui.language, &s.name)).clicked() {
                                picked = Some(s.name.clone());
                            }
                        }
                    });
                    if let Some(name) = picked {
                        d.fields.insert("paragraphStyle".into(), json!(name));
                    }
                });
                let mut next = d.b("applyNext");
                if ui.checkbox(&mut next, crate::rtl::widget(ui, tr("Apply Next Style"))).changed() {
                    d.fields.insert("applyNext".into(), json!(next));
                }
            });
        }
        "textFrame" => {
            ui.add_enabled_ui(included(d, "textFrame"), |ui| crate::dialogs::general(&app.ui.language, ui, d));
        }
        p => crate::effects_dialog::page_ui(app, ui, d, p),
    }
}

/// The dialog's body.
pub fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    ui.horizontal(|ui| {
        crate::rtl::label(ui, tr("Style Name:"));
        let mut name = d.s("name");
        if ui.add(egui::TextEdit::singleline(&mut name).desired_width(300.0)).changed() {
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
            crate::rtl::label(ui, egui::RichText::new(tr("Basic Attributes")).font(semibold(12.0)));
            for (key, label) in ATTRIBUTES {
                let check = (key != "general").then(|| included(d, key));
                if let Some(on) = list_row(ui, d, key, &tr(label), check) {
                    set(d, &["inc", key], json!(on));
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                crate::rtl::label(ui, tr("Effects for:"));
                // Stroke, Fill and Text levels aren't in the document model yet: the object only.
                ui.add_enabled_ui(false, |ui| {
                    craft_ui::ComboBox::from_id_salt("os_fx_level").selected_text(tr("Object")).width(100.0).show_ui(ui, |_| {});
                });
            });
            for (key, label) in crate::effects_dialog::PAGES.iter().skip(1) {
                let fx = d.fields.get("fx").cloned().unwrap_or(Value::Null);
                if let Some(on) = list_row(ui, d, key, &tr(label), Some(crate::effects_dialog::is_on(&fx, key)))
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
        if ui.checkbox(&mut apply, crate::rtl::widget(ui, tr("Apply Style to Selection"))).changed() {
            d.fields.insert("apply".into(), json!(apply));
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
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
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
            out.textures_delta.clear();
            fn walk(s: egui::epaint::Shape, texts: &mut Vec<String>) {
                match s {
                    egui::epaint::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                    egui::epaint::Shape::Vec(v) => v.into_iter().for_each(|s| walk(s, texts)),
                    _ => {}
                }
            }
            texts.clear();
            for c in out.shapes {
                walk(c.shape, &mut texts);
            }
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
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
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

    /// A refused OK (an empty name, a name another style has) keeps the dialog and its edits,
    /// with the reason shown.
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
    }
}
