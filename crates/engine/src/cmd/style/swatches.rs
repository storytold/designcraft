use super::color::check_text_nesting;
use super::{Result, Session, Styles, Swatch, SwatchValue, Value, bad, json, str_param};

pub(super) fn create_swatch(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(g) = p.get("gradient") {
        return create_gradient_swatch(s, p, g);
    }
    let c = p.get("color").ok_or_else(|| bad("swatch.create", "missing color (or gradient)"))?;
    let color = parse_color(c).ok_or_else(|| bad("swatch.create", "bad color"))?;
    let spot = p.get("spot").and_then(Value::as_bool).unwrap_or(false);
    let name = str_param(p, "name").map(str::to_string).unwrap_or_else(|| value_name(color));
    s.edit(|d, _| {
        let name = Styles::unique_name(|n| d.swatch(n).is_some(), &name);
        d.swatches.push(Swatch {
            name: name.clone(),
            value: SwatchValue::Color {
                color,
                color_type: if spot { designcraft_color::ColorType::Spot } else { designcraft_color::ColorType::Process },
            },
            locked: false,
            named: true,
            hidden: false,
        });
        Ok(json!({"name": name}))
    })
}

/// `swatch.create {gradient}`: a named gradient swatch. Stops take a colour swatch by name (as a
/// swatch dragged onto the Gradient panel's stop) or a colour.
fn create_gradient_swatch(s: &mut Session, p: &Value, g: &Value) -> Result<Value> {
    use designcraft_color::{Gradient, GradientKind, GradientStop};
    const ID: &str = "swatch.create";
    let kind = match g.get("kind").and_then(Value::as_str) {
        None | Some("linear") => GradientKind::Linear,
        Some("radial") => GradientKind::Radial,
        Some(k) => return Err(bad(ID, format!("unknown gradient kind {k}"))),
    };
    let doc = &s.doc()?.doc;
    let mut stops = Vec::new();
    for st in g.get("stops").and_then(Value::as_array).ok_or_else(|| bad(ID, "a gradient needs stops"))? {
        // The stop keeps the swatch, so print export writes it in that swatch's inks.
        let (color, swatch) = match (st.get("swatch").and_then(Value::as_str), st.get("color")) {
            (Some(n), _) => match doc.swatch(n).map(|w| &w.value) {
                Some(SwatchValue::Gradient { .. }) => return Err(bad(ID, format!("`{n}` is a gradient; a stop takes a colour"))),
                Some(_) => (doc.resolve_color(n, 1.0).ok_or_else(|| bad(ID, format!("`{n}` has no color")))?, Some(n.to_string())),
                None => return Err(bad(ID, format!("no swatch named `{n}`"))),
            },
            (None, Some(c)) => (parse_color(c).ok_or_else(|| bad(ID, "a stop has a bad color"))?, None),
            (None, None) => return Err(bad(ID, "a stop needs a swatch or a color")),
        };
        let pct = |k: &str, d: f64| st.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(d).clamp(0.0, 100.0) as f32 / 100.0;
        stops.push(GradientStop {
            offset: pct("location", 0.0),
            color,
            opacity: pct("opacity", 100.0),
            midpoint: pct("midpoint", 50.0).clamp(0.13, 0.87),
            swatch,
        });
    }
    if stops.len() < 2 {
        return Err(bad(ID, "a gradient needs at least two stops"));
    }
    let mut gradient = Gradient { kind, stops };
    gradient.sort();
    let name = str_param(p, "name").unwrap_or("New Gradient Swatch").to_string();
    s.edit(|d, _| {
        let name = Styles::unique_name(|n| d.swatch(n).is_some(), &name);
        d.swatches.push(Swatch { name: name.clone(), value: SwatchValue::Gradient { gradient }, locked: false, named: true, hidden: false });
        Ok(json!({"name": name}))
    })
}

/// `swatch.new`: InDesign's New Swatch. An unnamed colour or gradient (the selection's, or
/// `swatch`) is added to the swatches under a name, and stays applied; a named swatch is
/// duplicated as "<name> copy".
pub(super) fn new_swatch(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "swatch.new";
    let st = s.doc()?;
    let name = match str_param(p, "swatch") {
        Some(n) => n.to_string(),
        None => {
            let Some(id) = st.selection.items.first().copied() else {
                // Nothing selected: a new colour swatch.
                return create_swatch(s, &json!({"color": {"c": 0, "m": 50, "y": 100, "k": 0}}));
            };
            let it = st.doc.item(id).ok_or_else(|| bad(ID, "no such item"))?;
            if str_param(p, "target") == Some("stroke") { it.stroke.swatch.clone() } else { it.fill.swatch.clone() }
        }
    };
    let w = st.doc.swatch(&name).cloned().ok_or_else(|| bad(ID, format!("no swatch named `{name}`")))?;
    if matches!(w.value, SwatchValue::None | SwatchValue::Registration) {
        return Err(bad(ID, format!("`{name}` can't be made a new swatch")));
    }
    if w.hidden {
        // An unnamed gradient takes InDesign's "New Gradient Swatch"; an unnamed colour its values.
        let new = match w.value {
            SwatchValue::Gradient { .. } => Styles::unique_name(|n| st.doc.swatch(n).is_some(), "New Gradient Swatch"),
            _ => name.clone(),
        };
        return add_to_swatches(s, &json!({"swatch": name, "name": new}));
    }
    s.edit(|d, _| {
        let copy = Styles::unique_name(|n| d.swatch(n).is_some(), &format!("{name} copy"));
        d.swatches.push(Swatch { name: copy.clone(), locked: false, named: true, hidden: false, ..w.clone() });
        Ok(json!({"name": copy}))
    })
}

pub fn parse_color(v: &Value) -> Option<designcraft_color::Color> {
    use designcraft_color::Color;
    match v {
        Value::String(s) => Color::from_hex(s),
        // `{model, …}` is the colour's own form (as `color.convert` takes it): fractions, never guessed.
        Value::Object(o) if o.contains_key("model") => model_color(v),
        // One scale per colour, not per component: [1, 128, 255] is 0..255. Only an RGB array whose
        // components are all within 0..1 is read as fractions (the older form).
        Value::Array(a) if a.len() >= 3 => {
            let v = [a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?];
            let scale = if v.iter().any(|&x| x > 1.0) { 255.0 } else { 1.0 };
            Some(Color::rgb((v[0] / scale) as f32, (v[1] / scale) as f32, (v[2] / scale) as f32))
        }
        // Bare CMYK is always 0..100, as documented: C 1 % is a real tint, so values of 1 or less can't
        // mean fractions (#910).
        Value::Object(o) => {
            if o.contains_key("gray") {
                let gray = o.get("gray").and_then(Value::as_f64)?;
                return Some(Color::gray(if gray > 1.0 { gray / 100.0 } else { gray } as f32));
            }
            let g = |k: &str| o.get(k).and_then(Value::as_f64).map(|x| (x / 100.0) as f32);
            Some(Color::cmyk(g("c")?, g("m")?, g("y")?, g("k")?))
        }
        _ => None,
    }
}

/// `{model: rgb|cmyk|gray, …}` with every component in 0..=1; anything else is not a colour.
fn model_color(v: &Value) -> Option<designcraft_color::Color> {
    use designcraft_color::Color;
    let color: Color = serde_json::from_value(v.clone()).ok()?;
    // A float's last bit (100.0 / 100.0 ≠ 1.0 after an f32 round trip) is not out of range.
    let unit = |x: f32| x.is_finite() && (-1e-4..=1.0 + 1e-4).contains(&x);
    let q = |x: f32| x.clamp(0.0, 1.0);
    match color {
        Color::Rgb { r, g, b } => [r, g, b].into_iter().all(unit).then(|| Color::rgb(q(r), q(g), q(b))),
        Color::Cmyk { c, m, y, k } => [c, m, y, k].into_iter().all(unit).then(|| Color::cmyk(q(c), q(m), q(y), q(k))),
        Color::Gray { k } => unit(k).then_some(Color::Gray { k: q(k) }),
    }
}

/// The swatch name of a colour value ("C=… M=… Y=… K=…" / "R=… G=… B=…").
pub(super) fn value_name(color: designcraft_color::Color) -> String {
    designcraft_color::swatch::value_name(color)
}

pub(super) fn load_swatches(s: &mut Session, p: &Value) -> Result<Value> {
    let bytes = match (str_param(p, "path"), str_param(p, "base64")) {
        (_, Some(b)) => crate::cmd::file::base64_decode(b),
        (Some(path), None) => crate::io::read_file(path).map_err(|e| bad("swatch.load", e))?,
        _ => return Err(bad("swatch.load", "give `path` or `base64`")),
    };
    let incoming = designcraft_color::ase::read(&bytes).map_err(|e| bad("swatch.load", e.to_string()))?;
    let replace = p.get("replace").and_then(Value::as_bool).unwrap_or(false);
    s.edit(|d, _| {
        // The position of the first swatch with each name, kept up to date as the file's colors are added.
        let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (i, x) in d.swatches.iter().enumerate() {
            at.entry(x.name.clone()).or_insert(i);
        }
        let mut names = FileNames::default();
        let (mut added, mut replaced) = (0, 0);
        for mut w in incoming {
            // An unnamed color is named by its value, and a name the file already gave gets a number.
            if w.name.is_empty()
                && let SwatchValue::Color { color, .. } = w.value
            {
                w.name = value_name(color);
                w.named = false;
            }
            w.name = names.unique(std::mem::take(&mut w.name));
            match at.get(&w.name).and_then(|&i| d.swatches.get_mut(i)) {
                Some(x) if x.locked => {}
                Some(x) => {
                    if replace {
                        x.value = w.value;
                        x.hidden = false;
                        replaced += 1;
                    }
                }
                None => {
                    at.insert(w.name.clone(), d.swatches.len());
                    d.swatches.push(w);
                    added += 1;
                }
            }
        }
        Ok(json!({"added": added, "replaced": replaced}))
    })
}

/// The swatch names one `.ase` file has given. A name already given gets the first free number
/// ("Red 2"), as [`Styles::unique_name`] numbers names.
#[derive(Default)]
struct FileNames {
    taken: std::collections::HashSet<String>,
    /// The last number tried for each repeated name.
    last: std::collections::HashMap<String, usize>,
}

impl FileNames {
    fn unique(&mut self, base: String) -> String {
        if self.taken.insert(base.clone()) {
            return base;
        }
        // Each pass tries the next number, and only finitely many names are taken.
        let n = self.last.entry(base.clone()).or_insert(1);
        loop {
            *n += 1;
            let name = format!("{base} {n}");
            if self.taken.insert(name.clone()) {
                return name;
            }
        }
    }
}

pub(super) fn save_swatches(s: &mut Session, p: &Value) -> Result<Value> {
    let d = &s.doc()?.doc;
    let names: Option<Vec<String>> =
        p.get("names").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());
    let chosen: Vec<Swatch> = d.swatches.iter().filter(|w| names.as_ref().is_none_or(|n| n.contains(&w.name))).cloned().collect();
    let bytes = designcraft_color::ase::write(&chosen);
    match str_param(p, "path") {
        Some(path) => {
            crate::io::write_file(path, &bytes).map_err(|e| bad("swatch.save", e))?;
            Ok(json!({"path": path, "bytes": bytes.len()}))
        }
        None => Ok(json!({"base64": crate::cmd::file::base64_encode(&bytes)})),
    }
}

pub(super) fn add_to_swatches(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let name = match str_param(p, "swatch") {
        Some(n) => n.to_string(),
        None => {
            let id = st.selection.items.first().copied().ok_or_else(|| bad("swatch.addToSwatches", "select an object or give `swatch`"))?;
            let it = st.doc.item(id).ok_or_else(|| bad("swatch.addToSwatches", "no such item"))?;
            if str_param(p, "target") == Some("stroke") { it.stroke.swatch.clone() } else { it.fill.swatch.clone() }
        }
    };
    let rename = str_param(p, "name").map(str::to_string);
    let result = s.edit(|d, _| {
        let w = d.swatches.iter_mut().find(|w| w.name == name).ok_or_else(|| bad("swatch.addToSwatches", format!("no swatch `{name}`")))?;
        if w.locked {
            return Err(bad("swatch.addToSwatches", "special swatches are already listed"));
        }
        w.hidden = false;
        let Some(new) = rename.filter(|n| *n != name) else { return Ok(json!({"name": name})) };
        if d.swatches.iter().any(|w| w.name == new) {
            return Err(bad("swatch.addToSwatches", format!("a swatch named `{new}` exists")));
        }
        check_text_nesting(d, "swatch.addToSwatches")?;
        let w = d.swatches.iter_mut().find(|w| w.name == name).ok_or_else(|| bad("swatch.addToSwatches", format!("no swatch `{name}`")))?;
        w.name = new.clone();
        w.named = true;
        d.rename_swatch_refs(&name, &new);
        Ok(json!({"name": new}))
    })?;
    if let Some(new) = result.get("name").and_then(Value::as_str) {
        crate::cmd::text::rename_typing_swatch(s, &name, new);
    }
    Ok(result)
}

/// Swatch Options: rename, Name with Color Value, colour type and value, a tint swatch's tint
/// and base — one undo step. Every use of the swatch follows a rename.
pub(super) fn swatch_options(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "swatch.options";
    let name = str_param(p, "name").ok_or_else(|| bad(ID, "missing name"))?.to_string();
    let to = str_param(p, "to").map(|n| n.trim().to_string());
    let with_value = p.get("nameWithValue").and_then(Value::as_bool);
    let color = match p.get("color") {
        Some(c) => Some(parse_color(c).ok_or_else(|| bad(ID, "bad color"))?),
        None => None,
    };
    let spot = p.get("spot").and_then(Value::as_bool);
    let tint = match p.get("tint") {
        Some(v) => Some(v.as_f64().filter(|t| t.is_finite() && (0.0..=1.0).contains(t)).ok_or_else(|| bad(ID, "tint is 0–1"))? as f32),
        None => None,
    };
    let base = str_param(p, "base").map(str::to_string);
    let result = s.edit(|d, _| {
        let w = d.swatch(&name).cloned().ok_or_else(|| bad(ID, format!("no swatch `{name}`")))?;
        let mut w2 = w.clone();
        if w.locked {
            if to.as_ref().is_some_and(|t| *t != name) || with_value == Some(true) {
                return Err(bad(ID, format!("`{name}` can't be renamed")));
            }
            match (&mut w2.value, color) {
                (_, None) => {}
                (SwatchValue::Paper { color: c }, Some(new)) => *c = new,
                _ => return Err(bad(ID, format!("`{name}` can't be edited"))),
            }
            if spot.is_some() || tint.is_some() || base.is_some() {
                return Err(bad(ID, format!("`{name}` can't be edited")));
            }
        } else {
            match &mut w2.value {
                SwatchValue::Color { color: c, color_type } => {
                    if let Some(new) = color {
                        *c = new;
                    }
                    if let Some(sp) = spot {
                        *color_type = if sp { designcraft_color::ColorType::Spot } else { designcraft_color::ColorType::Process };
                    }
                    if tint.is_some() || base.is_some() {
                        return Err(bad(ID, "only tint swatches have a tint"));
                    }
                }
                SwatchValue::Tint { base: b, tint: t } => {
                    if color.is_some() || spot.is_some() {
                        return Err(bad(ID, "a tint swatch takes its colour from its base"));
                    }
                    if let Some(v) = tint {
                        *t = v;
                    }
                    if let Some(nb) = &base {
                        if !d.swatch(nb).is_some_and(|x| matches!(x.value, SwatchValue::Color { .. })) || *nb == name {
                            return Err(bad(ID, format!("`{nb}` isn't a color swatch")));
                        }
                        *b = nb.clone();
                    }
                }
                _ => {
                    if color.is_some() || spot.is_some() || tint.is_some() || base.is_some() {
                        return Err(bad(ID, format!("`{name}` only takes a name here")));
                    }
                }
            }
            // Name with Color Value: the name is the colour's values and tracks later edits.
            let auto = with_value.unwrap_or(!w.named && to.as_ref().is_none_or(|t| *t == name));
            let derived = match (&w2.value, auto) {
                (SwatchValue::Color { color, .. }, true) => Some(value_name(*color)),
                _ => None,
            };
            match derived {
                Some(n) => {
                    w2.name = n;
                    w2.named = false;
                }
                None => {
                    if let Some(t) = &to {
                        if t.is_empty() {
                            return Err(bad(ID, "a swatch needs a name"));
                        }
                        w2.name = t.clone();
                    }
                    if with_value == Some(false) || w2.name != name {
                        w2.named = true;
                    }
                }
            }
            if w2.name != name && d.swatch(&w2.name).is_some() {
                return Err(bad(ID, format!("a swatch named `{}` exists", w2.name)));
            }
        }
        let new = w2.name.clone();
        if new != name {
            check_text_nesting(d, ID)?;
        }
        let slot = d.swatches.iter_mut().find(|x| x.name == name).ok_or_else(|| bad(ID, format!("no swatch `{name}`")))?;
        *slot = w2;
        d.rename_swatch_refs(&name, &new);
        d.refresh_gradient_stops();
        Ok(json!({"name": new}))
    })?;
    if let Some(new) = result.get("name").and_then(Value::as_str) {
        crate::cmd::text::rename_typing_swatch(s, &name, new);
    }
    Ok(result)
}

#[cfg(test)]
mod group_sample_tests {
    use serde_json::json;

    use crate::Session;

    /// Grouping the sample's table styles keeps its table cells styled (no missing styles).
    #[test]
    fn grouping_reaches_table_cells() {
        let mut s = Session::new();
        s.execute("file.newSample", &json!({})).unwrap();
        let before = s.execute("preflight.run", &json!({})).unwrap()["errors"].as_u64().unwrap();
        s.execute("style.group", &json!({"kind": "paragraph", "names": ["Table Head", "Table Body"], "group": "Tables"})).unwrap();
        let after = s.execute("preflight.run", &json!({})).unwrap();
        assert_eq!(after["errors"].as_u64().unwrap(), before, "{after}");
    }
}

#[cfg(test)]
mod color_group_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn color_groups_hold_swatches_and_round_trip() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("swatch.create", &json!({"name": "Brand Red", "color": {"c": 0, "m": 100, "y": 100, "k": 0}})).unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string();
        let b = s.execute("swatch.create", &json!({"name": "Brand Blue", "color": {"c": 100, "m": 60, "y": 0, "k": 0}})).unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string();
        let g = s.execute("swatch.newColorGroup", &json!({"name": "Brand", "swatches": [a]})).unwrap();
        assert_eq!(g["name"], "Brand");
        assert!(s.execute("swatch.newColorGroup", &json!({"swatches": ["[Black]"]})).is_err(), "special swatches stay at the top");
        s.execute("swatch.moveToGroup", &json!({"swatches": [b], "group": "Brand"})).unwrap();
        let groups = |s: &Session| s.doc().unwrap().doc.color_groups.clone();
        assert_eq!(groups(&s)[0].swatches, [a.clone(), b.clone()]);
        let bytes = designcraft_idml::export_idml(&s.doc().unwrap().doc);
        let back = designcraft_idml::import_idml(&bytes).unwrap();
        assert_eq!(back.color_groups, groups(&s), "IDML ColorGroup");
        s.execute("swatch.delete", &json!({"name": b})).unwrap();
        assert_eq!(groups(&s)[0].swatches, [a.as_str()]);
        s.execute("swatch.renameColorGroup", &json!({"name": "Brand", "to": "Identity"})).unwrap();
        s.execute("swatch.ungroupColorGroup", &json!({"name": "Identity"})).unwrap();
        assert!(groups(&s).is_empty());
        assert!(s.doc().unwrap().doc.swatch(&a).is_some(), "ungrouping keeps the swatches");
    }
}

#[cfg(test)]
mod ink_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn ink_manager_converts_and_aliases_spots_in_pdf() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for (n, c) in [("Pantone A", [0, 50, 100, 0]), ("Pantone B", [100, 0, 0, 0])] {
            s.execute("swatch.create", &json!({"name": n, "spot": true, "color": {"c": c[0], "m": c[1], "y": c[2], "k": c[3]}})).unwrap();
        }
        let id = s.execute("frame.create", &json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "Pantone A", "ids": [id]})).unwrap();
        let pdf = |s: &mut Session| {
            let b = s.execute("file.exportPdf", &json!({})).unwrap()["base64"].as_str().unwrap().to_string();
            String::from_utf8_lossy(&crate::cmd::file::base64_decode(&b)).into_owned()
        };
        assert!(pdf(&mut s).contains("Pantone"), "a separation");
        s.execute("ink.options", &json!({"ink": "Pantone A", "alias": "Pantone B"})).unwrap();
        let p = pdf(&mut s);
        assert!(p.contains("Pantone#20B") || p.contains("Pantone B"), "aliased onto B");
        assert!(!p.contains("Pantone#20A") && !p.contains("/Pantone A"));
        s.execute("ink.options", &json!({"allToProcess": true})).unwrap();
        assert!(!pdf(&mut s).contains("Pantone"), "all spots to process");
        let l = s.execute("ink.list", &json!({})).unwrap();
        assert_eq!(l["inks"][0]["alias"], "Pantone B");
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        assert!(designcraft_pdf::inks::spot_output(&back, &designcraft_pdf::inks::ink_set(&back), "Pantone A").1, "IDML ConvertToProcess");
        assert!(back.inks.aliases.iter().any(|(a, b)| a == "Pantone A" && b == "Pantone B"), "IDML AliasInkName");
        assert!(s.execute("ink.options", &json!({"ink": "[Black]", "toProcess": true})).is_err());
        assert!(s.execute("ink.options", &json!({"ink": "Pantone B", "alias": "Pantone A"})).is_err(), "an alias loop is refused");
    }
}

#[cfg(test)]
mod swatch_options_tests {
    use designcraft_color::SwatchValue;
    use designcraft_doc::{ItemId, StoryId};
    use serde_json::json;

    use crate::Session;

    /// A document whose frame, text, styles, tint swatch and colour group all use "Brand".
    fn brand_doc() -> (Session, u64, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("swatch.create", &json!({"name": "Brand", "color": {"c": 0, "m": 80, "y": 100, "k": 0}})).unwrap();
        s.execute("swatch.create", &json!({"name": "Other", "color": "#108080"})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hello"})).unwrap();
        let (id, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        s.execute("object.fill", &json!({"swatch": "Brand", "ids": [id]})).unwrap();
        s.execute("object.stroke", &json!({"swatch": "Brand", "ids": [id]})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 3})).unwrap();
        s.execute("type.char", &json!({"attrs": {"fill": "Brand"}})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 5, "focus": 5})).unwrap();
        s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
        s.execute("table.select", &json!({"what": "table"})).unwrap();
        s.execute("table.setCell", &json!({"stroke": {"weight": 2, "type": "dashed", "gapColor": "Brand", "edges": ["all"]}})).unwrap();
        s.execute("swatch.newColorGroup", &json!({"name": "Brand Colors", "swatches": ["Brand"]})).unwrap();
        s.execute("style.character.create", &json!({"name": "Accent"})).unwrap();
        s.edit(|d, _| {
            d.swatches.push(designcraft_color::Swatch {
                name: "Brand 50%".into(),
                value: SwatchValue::Tint { base: "Brand".into(), tint: 0.5 },
                locked: false,
                named: true,
                hidden: false,
            });
            let st = d.styles_mut();
            if let Some(p) = st.para_mut(designcraft_doc::BASIC_PARAGRAPH) {
                p.chars.underline_color = Some("Brand".into());
                p.para.shading_color = Some("Brand".into());
                p.para.border_gap_color = Some("Brand".into());
                p.chars.ruby_stroke = Some("Brand".into());
            }
            if let Some(c) = st.char_style_mut("Accent") {
                c.chars.fill = Some("Brand".into());
                c.chars.kenten_fill = Some("Brand".into());
            }
            Ok(())
        })
        .unwrap();
        (s, id, sid)
    }

    fn uses(s: &Session, id: u64, sid: u64, name: &str) -> Vec<bool> {
        let d = &s.doc().unwrap().doc;
        let it = d.item(ItemId(id)).unwrap();
        let story = d.story(StoryId(sid)).unwrap();
        let basic = d.styles.para(designcraft_doc::BASIC_PARAGRAPH).unwrap();
        vec![
            it.fill.swatch == name,
            it.stroke.swatch == name,
            story.chars.iter().any(|r| r.format.over.fill.as_deref() == Some(name)),
            basic.chars.underline_color.as_deref() == Some(name),
            basic.para.shading_color.as_deref() == Some(name),
            d.styles.char_style("Accent").unwrap().chars.fill.as_deref() == Some(name),
            matches!(&d.swatch("Brand 50%").unwrap().value, SwatchValue::Tint { base, .. } if base == name),
            d.color_groups[0].swatches == [name],
            basic.para.border_gap_color.as_deref() == Some(name),
            basic.chars.ruby_stroke.as_deref() == Some(name),
            d.styles.char_style("Accent").unwrap().chars.kenten_fill.as_deref() == Some(name),
            story.tables.values().any(|t| t.cells.iter().all(|c| c.strokes.iter().all(|e| e.gap_color == name))),
        ]
    }

    #[test]
    fn renaming_a_swatch_renames_every_use_in_one_undo_step() {
        let (mut s, id, sid) = brand_doc();
        assert!(uses(&s, id, sid, "Brand").iter().all(|u| *u));
        let r = s.execute("swatch.options", &json!({"name": "Brand", "to": "Signal Orange", "spot": true})).unwrap();
        assert_eq!(r["name"], "Signal Orange");
        let d = &s.doc().unwrap().doc;
        assert!(d.swatch("Brand").is_none());
        assert!(matches!(d.swatch("Signal Orange").unwrap().value, SwatchValue::Color { color_type: designcraft_color::ColorType::Spot, .. }));
        assert_eq!(uses(&s, id, sid, "Signal Orange"), vec![true; 12]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.swatch("Brand").is_some());
        assert_eq!(uses(&s, id, sid, "Brand"), vec![true; 12]);
    }

    #[test]
    fn bad_swatch_renames_change_nothing() {
        let (mut s, id, sid) = brand_doc();
        let before = s.doc().unwrap().doc.clone();
        for p in [
            json!({"name": "Brand", "to": "Other"}),
            json!({"name": "Brand", "to": "  "}),
            json!({"name": "Brand", "to": "[Paper]"}),
            json!({"name": "[Black]", "to": "Ink"}),
            json!({"name": "[Registration]", "to": "Reg"}),
            json!({"name": "[None]", "to": "Nothing"}),
            json!({"name": "[Black]", "color": "#ff0000"}),
            json!({"name": "Brand 50%", "color": "#ff0000"}),
            json!({"name": "Missing", "to": "X"}),
        ] {
            assert!(s.execute("swatch.options", &p).is_err(), "{p}");
        }
        assert_eq!(s.doc().unwrap().doc.swatches, before.swatches);
        assert_eq!(uses(&s, id, sid, "Brand"), vec![true; 12]);
        // [Paper]'s colour can change; its name can't.
        s.execute("swatch.options", &json!({"name": "[Paper]", "to": "[Paper]", "color": "#fff8e0"})).unwrap();
    }

    #[test]
    fn name_with_color_value_tracks_the_colour() {
        let (mut s, id, sid) = brand_doc();
        let r = s.execute("swatch.options", &json!({"name": "Brand", "nameWithValue": true, "color": {"c": 10, "m": 20, "y": 30, "k": 40}})).unwrap();
        assert_eq!(r["name"], "C=10 M=20 Y=30 K=40");
        assert_eq!(uses(&s, id, sid, "C=10 M=20 Y=30 K=40"), vec![true; 12]);
        // Later colour edits keep the name in step until the swatch is named by hand.
        let r = s.execute("swatch.options", &json!({"name": "C=10 M=20 Y=30 K=40", "color": {"c": 0, "m": 0, "y": 0, "k": 50}})).unwrap();
        assert_eq!(r["name"], "C=0 M=0 Y=0 K=50");
        let r = s.execute("swatch.options", &json!({"name": "C=0 M=0 Y=0 K=50", "to": "Warm Gray"})).unwrap();
        assert_eq!(r["name"], "Warm Gray");
        assert!(s.doc().unwrap().doc.swatch("Warm Gray").unwrap().named);
    }
}

#[cfg(test)]
mod sample_color_tests {
    use designcraft_color::{Color, SwatchValue};
    use designcraft_doc::ItemId;
    use serde_json::json;

    use crate::Session;

    /// A 2×1 image, red then blue, placed 144 pt wide at (72, 72): each pixel is 72 pt.
    fn session_with_image() -> (Session, u64) {
        let png = designcraft_render::Rendered { width: 2, height: 1, pixels: vec![255, 0, 0, 255, 0, 0, 255, 255] }.to_png();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let b64 = crate::cmd::file::base64_encode(&png);
        let r = s.execute("file.place", &json!({"base64": b64, "name": "rb.png", "x": 72, "y": 72, "width": 144})).unwrap();
        (s, r["id"].as_u64().unwrap())
    }

    fn swatch_color(s: &Session, name: &str) -> Color {
        match &s.doc().unwrap().doc.swatch(name).unwrap().value {
            SwatchValue::Color { color, .. } => *color,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_eyedropper_colours_selected_text_with_the_image_under_it() {
        let (mut s, image) = session_with_image();
        let f = s.execute("frame.create", &json!({"rect": [300, 300, 500, 360], "content": "text", "text": "Elevate."})).unwrap();
        let fid = ItemId(f["id"].as_u64().unwrap());
        let story = s.doc().unwrap().doc.item(fid).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": story.0, "anchor": 0, "focus": 7})).unwrap();
        let undo = s.doc().unwrap().history.undo.len();
        let r = s.execute("color.sample", &json!({"at": [180, 100]})).unwrap();
        assert_eq!((r["color"].as_str(), r["from"].as_u64()), (Some("#0000ff"), Some(image)), "{r}");
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "one undo step");
        let name = r["swatch"].as_str().unwrap().to_string();
        assert_eq!(swatch_color(&s, &name), Color::rgb8(0, 0, 255));
        assert!(s.doc().unwrap().doc.swatch(&name).unwrap().hidden, "an unnamed colour");
        let st = s.doc().unwrap().doc.story(story).unwrap().clone();
        assert_eq!(st.format_after(0).over.fill.as_deref(), Some(name.as_str()));
        assert_eq!(st.format_after(7).over.fill, None, "the full stop keeps its colour");
        // The frame itself is not coloured.
        assert!(s.doc().unwrap().doc.item(fid).unwrap().fill.is_none());
    }

    #[test]
    fn the_eyedropper_fills_selected_objects_and_samples_locked_images() {
        let (mut s, _) = session_with_image();
        let lid = s.doc().unwrap().doc.layers[0].id.0;
        s.execute("layer.set", &json!({"id": lid, "locked": true})).unwrap();
        s.execute("layer.new", &json!({"name": "TEXT"})).unwrap();
        let rect = s.execute("frame.create", &json!({"rect": [300, 300, 400, 400]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [rect]})).unwrap();
        let r = s.execute("color.sample", &json!({"at": [100, 100], "target": "stroke"})).unwrap();
        assert_eq!(r["color"], "#ff0000", "{r}");
        let it = s.doc().unwrap().doc.item(ItemId(rect)).unwrap().clone();
        assert_eq!(swatch_color(&s, &it.stroke.swatch), Color::rgb8(255, 0, 0));
        // Sampling the same colour again reuses its swatch.
        let again = s.execute("color.sample", &json!({"at": [90, 90]})).unwrap();
        assert_eq!(again["swatch"], r["swatch"]);
    }

    #[test]
    fn a_caret_takes_the_sample_for_typing_and_nothing_to_sample_is_refused() {
        let (mut s, _) = session_with_image();
        let f = s.execute("frame.create", &json!({"rect": [300, 300, 500, 360], "content": "text", "text": "Hi"})).unwrap();
        let fid = ItemId(f["id"].as_u64().unwrap());
        let story = s.doc().unwrap().doc.item(fid).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": story.0, "anchor": 1, "focus": 1})).unwrap();
        // A caret: the sampled colour is for the next typing.
        let r = s.execute("color.sample", &json!({"at": [100, 100]})).unwrap();
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], r["swatch"]);
        assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "Hi", "caret color waits for typing");
        s.execute("text.insert", &json!({"text": "X"})).unwrap();
        let st = s.doc().unwrap().doc.story(story).unwrap().clone();
        assert_eq!(st.format_after(1).over.fill.as_deref(), r["swatch"].as_str(), "{r}");
        assert_eq!(st.format_after(0).over.fill, None, "the text before the caret keeps its colour");
        let undo = s.doc().unwrap().history.undo.len();
        s.execute("text.select", &json!({"story": story.0, "anchor": 0, "focus": 2})).unwrap();
        let none = s.execute("color.sample", &json!({"at": [600, 600]})).unwrap_err().to_string();
        assert!(none.contains("no placed image"), "{none}");
        // Just past the image's right edge.
        assert!(s.execute("color.sample", &json!({"at": [216.5, 100]})).is_err());
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }

    #[test]
    fn where_no_image_pixel_shows_the_clicked_frame_is_matched_instead() {
        let (mut s, image) = session_with_image();
        let rect = s.execute("frame.create", &json!({"rect": [300, 300, 400, 400]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [image]})).unwrap();
        // The frame reaches 72 pt below its image, and a clipping path shows the left (red) pixel only.
        s.edit(|d, _| {
            let it = d.item_mut(ItemId(image)).ok_or(crate::EngineError::NoDocument)?;
            let b = it.inner_bounds();
            it.path = designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(b.x0, b.y0, b.x1, b.y1 + 72.0));
            let designcraft_doc::Content::Graphic(g) = &mut it.content else { return Err(crate::EngineError::NoDocument) };
            let left = designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(0.0, 0.0, g.size.0 / 2.0, g.size.1));
            g.clip = Some(std::sync::Arc::new(designcraft_doc::ClippingPath::new(designcraft_doc::ClippingKind::PhotoshopPath, left)));
            Ok(())
        })
        .unwrap();
        s.execute("selection.set", &json!({"ids": [rect]})).unwrap();
        let r = s.execute("color.sample", &json!({"at": [100, 100], "matchFrom": image})).unwrap();
        assert_eq!(r["color"], "#ff0000", "inside the clipping path the image is sampled");
        // The clipped-out blue pixel and the empty frame below the image: nothing to sample, so the
        // frame's attributes apply.
        for at in [[180, 100], [100, 180]] {
            assert!(s.execute("color.sample", &json!({"at": at})).is_err(), "{at:?}");
            s.execute("object.fill", &json!({"swatch": "[Paper]", "ids": [rect]})).unwrap();
            s.execute("color.sample", &json!({"at": at, "matchFrom": image})).unwrap();
            assert_eq!(s.doc().unwrap().doc.item(ItemId(rect)).unwrap().fill.swatch, "[Black]", "{at:?}");
        }
    }

    #[test]
    fn sampling_and_explicit_colour_use_the_same_text_application_path() {
        for command in ["color.sample", "object.color"] {
            // Each command gets its own key: the eyedropper samples at `at`, object.color takes `color`.
            let params = |at: [f64; 2], color: &str, extra: serde_json::Value| {
                let mut p = if command == "color.sample" { json!({"at": at}) } else { json!({"color": color}) };
                if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
                    p.extend(extra.clone());
                }
                p
            };
            let (mut s, _) = session_with_image();
            let f = s.execute("frame.create", &json!({"rect": [300, 300, 500, 360], "content": "text"})).unwrap();
            let story = designcraft_doc::StoryId(f["story"].as_u64().unwrap());
            s.execute("text.select", &json!({"story": story.0, "anchor": 0})).unwrap();
            let undo = s.doc().unwrap().history.undo.len();
            let r = s.execute(command, &params([100.0, 100.0], "#ff0000", json!({}))).unwrap();
            assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "{command}");
            s.execute("text.insert", &json!({"text": "Hi"})).unwrap();
            let text = s.doc().unwrap().doc.story(story).unwrap();
            assert_eq!(text.format_after(0).over.fill.as_deref(), r["swatch"].as_str(), "empty-story typing: {command}");
            // Explicit ids override text targeting, including with a text selection active.
            let fid = ItemId(f["id"].as_u64().unwrap());
            s.execute("text.select", &json!({"story": story.0, "anchor": 0, "focus": 2})).unwrap();
            let r = s.execute(command, &params([180.0, 100.0], "#0000ff", json!({"id": fid.0}))).unwrap();
            assert_eq!(s.doc().unwrap().doc.item(fid).unwrap().fill.swatch, r["swatch"].as_str().unwrap());
            let text = s.doc().unwrap().doc.story(story).unwrap();
            assert_ne!(text.format_after(0).over.fill.as_deref(), r["swatch"].as_str(), "text unchanged: {command}");
            let r = s.execute(command, &params([180.0, 100.0], "#0000ff", json!({"target": "stroke"}))).unwrap();
            let text = s.doc().unwrap().doc.story(story).unwrap();
            assert_eq!(text.format_after(0).over.stroke.as_deref(), r["swatch"].as_str(), "selected text stroke: {command}");
        }
    }

    #[test]
    fn sampling_with_the_stroke_in_front_colours_a_lines_stroke() {
        let (mut s, _) = session_with_image();
        let line = s.execute("line.create", &json!({"a": [300, 300], "b": [500, 300]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [line]})).unwrap();
        let r = s.execute("color.sample", &json!({"at": [100, 100], "target": "stroke"})).unwrap();
        let it = s.doc().unwrap().doc.item(ItemId(line)).unwrap().clone();
        assert_eq!(it.stroke.swatch, r["swatch"].as_str().unwrap(), "{r}");
        assert!(it.fill.is_none(), "the fill stays empty");
    }
}

#[cfg(test)]
mod radial_gradient_tests {
    use serde_json::json;

    /// White at the side and the top of a 100×200 radial (black → white) rectangle at (50, 20) on
    /// a 200×240 page, rendered at 1 px/pt.
    fn sides(img: &designcraft_render::Rendered) -> (u8, u8, u8) {
        (img.pixel(100, 120)[0], img.pixel(52, 120)[0], img.pixel(100, 22)[0])
    }

    #[test]
    fn a_radial_gradient_is_the_objects_ellipse_on_screen_and_in_pdf() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"width": 200, "height": 240, "facingPages": false})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [50, 20, 150, 220], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.stroke", &json!({"ids": [id], "swatch": "[None]"})).unwrap();
        s.execute(
            "object.gradient",
            &json!({"ids": [id], "kind": "radial", "stops": [{"location": 0, "color": "#000000"}, {"location": 100, "color": "#ffffff"}]}),
        )
        .unwrap();
        let d = s.doc().unwrap().doc.clone();
        let mut r = designcraft_render::Renderer::new();
        let (c, side, top) = sides(&r.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap());
        assert!(c < 30 && side > 220 && top > 220, "screen: centre {c}, side {side}, top {top}");
        // The PDF, placed back and drawn: the same ellipse.
        let pdf = designcraft_pdf::export_pdf(&d, &s.cache, &Default::default()).unwrap();
        let mut p = crate::Session::new();
        p.execute("file.new", &json!({"width": 200, "height": 240, "facingPages": false})).unwrap();
        p.execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&pdf), "name": "radial.pdf", "x": 0, "y": 0, "width": 200}))
            .unwrap();
        let d = p.doc().unwrap().doc.clone();
        let (c, side, top) = sides(&r.render_page(&d, &p.cache, 0, 1.0, false, &Default::default()).unwrap());
        assert!(c < 30 && side > 220 && top > 220, "pdf: centre {c}, side {side}, top {top}");
    }

    #[test]
    fn a_box_with_no_height_or_width_has_a_circle_across_its_length() {
        let f = designcraft_doc::Fill::swatch("g");
        let line = designcraft_geom::Rect::new(20.0, 60.0, 180.0, 60.0);
        let (c, rx, ry) = f.radial_ellipse(line);
        assert_eq!((c.x, c.y, rx, ry), (100.0, 60.0, 80.0, 80.0));
        let (_, rx, ry) = f.radial_ellipse(designcraft_geom::Rect::new(10.0, 0.0, 10.0, 50.0));
        assert_eq!((rx, ry), (25.0, 25.0));
        // A point: a tiny circle, never a zero radius.
        let (_, rx, ry) = f.radial_ellipse(designcraft_geom::Rect::new(5.0, 5.0, 5.0, 5.0));
        assert!(rx > 0.0 && rx == ry);
        // A box keeps its ellipse.
        let (_, rx, ry) = f.radial_ellipse(designcraft_geom::Rect::new(0.0, 0.0, 100.0, 200.0));
        assert_eq!((rx, ry), (50.0, 100.0));
    }
}

#[cfg(test)]
mod gradient_swatch_tests {
    use designcraft_color::SwatchValue;
    use serde_json::json;

    fn doc() -> crate::Session {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s
    }

    #[test]
    fn a_gradient_swatch_from_colour_swatches() {
        let mut s = doc();
        // As the Gradient panel's stops take swatches dragged onto them.
        let r = s
            .execute(
                "swatch.create",
                &json!({"gradient": {"stops": [{"location": 100, "swatch": "C=100 M=0 Y=0 K=0"}, {"location": 0, "swatch": "C=0 M=100 Y=0 K=0"}]}}),
            )
            .unwrap();
        assert_eq!(r["name"], "New Gradient Swatch");
        let d = &s.doc().unwrap().doc;
        let w = d.swatch("New Gradient Swatch").unwrap();
        assert!(w.named && !w.hidden);
        let SwatchValue::Gradient { gradient } = &w.value else { panic!("{w:?}") };
        assert_eq!(gradient.stops[0].color, d.resolve_color("C=0 M=100 Y=0 K=0", 1.0).unwrap(), "sorted: magenta first");
        assert_eq!(gradient.stops[1].color, d.resolve_color("C=100 M=0 Y=0 K=0", 1.0).unwrap());
        // A second one gets its own name; bad stops are refused.
        let r = s
            .execute(
                "swatch.create",
                &json!({"gradient": {"kind": "radial", "stops": [{"color": "#ff0000"}, {"location": 100, "color": "#0000ff"}]}}),
            )
            .unwrap();
        assert_eq!(r["name"], "New Gradient Swatch 2");
        for bad in [
            json!({"stops": [{"swatch": "C=0 M=100 Y=0 K=0"}]}),
            json!({"stops": [{"swatch": "Nope"}, {"swatch": "[Black]"}]}),
            json!({"stops": [{"swatch": "New Gradient Swatch"}, {"swatch": "[Black]"}]}),
            json!({"kind": "conic", "stops": [{"color": "#000000"}, {"color": "#ffffff"}]}),
        ] {
            assert!(s.execute("swatch.create", &json!({"gradient": bad})).is_err(), "{bad}");
        }
    }

    #[test]
    fn gradient_stops_follow_their_swatch() {
        let mut s = doc();
        s.execute("swatch.create", &json!({"name": "Gold", "color": {"c": 0, "m": 30, "y": 90, "k": 10}, "spot": true})).unwrap();
        s.execute(
            "swatch.create",
            &json!({"name": "G", "gradient": {"stops": [{"location": 0, "swatch": "Gold"}, {"location": 100, "swatch": "[Paper]"}]}}),
        )
        .unwrap();
        let stops = |s: &crate::Session| match &s.doc().unwrap().doc.swatch("G").unwrap().value {
            SwatchValue::Gradient { gradient } => gradient.stops.clone(),
            v => panic!("{v:?}"),
        };
        assert_eq!(stops(&s)[0].swatch.as_deref(), Some("Gold"));
        assert_eq!(stops(&s)[1].swatch.as_deref(), Some("[Paper]"));
        // Renamed and edited: the stop names the new name and shows the new color.
        s.execute("swatch.options", &json!({"name": "Gold", "to": "Gold 871", "color": {"c": 0, "m": 20, "y": 80, "k": 20}})).unwrap();
        assert_eq!(stops(&s)[0].swatch.as_deref(), Some("Gold 871"));
        assert_eq!(stops(&s)[0].color, designcraft_color::Color::cmyk(0.0, 0.2, 0.8, 0.2));
        // Deleted: the stop keeps its color as a plain one.
        s.execute("swatch.delete", &json!({"name": "Gold 871"})).unwrap();
        assert_eq!(stops(&s)[0].swatch, None);
        assert_eq!(stops(&s)[0].color, designcraft_color::Color::cmyk(0.0, 0.2, 0.8, 0.2));
    }

    #[test]
    fn new_swatch_names_the_selections_gradient_or_duplicates_a_swatch() {
        let mut s = doc();
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 100, 100], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        // A gradient made in the Gradient panel is unnamed (and not listed) …
        s.execute("object.gradient", &json!({"ids": [id], "stops": [{"location": 0, "color": {"c": 0, "m": 100, "y": 0, "k": 0}}, {"location": 100, "color": {"c": 100, "m": 0, "y": 0, "k": 0}}]})).unwrap();
        let fill = |s: &crate::Session| s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch.clone();
        assert!(s.doc().unwrap().doc.swatch(&fill(&s)).unwrap().hidden);
        // … until New Swatch: "New Gradient Swatch", listed, still the object's fill.
        s.execute("selection.set", &json!({"ids": [id]})).unwrap();
        assert_eq!(s.execute("swatch.new", &json!({})).unwrap()["name"], "New Gradient Swatch");
        assert_eq!(fill(&s), "New Gradient Swatch");
        let w = s.doc().unwrap().doc.swatch("New Gradient Swatch").unwrap().clone();
        assert!(w.named && !w.hidden && matches!(w.value, SwatchValue::Gradient { .. }));
        // A named swatch: a copy.
        assert_eq!(s.execute("swatch.new", &json!({})).unwrap()["name"], "New Gradient Swatch copy");
        assert_eq!(s.execute("swatch.new", &json!({"swatch": "C=100 M=0 Y=0 K=0"})).unwrap()["name"], "C=100 M=0 Y=0 K=0 copy");
        assert!(s.execute("swatch.new", &json!({"swatch": "[None]"})).is_err());
        // The stroke in front: the stroke's swatch.
        s.execute("object.stroke", &json!({"ids": [id], "swatch": "C=100 M=0 Y=0 K=0"})).unwrap();
        assert_eq!(s.execute("swatch.new", &json!({"target": "stroke"})).unwrap()["name"], "C=100 M=0 Y=0 K=0 copy 2");
        // Nothing selected, no swatch given: a new colour swatch.
        s.execute("edit.deselectAll", &json!({})).unwrap();
        let n = s.execute("swatch.new", &json!({})).unwrap()["name"].as_str().unwrap().to_string();
        let w = s.doc().unwrap().doc.swatch(&n).unwrap();
        assert!(w.named && !w.hidden && matches!(w.value, SwatchValue::Color { .. }), "{w:?}");
    }
}

#[cfg(test)]
mod gradient_stroke_tests {
    use serde_json::json;

    /// Grey along a 20 pt line from (20, 60) to (180, 60): near its start, middle and end.
    fn along(img: &designcraft_render::Rendered) -> (u8, u8, u8) {
        (img.pixel(26, 60)[0], img.pixel(100, 60)[0], img.pixel(174, 60)[0])
    }

    #[test]
    fn a_gradient_on_a_stroke_runs_across_the_path_on_screen_and_in_pdf() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"width": 200, "height": 120, "facingPages": false})).unwrap();
        let id = s.execute("line.create", &json!({"a": [20, 60], "b": [180, 60]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.stroke", &json!({"ids": [id], "weight": 20})).unwrap();
        let r = s
            .execute(
                "object.gradient",
                &json!({"ids": [id], "target": "stroke", "stops": [{"location": 0, "color": "#000000"}, {"location": 100, "color": "#ffffff"}]}),
            )
            .unwrap();
        let d = s.doc().unwrap().doc.clone();
        let it = d.item(designcraft_doc::ItemId(id)).unwrap();
        assert_eq!(it.stroke.swatch, r["swatch"].as_str().unwrap());
        assert_eq!(it.fill.swatch, designcraft_color::swatch::NONE, "the fill is left alone");
        let mut rr = designcraft_render::Renderer::new();
        let (a, m, b) = along(&rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap());
        assert!(a < 40 && (90..170).contains(&m) && b > 215, "screen: {a} {m} {b}");
        // The PDF, placed back and drawn: the same.
        let pdf = designcraft_pdf::export_pdf(&d, &s.cache, &Default::default()).unwrap();
        let mut p = crate::Session::new();
        p.execute("file.new", &json!({"width": 200, "height": 120, "facingPages": false})).unwrap();
        p.execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&pdf), "name": "stroke.pdf", "x": 0, "y": 0, "width": 200}))
            .unwrap();
        let pd = p.doc().unwrap().doc.clone();
        let (a, m, b) = along(&rr.render_page(&pd, &p.cache, 0, 1.0, false, &Default::default()).unwrap());
        assert!(a < 40 && (90..170).contains(&m) && b > 215, "pdf: {a} {m} {b}");
        // The Gradient Swatch tool's drag on the stroke: reversed by dragging right to left.
        s.execute("object.gradient", &json!({"ids": [id], "target": "stroke", "from": [180, 60], "to": [20, 60]})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let (a, _, b) = along(&rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap());
        assert!(a > 215 && b < 40, "dragged: {a} {b}");
        assert!(s.execute("object.gradient", &json!({"ids": [id], "target": "gap"})).is_err());
    }

    /// A 20 pt horizontal line with a gradient on its stroke, drawn on screen and from its PDF.
    fn line_with_stroke_gradient(g: serde_json::Value) -> (designcraft_render::Rendered, designcraft_render::Rendered) {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"width": 200, "height": 120, "facingPages": false})).unwrap();
        let id = s.execute("line.create", &json!({"a": [20, 60], "b": [180, 60]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.stroke", &json!({"ids": [id], "weight": 20})).unwrap();
        let mut g = g;
        g["ids"] = json!([id]);
        g["target"] = json!("stroke");
        s.execute("object.gradient", &g).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let mut rr = designcraft_render::Renderer::new();
        let screen = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        let pdf = designcraft_pdf::export_pdf(&d, &s.cache, &Default::default()).unwrap();
        let mut p = crate::Session::new();
        p.execute("file.new", &json!({"width": 200, "height": 120, "facingPages": false})).unwrap();
        p.execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&pdf), "name": "line.pdf", "x": 0, "y": 0, "width": 200})).unwrap();
        let pd = p.doc().unwrap().doc.clone();
        let placed = rr.render_page(&pd, &p.cache, 0, 1.0, false, &Default::default()).unwrap();
        (screen, placed)
    }

    #[test]
    fn a_radial_gradient_on_a_straight_lines_stroke_shows_all_its_stops() {
        // The line's box has no height: the gradient is a circle across the line's length.
        let (screen, pdf) = line_with_stroke_gradient(
            json!({"kind": "radial", "stops": [{"location": 0, "color": "#000000"}, {"location": 100, "color": "#ffffff"}]}),
        );
        for (what, img) in [("screen", &screen), ("pdf", &pdf)] {
            let (a, m, b) = along(img);
            assert!(m < 40 && a > 180 && b > 180, "{what}: {a} {m} {b}");
        }
    }

    #[test]
    fn a_linear_gradient_across_a_straight_line_paints_its_stroke_alike_on_screen_and_in_pdf() {
        let (screen, pdf) =
            line_with_stroke_gradient(json!({"angle": 90, "stops": [{"location": 0, "color": "#ff0000"}, {"location": 100, "color": "#0000ff"}]}));
        let (s, p) = (screen.pixel(100, 60), pdf.pixel(100, 60));
        // The line's box has no height: the gradient runs across the line's length, so the stroke
        // shows its middle.
        assert!(s[1] < 60 && s[0] > 80 && s[2] > 80, "screen: the stroke is painted, {s:?}");
        assert!((0..3).all(|i| s[i].abs_diff(p[i]) < 40), "screen {s:?}, pdf {p:?}");
    }
}
