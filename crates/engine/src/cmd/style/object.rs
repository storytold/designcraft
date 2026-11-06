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
    // Each target with its style's attributes applied, kept only where that changes the item.
    let d = &s.doc()?.doc;
    let changed: Vec<Item> = ids
        .iter()
        .filter_map(|id| {
            let it = d.item(*id)?;
            let mut next = it.clone();
            d.styles.object_style(&it.object_style)?.apply_to(&mut next);
            (next != *it).then_some(next)
        })
        .collect();
    if changed.is_empty() {
        return Ok(json!({"changed": 0}));
    }
    s.edit(|d, _| {
        for next in &changed {
            if let Some(it) = d.item_mut(next.id) {
                *it = next.clone();
            }
        }
        Ok(json!({"changed": changed.len()}))
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
        os.text_frame = it.text_frame().map(|t| t.options.clone());
        os.effects = Some(it.effects.clone());
    }
    if let Some(sw) = str_param(p, "fill") {
        os.fill = Some(designcraft_doc::Fill::swatch(sw));
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

pub(super) fn edit_object(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "style.object.edit";
    let name = str_param(p, "name").ok_or_else(|| bad(CMD, "missing name"))?.to_string();
    if name == designcraft_doc::NO_OBJECT_STYLE {
        return Err(bad(CMD, format!("`{name}` can't be edited")));
    }
    let old = s.doc()?.doc.styles.object_style(&name).cloned().ok_or_else(|| bad(CMD, format!("no object style `{name}`")))?;
    let effects = if p.get("effects").is_some_and(Value::is_null) {
        if EFFECT_KEYS.iter().any(|k| p.get(k).is_some()) {
            return Err(bad(CMD, "`effects: null` can't be combined with effect settings"));
        }
        None
    } else {
        match effects_param(CMD, p, old.effects.as_ref().unwrap_or(&Effects::default()))? {
            Some(e) => Some(e),
            None => return Ok(json!({"name": name, "updated": 0})),
        }
    };
    if effects == old.effects {
        return Ok(json!({"name": name, "updated": 0}));
    }
    s.edit(|d, _| {
        let Some(os) = d.styles_mut().object.iter_mut().find(|o| o.name == name) else {
            return Err(bad(CMD, format!("no object style `{name}`")));
        };
        os.effects = effects.clone();
        // An item follows the style while its effects are the style's. When the style didn't set
        // effects before, only items without effects of their own follow (the default styles are
        // on most frames).
        let follows = old.effects.clone().unwrap_or_default();
        let updated = match &effects {
            Some(new) => d.update_items(|it| it.object_style == name && it.effects == follows, |it| it.effects = new.clone()),
            None => 0,
        };
        Ok(json!({"name": name, "updated": updated}))
    })
}
