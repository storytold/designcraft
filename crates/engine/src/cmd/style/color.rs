use super::swatches::{parse_color, value_name};
use super::{CharAttrs, Result, Session, Styles, Swatch, SwatchValue, Value, bad, has_doc, json, str_param};

fn gradient_json(name: &str, g: &designcraft_color::Gradient) -> Value {
    let stops: Vec<Value> = g
        .stops
        .iter()
        .map(|s| {
            let mut v = json!({"location": (s.offset as f64 * 1000.0).round() / 10.0, "color": s.color.to_hex(), "opacity": (s.opacity * 100.0).round(), "midpoint": (s.midpoint * 100.0).round()});
            if let (Some(o), Some(w)) = (v.as_object_mut(), &s.swatch) {
                o.insert("swatch".into(), json!(w));
            }
            v
        })
        .collect();
    json!({"swatch": name, "kind": if g.kind == designcraft_color::GradientKind::Radial { "radial" } else { "linear" }, "stops": stops})
}

pub(super) fn apply_gradient(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_color::{GradientKind, GradientStop};
    let kind = match str_param(p, "kind") {
        Some("radial") => Some(GradientKind::Radial),
        Some("linear") => Some(GradientKind::Linear),
        Some(k) => return Err(bad("object.gradient", format!("unknown kind {k}"))),
        None => None,
    };
    let stops = match p.get("stops").and_then(Value::as_array) {
        Some(a) => {
            let doc = &s.doc()?.doc;
            let mut v = Vec::new();
            for st in a {
                // A swatch dropped on a stop gives it the swatch's color (a tint's mixed in) and
                // its inks for print.
                // A stop naming a swatch that's gone keeps the color given with it.
                let color = st.get("color").and_then(parse_color);
                let (color, swatch) = match st.get("swatch").and_then(Value::as_str) {
                    Some(name) => {
                        match doc.resolve_color(name, 1.0).filter(|_| designcraft_color::swatch::resolve_gradient(&doc.swatches, name).is_none()) {
                            Some(c) => (c, Some(name.to_string())),
                            None => (color.ok_or_else(|| bad("object.gradient", format!("`{name}` isn't a color swatch")))?, None),
                        }
                    }
                    None => (color.ok_or_else(|| bad("object.gradient", "a stop needs a color or a swatch"))?, None),
                };
                let pct = |k: &str, d: f64| st.get(k).and_then(Value::as_f64).unwrap_or(d).clamp(0.0, 100.0) as f32 / 100.0;
                v.push(GradientStop {
                    offset: pct("location", 0.0),
                    color,
                    opacity: pct("opacity", 100.0),
                    midpoint: pct("midpoint", 50.0).clamp(0.13, 0.87),
                    swatch,
                });
            }
            if v.len() < 2 {
                return Err(bad("object.gradient", "a gradient needs at least two stops"));
            }
            Some(v)
        }
        None => None,
    };
    let reverse = p.get("reverse").and_then(Value::as_bool).unwrap_or(false);
    let angle = p.get("angle").and_then(Value::as_f64);
    let stroke = match str_param(p, "target") {
        None | Some("fill") => false,
        Some("stroke") => true,
        Some(t) => return Err(bad("object.gradient", format!("unknown target {t}"))),
    };
    let (from, to) = (crate::cmd::point_param(p, "from"), crate::cmd::point_param(p, "to"));
    let ids = crate::cmd::targets(s, p)?;
    s.edit(|d, _| {
        let mut out = Value::Null;
        for id in &ids {
            let Some(it) = d.item(*id) else { continue };
            let cur_name = if stroke { it.stroke.swatch.clone() } else { it.fill.swatch.clone() };
            let cur = designcraft_color::swatch::resolve_gradient(&d.swatches, &cur_name).cloned();
            let mut g = cur.clone().unwrap_or_default();
            if let Some(k) = kind {
                g.kind = k;
            }
            if let Some(st) = &stops {
                g.stops = st.clone();
                g.sort();
            }
            if reverse {
                g.reverse();
            }
            // Keep the swatch while the gradient is unchanged, else reuse an equal gradient swatch
            // or add an unnamed one.
            let name = if cur.as_ref() == Some(&g) {
                cur_name
            } else if let Some(w) = d.swatches.iter().find(|w| matches!(&w.value, SwatchValue::Gradient { gradient } if *gradient == g)) {
                w.name.clone()
            } else {
                let name = Styles::unique_name(|n| d.swatch(n).is_some(), "Gradient");
                d.swatches.push(Swatch {
                    name: name.clone(),
                    value: SwatchValue::Gradient { gradient: g.clone() },
                    locked: false,
                    named: false,
                    hidden: true,
                });
                name
            };
            let inv = d.find(*id).map(|l| d.parent_xf(&l)).zip(d.item(*id).map(|i| i.xf)).map(|(pxf, xf)| (pxf * xf).inverse());
            let Some(it) = d.item_mut(*id) else { continue };
            // The fill's or the stroke's paint: swatch, tint, angle, vector.
            let (swatch, tint, g_angle, g_vector) = if stroke {
                (&mut it.stroke.swatch, &mut it.stroke.tint, &mut it.stroke.gradient_angle, &mut it.stroke.gradient_vector)
            } else {
                (&mut it.fill.swatch, &mut it.fill.tint, &mut it.fill.gradient_angle, &mut it.fill.gradient_vector)
            };
            *swatch = name.clone();
            *tint = 1.0;
            if let Some(a) = angle {
                *g_angle = Some(a);
                *g_vector = None;
            }
            if let (Some(a), Some(b), Some(inv)) = (from, to, inv) {
                let (a, b) = (inv * a, inv * b);
                if (b - a).hypot() > 1e-6 {
                    *g_vector = Some([a.x, a.y, b.x, b.y]);
                }
            }
            if out.is_null() {
                out = gradient_json(&name, &g);
            }
        }
        Ok(out)
    })
}

pub(super) fn apply_color(s: &mut Session, p: &Value) -> Result<Value> {
    let color = p.get("color").and_then(parse_color).ok_or_else(|| bad("object.color", "missing or bad color"))?;
    let stroke = str_param(p, "target") == Some("stroke");
    let named = p.get("ids").is_some() || p.get("id").is_some();
    let text = !named
        && (p.get("text").and_then(Value::as_bool).unwrap_or(false) || s.doc()?.selection.text.is_some() || s.doc()?.selection.cells.is_some());
    // Nothing selected: the colour of new shapes (InDesign's document defaults).
    if !text && crate::cmd::object::nothing_targeted(s, p)? {
        return s.edit(|d, _| {
            let name = color_swatch(d, color);
            let c = &mut d.settings.new_object;
            if stroke {
                (c.stroke, c.stroke_tint) = (name.clone(), 1.0);
            } else {
                (c.fill, c.fill_tint) = (name.clone(), 1.0);
            }
            Ok(json!({"swatch": name, "newObject": true}))
        });
    }
    let (ids, texts) = if text { (vec![], crate::cmd::text::format_targets_pub(s)) } else { (crate::cmd::targets(s, p)?, vec![]) };
    if text && texts.is_empty() {
        return Err(bad("object.color", "select the text to colour"));
    }
    // A caret: the colour is for the next typing (once the swatch exists).
    let caret = matches!(texts.as_slice(), [t] if t.range.is_empty());
    let r = s.edit(|d, _| {
        let name = color_swatch(d, color);
        color_items(d, &ids, &name, stroke);
        if !caret {
            let a = if stroke {
                CharAttrs { stroke: Some(name.clone()), stroke_tint: Some(1.0), ..Default::default() }
            } else {
                CharAttrs { fill: Some(name.clone()), fill_tint: Some(1.0), ..Default::default() }
            };
            crate::cmd::text::apply_char_attrs(d, &texts, &a, &[]);
        }
        Ok(json!({"swatch": name}))
    })?;
    if caret && let Some(name) = r.get("swatch").cloned() {
        let attrs = if stroke { json!({"stroke": name, "strokeTint": 1.0}) } else { json!({"fill": name, "fillTint": 1.0}) };
        crate::cmd::text::format_chars(s, &attrs)?;
    }
    Ok(r)
}

/// A process swatch holding this colour to the precision its name shows (whole CMYK percents,
/// 0-255 RGB), else a new unnamed one (hidden from the Swatches panel until Add to Swatches):
/// values a fraction of a percent apart would otherwise add hidden "C=0 M=50 Y=100 K=0 2" twins.
fn color_swatch(d: &mut designcraft_doc::Document, color: designcraft_color::Color) -> String {
    let shown = value_name(color);
    let process = |w: &&Swatch| match &w.value {
        SwatchValue::Color { color: c, color_type: designcraft_color::ColorType::Process } => Some(*c),
        _ => None,
    };
    let existing = d
        .swatches
        .iter()
        .find(|w| process(w) == Some(color))
        .or_else(|| {
            d.swatches
                .iter()
                .find(|w| process(w).is_some_and(|c| std::mem::discriminant(&c) == std::mem::discriminant(&color) && value_name(c) == shown))
        })
        .map(|w| w.name.clone());
    if let Some(n) = existing {
        return n;
    }
    let name = Styles::unique_name(|n| d.swatch(n).is_some(), &shown);
    d.swatches.push(Swatch {
        name: name.clone(),
        value: SwatchValue::Color { color, color_type: designcraft_color::ColorType::Process },
        locked: false,
        named: false,
        hidden: true,
    });
    name
}

fn color_items(d: &mut designcraft_doc::Document, ids: &[designcraft_doc::ItemId], name: &str, stroke: bool) {
    for id in ids {
        if let Some(it) = d.item_mut(*id) {
            if stroke {
                it.stroke.swatch = name.to_string();
                it.stroke.tint = 1.0;
            } else {
                it.fill.swatch = name.to_string();
                it.fill.tint = 1.0;
            }
        }
    }
}

pub(super) fn has_text_or_selection(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.text.is_some() || d.selection.cells.is_some()) { Ok(()) } else { crate::cmd::has_selection(s) }
}

/// The Eyedropper on a placed image (InDesign samples the pixel under the pointer): that colour,
/// as an unnamed colour, fills the selected text (characters), or the selected objects.
pub(super) fn sample_color(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "color.sample";
    let at = crate::cmd::point_param(p, "at").ok_or_else(|| bad(ID, "missing at"))?;
    let sr = crate::cmd::spread_param(p, ID)?;
    let st = s.doc()?;
    let Some((from, color)) = image_color_at(&st.doc, sr, at) else {
        // No opaque image pixel there (an empty part of an image frame, a transparent pixel): the
        // Eyedropper applies the clicked object's attributes instead.
        return match p.get("matchFrom") {
            Some(from) => {
                let mut params = p.clone();
                params["from"] = from.clone();
                crate::cmd::object::match_attributes(s, &params)
            }
            None => Err(bad(ID, "no placed image under the point")),
        };
    };
    let hex = color.to_hex();
    let mut params = p.clone();
    params["color"] = json!(hex);
    let mut result = apply_color(s, &params)?;
    result["color"] = json!(hex);
    result["from"] = json!(from.0);
    Ok(result)
}

/// The colour of the placed raster image at `at` on spread `sr`: the frontmost visible image whose
/// frame shows that point (locked ones too, as sampling changes nothing). (image frame, colour);
/// None over no image, a transparent pixel, or a graphic that is not a raster (PDF, EPS).
pub(crate) fn image_color_at(
    d: &designcraft_doc::Document,
    sr: designcraft_doc::SpreadRef,
    at: designcraft_geom::Point,
) -> Option<(designcraft_doc::ItemId, designcraft_color::Color)> {
    let sp = d.spread(sr)?;
    let mut items: Vec<&designcraft_doc::Item> = sp.items.iter().map(|i| &**i).collect();
    items.sort_by_key(|i| d.layer_rank(i.layer));
    items
        .into_iter()
        .rev()
        .filter(|it| d.layer(it.layer).is_none_or(|l| l.visible))
        .find_map(|it| sample_item(d, it, designcraft_geom::Affine::IDENTITY, at, 0))
}

fn sample_item(
    d: &designcraft_doc::Document,
    it: &designcraft_doc::Item,
    outer: designcraft_geom::Affine,
    at: designcraft_geom::Point,
    depth: usize,
) -> Option<(designcraft_doc::ItemId, designcraft_color::Color)> {
    use designcraft_doc::Content;
    if it.hidden || depth > 64 {
        return None;
    }
    let xf = outer * it.xf;
    match &it.content {
        // Only the active state of a multi-state object shows.
        Content::Group { .. } => {
            let shown: Vec<_> = it.shown_children().collect();
            shown.into_iter().rev().find_map(|c| sample_item(d, c, xf, at, depth + 1))
        }
        Content::Graphic(g) => {
            let inner = xf.inverse() * at;
            if !designcraft_geom::hit::fill_contains(&it.path.to_bezpath(), designcraft_geom::FillRule::NonZero, inner) {
                return None;
            }
            // A clipping path hides the image outside it.
            if let Some((region, even_odd)) = g.clip_region() {
                let rule = if even_odd { designcraft_geom::FillRule::EvenOdd } else { designcraft_geom::FillRule::NonZero };
                if !designcraft_geom::hit::fill_contains(&region, rule, inner) {
                    return None;
                }
            }
            let gp = g.xf.inverse() * inner;
            let (w, h) = g.size;
            // Written so that NaN (a degenerate transform) is outside too.
            if !(w > 0.0 && h > 0.0 && gp.x >= 0.0 && gp.y >= 0.0 && gp.x < w && gp.y < h) {
                return None;
            }
            let asset = d.assets.get(&g.asset).filter(|a| a.is_raster())?;
            let img = designcraft_images::decode_rgba(&asset.data)?;
            let (pw, ph) = img.dimensions();
            let px = ((gp.x / w * f64::from(pw)) as u32).min(pw.checked_sub(1)?);
            let py = ((gp.y / h * f64::from(ph)) as u32).min(ph.checked_sub(1)?);
            let [r, gr, b, a] = img.get_pixel_checked(px, py)?.0;
            (a > 0).then(|| (it.id, designcraft_color::Color::rgb8(r, gr, b)))
        }
        _ => None,
    }
}

/// Renaming a swatch walks every text inside a story (table cells, notes): refuse the whole edit
/// for nesting deeper than 32 levels rather than leave deeper references at a removed name.
pub(super) fn check_text_nesting(d: &designcraft_doc::Document, id: &str) -> Result<()> {
    fn within(st: &designcraft_doc::Story, depth: usize) -> bool {
        depth <= 32
            && st.tables.values().all(|t| t.cells.iter().all(|c| within(&c.text, depth + 1)))
            && st.notes.iter().chain(&st.endnotes).all(|n| within(&n.text, depth + 1))
    }
    if d.stories.values().all(|st| within(st, 0)) { Ok(()) } else { Err(bad(id, "story nesting exceeds 32 levels")) }
}
