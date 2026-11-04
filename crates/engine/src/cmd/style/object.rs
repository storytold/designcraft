use super::{Effects, Item, ObjectStyle, Result, Session, Styles, Value, bad, json, str_param};

pub(super) fn apply_object(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.object.apply", "missing name"))?.to_string();
    let os = s.doc()?.doc.styles.object_style(&name).cloned().ok_or_else(|| bad("style.object.apply", format!("no object style `{name}`")))?;
    let ids = crate::cmd::targets(s, p)?;
    s.edit(|d, _| {
        let mut stories = Vec::new();
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            it.object_style = name.clone();
            os.apply_to(it);
            if let Some(tf) = it.text_frame() {
                stories.push(tf.story);
            }
        }
        if let Some(ps) = &os.paragraph_style {
            for sid in stories {
                if let Some(st) = d.story_mut(sid) {
                    let len = st.len();
                    st.format_paras(0..len, |f| f.style = ps.clone());
                }
            }
        }
        Ok(Value::Null)
    })
}

pub(super) fn clear_object_overrides(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = crate::cmd::targets(s, p)?;
    // Each target with its style's attributes applied, kept only where that changes the item;
    // and a text frame's story back in the style's paragraph style, as applying the style does.
    let d = &s.doc()?.doc;
    let mut changed: Vec<Item> = Vec::new();
    let mut restyled: Vec<(designcraft_doc::StoryId, String)> = Vec::new();
    let mut n = 0;
    for id in &ids {
        let Some(it) = d.item(*id) else { continue };
        let Some(os) = d.styles.object_style(&it.object_style) else { continue };
        let mut next = it.clone();
        os.apply_to(&mut next);
        let item_changed = next != *it;
        if item_changed {
            changed.push(next);
        }
        let mut story_changed = false;
        if let (Some(ps), Some(tf)) = (&os.paragraph_style, it.text_frame())
            && d.story(tf.story).is_some_and(|st| st.paras.iter().any(|f| f.style != *ps))
        {
            story_changed = true;
            if !restyled.iter().any(|(sid, _)| *sid == tf.story) {
                restyled.push((tf.story, ps.clone()));
            }
        }
        n += usize::from(item_changed || story_changed);
    }
    if n == 0 {
        return Ok(json!({"changed": 0}));
    }
    s.edit(|d, _| {
        for next in &changed {
            if let Some(it) = d.item_mut(next.id) {
                *it = next.clone();
            }
        }
        for (sid, ps) in &restyled {
            if let Some(st) = d.story_mut(*sid) {
                let len = st.len();
                st.format_paras(0..len, |f| f.style = ps.clone());
            }
        }
        Ok(json!({"changed": n}))
    })
}

/// Effect keys accepted at the top level of `style.object.create` / `edit` as shorthand for
/// `effects.<key>`.
const EFFECT_KEYS: [&str; 9] =
    ["dropShadow", "feather", "innerShadow", "outerGlow", "innerGlow", "bevel", "satin", "gradientFeather", "directionalFeather"];

/// The effects `p` gives (`effects`, then the top-level effect keys) merged field by field over
/// `base`, or `None` when `p` names no effect.
fn effects_param(cmd: &str, p: &Value, base: &Effects) -> Result<Option<Effects>> {
    let mut patch = serde_json::Map::new();
    match p.get("effects") {
        None | Some(Value::Null) => {}
        Some(Value::Object(o)) => patch.extend(o.iter().map(|(k, v)| (k.clone(), v.clone()))),
        Some(_) => return Err(bad(cmd, "`effects` must be an object or null")),
    }
    for k in EFFECT_KEYS {
        if let Some(v) = p.get(k) {
            patch.insert(k.to_string(), v.clone());
        }
    }
    if patch.is_empty() {
        return Ok(None);
    }
    let mut v = serde_json::to_value(base).map_err(|e| bad(cmd, e.to_string()))?;
    merge_json(&mut v, Value::Object(patch), 0);
    let e: Effects = serde_json::from_value(v).map_err(|e| bad(cmd, format!("effects: {e}")))?;
    e.sanitized().map(Some).map_err(|m| bad(cmd, m))
}

/// Merges `top` into `base`: objects key by key, anything else replaced.
fn merge_json(base: &mut Value, top: Value, depth: usize) {
    match (base, top) {
        (Value::Object(b), Value::Object(t)) if depth < 8 => {
            for (k, v) in t {
                match b.get_mut(&k) {
                    Some(slot) => merge_json(slot, v, depth + 1),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, t) => *b = t,
    }
}

pub(super) fn create_object(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Object Style 1").to_string();
    let from_sel = p.get("fromSelection").and_then(Value::as_bool).unwrap_or(true);
    let src = if from_sel { s.doc()?.selection.items.first().and_then(|i| s.doc().ok()?.doc.item(*i).cloned()) } else { None };
    let mut os = ObjectStyle::default();
    if let Some(it) = &src {
        os.fill = Some(it.fill.clone());
        os.stroke = Some(it.stroke.clone());
        os.corners = Some(it.corners.path_to_named(&it.path));
        os.text_frame = it.text_frame().map(|t| t.options.clone());
        os.effects = Some(it.effects.clone());
    }
    const CREATE: &str = "style.object.create";
    if let Some(f) = fill_param(CREATE, p, os.fill.as_ref())? {
        os.fill = f;
    }
    if let Some(st) = stroke_param(CREATE, p, os.stroke.as_ref())? {
        os.stroke = st;
    }
    if let Some(c) = corners_param(CREATE, p, os.corners.unwrap_or_default())? {
        os.corners = c;
    }
    if let Some(ps) = str_param(p, "paragraphStyle") {
        os.paragraph_style = Some(ps.to_string());
    }
    if let Some(e) = effects_param("style.object.create", p, os.effects.as_ref().unwrap_or(&Effects::default()))? {
        os.effects = Some(e);
    }
    s.edit(|d, _| {
        let name = Styles::unique_name(|n| d.styles.object_style(n).is_some(), &base);
        os.name = name.clone();
        d.styles_mut().object.push(os.clone());
        Ok(json!({"name": name}))
    })
}

/// `p.stroke` merged over `base` (as [`group_param`]), refused when the weight isn't a number ≥ 0
/// or the tint isn't 0–1.
fn stroke_param(cmd: &str, p: &Value, base: Option<&designcraft_doc::Stroke>) -> Result<Option<Option<designcraft_doc::Stroke>>> {
    let st = group_param(cmd, p, "stroke", base.unwrap_or(&designcraft_doc::Stroke::default()))?;
    if let Some(Some(st)) = &st {
        if !(st.weight.is_finite() && st.weight >= 0.0) {
            return Err(bad(cmd, "stroke weight must be a number ≥ 0"));
        }
        if !(0.0..=1.0).contains(&st.tint) {
            return Err(bad(cmd, "stroke tint must be 0..1"));
        }
    }
    Ok(st)
}

/// `p[key]`, an attribute group of a style, merged field by field over `base`: `None` when not
/// given, `Some(None)` for null (the style no longer sets it).
fn group_param<T: serde::Serialize + serde::de::DeserializeOwned>(cmd: &str, p: &Value, key: &str, base: &T) -> Result<Option<Option<T>>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v @ Value::Object(_)) => {
            let mut cur = serde_json::to_value(base).map_err(|e| bad(cmd, e.to_string()))?;
            merge_json(&mut cur, v.clone(), 0);
            serde_json::from_value(cur).map(|t| Some(Some(t))).map_err(|e| bad(cmd, format!("{key}: {e}")))
        }
        Some(_) => Err(bad(cmd, format!("`{key}` must be an object or null"))),
    }
}

/// `fill`: a swatch name, `{swatch?, tint?}` over `base`, or null.
fn fill_param(cmd: &str, p: &Value, base: Option<&designcraft_doc::Fill>) -> Result<Option<Option<designcraft_doc::Fill>>> {
    if let Some(sw) = p.get("fill").and_then(Value::as_str) {
        return Ok(Some(Some(designcraft_doc::Fill::swatch(sw))));
    }
    let fill = group_param(cmd, p, "fill", base.unwrap_or(&designcraft_doc::Fill::default()))?;
    if let Some(Some(f)) = &fill
        && !(f.tint.is_finite() && (0.0..=1.0).contains(&f.tint))
    {
        return Err(bad(cmd, "fill tint must be 0–1"));
    }
    Ok(fill)
}

/// `corners`: `{shape?, size?}` for all four, `{corners: [{shape, size}] ×4}`, or null.
fn corners_param(
    cmd: &str,
    p: &Value,
    base: designcraft_geom::corners::CornerOptions,
) -> Result<Option<Option<designcraft_geom::corners::CornerOptions>>> {
    let Some(v) = p.get("corners") else { return Ok(None) };
    if v.is_null() {
        return Ok(Some(None));
    }
    let o = v.as_object().ok_or_else(|| bad(cmd, "`corners` must be an object or null"))?;
    let mut c = if o.contains_key("corners") {
        serde_json::from_value::<designcraft_geom::corners::CornerOptions>(v.clone()).map_err(|e| bad(cmd, format!("corners: {e}")))?
    } else {
        base
    };
    if let Some(sh) = o.get("shape") {
        let shape = serde_json::from_value(sh.clone()).map_err(|e| bad(cmd, format!("corner shape: {e}")))?;
        c.corners.iter_mut().for_each(|k| k.shape = shape);
    }
    if let Some(sz) = o.get("size") {
        let size = sz.as_f64().filter(|v| v.is_finite()).ok_or_else(|| bad(cmd, "corner size must be a number"))?;
        c.corners.iter_mut().for_each(|k| k.size = size);
    }
    for k in &mut c.corners {
        if !k.size.is_finite() {
            return Err(bad(cmd, "corner sizes must be finite"));
        }
        k.size = k.size.clamp(0.0, 10_000.0);
    }
    Ok(Some(Some(c)))
}

pub(super) fn edit_object(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "style.object.edit";
    let name = str_param(p, "name").ok_or_else(|| bad(CMD, "missing name"))?.to_string();
    if name == designcraft_doc::NO_OBJECT_STYLE {
        return Err(bad(CMD, format!("`{name}` can't be edited")));
    }
    let old = s.doc()?.doc.styles.object_style(&name).cloned().ok_or_else(|| bad(CMD, format!("no object style `{name}`")))?;
    let mut new = old.clone();
    if p.get("effects").is_some_and(Value::is_null) {
        if EFFECT_KEYS.iter().any(|k| p.get(k).is_some()) {
            return Err(bad(CMD, "`effects: null` can't be combined with effect settings"));
        }
        new.effects = None;
    } else if let Some(e) = effects_param(CMD, p, old.effects.as_ref().unwrap_or(&Effects::default()))? {
        new.effects = Some(e);
    }
    if let Some(f) = fill_param(CMD, p, old.fill.as_ref())? {
        new.fill = f;
    }
    if let Some(st) = stroke_param(CMD, p, old.stroke.as_ref())? {
        new.stroke = st;
    }
    if let Some(c) = corners_param(CMD, p, old.corners.unwrap_or_default())? {
        new.corners = c;
    }
    let rename = str_param(p, "rename").map(str::trim).filter(|r| *r != name).map(str::to_string);
    if let Some(r) = &rename {
        let builtin = name.starts_with('[') && name.ends_with(']');
        if builtin || r.is_empty() || (r.starts_with('[') && r.ends_with(']')) {
            return Err(bad(CMD, format!("`{name}` can't be renamed to `{r}`")));
        }
        if s.doc()?.doc.styles.object_style(r).is_some() {
            return Err(bad(CMD, format!("an object style is already named `{r}`")));
        }
        new.name = r.clone();
    }
    if new == old {
        return Ok(json!({"name": name, "updated": 0}));
    }
    s.edit(|d, _| {
        let Some(os) = d.styles_mut().object.iter_mut().find(|o| o.name == name) else {
            return Err(bad(CMD, format!("no object style `{name}`")));
        };
        *os = new.clone();
        if let Some(r) = &rename {
            let st = d.styles_mut();
            for o in &mut st.object {
                if o.based_on.as_deref() == Some(name.as_str()) {
                    o.based_on = Some(r.clone());
                }
            }
            for n in [&mut st.default_text_frame, &mut st.default_graphic_frame] {
                if *n == name {
                    *n = r.clone();
                }
            }
            d.update_items(|it| it.object_style == name, |it| it.object_style = r.clone());
        }
        // An item follows each attribute the style sets while it has the style's value. When the
        // style didn't set it before, items with the default follow (the default styles are on
        // most frames). An attribute the style no longer sets stays as it is on the items.
        let effects = (new.effects != old.effects).then_some(new.effects.clone()).flatten().map(|e| (old.effects.clone().unwrap_or_default(), e));
        let fill = (new.fill != old.fill).then_some(new.fill.clone()).flatten().map(|f| (old.fill.clone().unwrap_or_default(), f));
        let stroke = (new.stroke != old.stroke)
            .then_some(new.stroke.clone())
            .flatten()
            .map(|f| (old.stroke.clone().unwrap_or_else(designcraft_doc::Stroke::none), f));
        let corners = (new.corners != old.corners).then_some(new.corners).flatten().map(|c| (old.corners.unwrap_or_default(), c));
        let follows = |it: &Item| {
            effects.as_ref().is_some_and(|(o, _)| it.effects == *o)
                || fill.as_ref().is_some_and(|(o, _)| it.fill == *o)
                || stroke.as_ref().is_some_and(|(o, _)| it.stroke == *o)
                || corners.as_ref().is_some_and(|(o, _)| it.corners == o.named_to_path(&it.path))
        };
        let updated = d.update_items(
            |it| it.object_style == new.name && follows(it),
            |it| {
                if let Some((o, n)) = &effects
                    && it.effects == *o
                {
                    it.effects = n.clone();
                }
                if let Some((o, n)) = &fill
                    && it.fill == *o
                {
                    it.fill = n.clone();
                }
                if let Some((o, n)) = &stroke
                    && it.stroke == *o
                {
                    it.stroke = n.clone();
                }
                if let Some((o, n)) = &corners
                    && it.corners == o.named_to_path(&it.path)
                {
                    it.corners = n.named_to_path(&it.path);
                }
            },
        );
        Ok(json!({"name": new.name, "updated": updated}))
    })
}
