//! Swatch Options, Color Group Options and Layer Options.

use designcraft_color::{Color, ColorType, SwatchValue};
use serde_json::{Map, Value, json};

use super::Dialog;
use crate::DesignApp;

/// Swatch Options for `name`: its name, and what its kind lets the user change.
pub fn open_swatch_options(app: &mut DesignApp, name: &str) -> Result<(), String> {
    let doc = &app.session.active().ok_or("no document open")?.doc;
    let w = doc.swatch(name).ok_or_else(|| format!("no swatch `{name}`"))?;
    let mut f = Map::new();
    f.insert("name".into(), json!(name));
    f.insert("swatchName".into(), json!(name));
    f.insert("locked".into(), json!(w.locked));
    let kind = match &w.value {
        SwatchValue::Color { color, color_type } if !w.locked => {
            f.insert("nameWithValue".into(), json!(!w.named));
            f.insert("colorType".into(), json!(if *color_type == ColorType::Spot { "spot" } else { "process" }));
            set_color(&mut f, *color, None);
            "color"
        }
        SwatchValue::Paper { color } => {
            set_color(&mut f, *color, None);
            "paper"
        }
        SwatchValue::Tint { base, tint } => {
            f.insert("base".into(), json!(base));
            f.insert("tint".into(), json!((f64::from(*tint) * 100.0).round()));
            "tint"
        }
        SwatchValue::Gradient { .. } => "gradient",
        _ => "fixed",
    };
    f.insert("kind".into(), json!(kind));
    if let Some(c) = dialog_color(&f) {
        f.insert("origColor".into(), color_json(c));
    }
    app.ui.dialog = Some(Dialog::new("swatchOptions", Value::Object(f)));
    Ok(())
}

/// Write `c`'s channels in `mode` (default: the colour's own model).
pub(super) fn set_color(f: &mut Map<String, Value>, c: Color, mode: Option<&str>) {
    let mode = mode.unwrap_or(match c {
        Color::Cmyk { .. } => "cmyk",
        Color::Rgb { .. } => "rgb",
        Color::Gray { .. } => "gray",
    });
    f.insert("mode".into(), json!(mode));
    match mode {
        "rgb" => {
            for (k, v) in ["r", "g", "b"].iter().zip(c.to_rgb()) {
                f.insert((*k).into(), json!((f64::from(v) * 255.0).round()));
            }
        }
        "gray" => {
            let k = match c {
                Color::Gray { k } => k,
                other => other.to_cmyk()[3],
            };
            f.insert("gray".into(), json!((f64::from(k) * 1000.0).round() / 10.0));
        }
        _ => {
            for (k, v) in ["c", "m", "y", "k"].iter().zip(c.to_cmyk()) {
                f.insert((*k).into(), json!((f64::from(v) * 1000.0).round() / 10.0));
            }
        }
    }
}

/// The colour the dialog's channels describe.
pub(super) fn dialog_color(f: &Map<String, Value>) -> Option<Color> {
    let n = |k: &str, max: f64| f.get(k).and_then(Value::as_f64).map(|v| (v.clamp(0.0, max) / max) as f32);
    match f.get("mode").and_then(Value::as_str)? {
        "rgb" => Some(Color::rgb(n("r", 255.0)?, n("g", 255.0)?, n("b", 255.0)?)),
        "gray" => Some(Color::gray(n("gray", 100.0)?)),
        _ => Some(Color::cmyk(n("c", 100.0)?, n("m", 100.0)?, n("y", 100.0)?, n("k", 100.0)?)),
    }
}

/// A colour as `swatch.options` takes it (fractions, so 1% isn't read as 100%).
fn color_json(c: Color) -> Value {
    match c {
        Color::Cmyk { c, m, y, k } => json!({"model": "cmyk", "c": c, "m": m, "y": y, "k": k}),
        Color::Rgb { r, g, b } => json!([r, g, b]),
        Color::Gray { k } => json!({"gray": k}),
    }
}

pub fn confirm_swatch_options(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let kind = d.s("kind");
    let mut p = json!({"name": d.s("name")});
    if !d.b("locked") {
        p["to"] = json!(d.s("swatchName").trim());
    }
    if let Some(c) = dialog_color(&d.fields).map(color_json)
        && d.fields.get("origColor") != Some(&c)
    {
        p["color"] = c;
    }
    match kind.as_str() {
        "color" => {
            p["nameWithValue"] = json!(d.b("nameWithValue"));
            p["spot"] = json!(d.s("colorType") == "spot");
        }
        "tint" => p["tint"] = json!((d.n("tint").unwrap_or(100.0) / 100.0).clamp(0.0, 1.0)),
        "paper" => {}
        "gradient" => {}
        _ => return Ok(Value::Null),
    }
    app.run("swatch.options", p)
}

pub fn open_color_group_options(app: &mut DesignApp, name: &str) -> Result<(), String> {
    let doc = &app.session.active().ok_or("no document open")?.doc;
    if !doc.color_groups.iter().any(|g| g.name == name) {
        return Err(format!("no color group `{name}`"));
    }
    app.ui.dialog = Some(Dialog::new("colorGroupOptions", json!({"name": name, "groupName": name})));
    Ok(())
}

pub fn confirm_color_group_options(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let (name, to) = (d.s("name"), d.s("groupName").trim().to_string());
    if to == name {
        return Ok(Value::Null);
    }
    app.run("swatch.renameColorGroup", json!({"name": name, "to": to}))
}

/// Layer Options for layer `id` (default: the active layer).
pub fn open_layer_options(app: &mut DesignApp, id: Option<u64>) -> Result<(), String> {
    let st = app.session.active().ok_or("no document open")?;
    let id = id.map_or(st.active_layer, designcraft_doc::LayerId);
    let l = st.doc.layer(id).ok_or("no such layer")?;
    let f = json!({"id": l.id.0, "name": l.name, "rgb": l.color, "visible": l.visible, "locked": l.locked, "printable": l.printable,
        "showGuides": l.show_guides, "lockGuides": l.lock_guides, "suppressWrapWhenHidden": l.suppress_wrap_when_hidden});
    app.ui.dialog = Some(Dialog::new("layerSettings", f));
    Ok(())
}

pub fn confirm_layer_options(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let mut p = json!({"id": d.fields.get("id").cloned().unwrap_or(Value::Null), "name": d.s("name"), "color": d.fields.get("rgb").cloned().unwrap_or(Value::Null)});
    for k in ["visible", "locked", "printable", "showGuides", "lockGuides", "suppressWrapWhenHidden"] {
        p[k] = json!(d.b(k));
    }
    app.run("layer.set", p)
}


