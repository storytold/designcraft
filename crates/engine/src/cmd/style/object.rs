use super::{Effects, Item, ObjectStyle, Result, Session, Styles, Value, bad, json, str_param};

pub(super) fn apply_object(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.object.apply", "missing name"))?.to_string();
    let os = s.doc()?.doc.styles.object_style(&name).cloned().ok_or_else(|| bad("style.object.apply", format!("no object style `{name}`")))?;
    let ids = crate::cmd::targets(s, p)?;
    // Nothing to record when every target already has the style and its attributes, and its text
    // the style's paragraph styles.
    let d = &s.doc()?.doc;
    let unchanged = ids.iter().filter_map(|id| d.item(*id)).all(|it| {
        let mut next = it.clone();
        next.object_style = name.clone();
        os.apply_to(&mut next);
        next == *it && it.text_frame().and_then(|tf| d.story(tf.story)).is_none_or(|st| os.story_follows(&d.styles, st))
    });
    if unchanged {
        return Ok(Value::Null);
    }
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
        // The text takes the style's paragraph style (with Apply Next Style, a chain of them).
        let styles = d.styles.clone();
        for sid in stories {
            if let Some(st) = d.story_mut(sid) {
                os.restyle(&styles, st);
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
    let mut restyled: Vec<(designcraft_doc::StoryId, ObjectStyle)> = Vec::new();
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
        if let Some(tf) = it.text_frame()
            && d.story(tf.story).is_some_and(|st| !os.story_follows(&d.styles, st))
        {
            story_changed = true;
            if !restyled.iter().any(|(sid, _)| *sid == tf.story) {
                restyled.push((tf.story, os.clone()));
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
        let styles = d.styles.clone();
        for (sid, os) in &restyled {
            if let Some(st) = d.story_mut(*sid) {
                os.restyle(&styles, st);
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
    const CREATE: &str = "style.object.create";
    check_swatches(CREATE, s, p)?;
    let from_sel = bool_param(CREATE, p, "fromSelection")?.unwrap_or(true);
    let src = if from_sel { s.doc()?.selection.items.first().and_then(|i| s.doc().ok()?.doc.item(*i).cloned()) } else { None };
    let mut os = ObjectStyle::default();
    if let Some(it) = &src {
        os.fill = Some(it.fill.clone());
        os.stroke = Some(it.stroke.clone());
        os.corners = Some(it.corners.path_to_named(&it.path));
        os.text_frame = it.text_frame().map(|t| t.options.clone());
        os.effects = Some(it.effects.clone());
    }
    if let Some(f) = fill_param(CREATE, p, os.fill.as_ref())? {
        os.fill = f;
    }
    if let Some(st) = stroke_param(CREATE, s, p, os.stroke.as_ref(), false)? {
        os.stroke = st;
    }
    if let Some(c) = corners_param(CREATE, p, os.corners.unwrap_or_default())? {
        os.corners = c;
    }
    if let Some(ps) = paragraph_style_param(CREATE, s, p)? {
        os.paragraph_style = ps;
    }
    if let Some(next) = bool_param(CREATE, p, "applyNextStyle")? {
        os.apply_next_style = next;
    }
    if let Some(tf) = text_frame_param(CREATE, p, os.text_frame.as_ref())? {
        os.text_frame = tf;
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

/// The swatches this call gives (`fill` as a name or `fill.swatch`, `stroke.swatch` and
/// `stroke.gapSwatch`) must be swatches of the document.
fn check_swatches(cmd: &str, s: &Session, p: &Value) -> Result<()> {
    let doc = &s.doc()?.doc;
    let given = [
        ("fill", p.get("fill").and_then(|f| f.as_str().or_else(|| f.get("swatch").and_then(Value::as_str)))),
        ("stroke", p.get("stroke").and_then(|f| f.get("swatch")).and_then(Value::as_str)),
        ("stroke gap", p.get("stroke").and_then(|f| f.get("gapSwatch")).and_then(Value::as_str)),
    ];
    for (key, name) in given {
        if let Some(name) = name {
            crate::cmd::object::known_swatch(doc, name, cmd, key)?;
        }
    }
    Ok(())
}

/// `paragraphStyle`: a paragraph style's name, or null (the object style sets none).
fn paragraph_style_param(cmd: &str, s: &Session, p: &Value) -> Result<Option<Option<String>>> {
    match p.get("paragraphStyle") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(n)) if s.doc()?.doc.styles.para(n).is_some() => Ok(Some(Some(n.clone()))),
        Some(Value::String(n)) => Err(bad(cmd, format!("no paragraph style `{n}`"))),
        Some(_) => Err(bad(cmd, "`paragraphStyle` must be a paragraph style's name or null")),
    }
}

/// `textFrame` (a text frame's General options, as `document.inspect` shows them) merged over
/// `base`, or null; refused when it names another key, the columns aren't 1–40 or a gutter, width
/// or inset isn't 0–15552 pt. `inset` is `[top, left, bottom, right]` or one number for all four.
fn text_frame_param(
    cmd: &str,
    p: &Value,
    base: Option<&designcraft_doc::TextFrameOptions>,
) -> Result<Option<Option<designcraft_doc::TextFrameOptions>>> {
    let general = designcraft_doc::TextFrameOptions::GENERAL_KEYS;
    let mut p = p.clone();
    if let Some(Value::Object(o)) = p.get_mut("textFrame") {
        if let Some(k) = o.keys().find(|k| !general.contains(&k.as_str())) {
            return Err(bad(cmd, format!("textFrame.{k} is not a General option ({})", general.join(", "))));
        }
        if let Some(n) = o.get("inset").filter(|v| v.is_number()).cloned() {
            o.insert("inset".into(), json!([n, n, n, n]));
        }
    }
    let tf = group_param(cmd, &p, "textFrame", base.unwrap_or(&designcraft_doc::TextFrameOptions::default()), &[])?;
    if let Some(Some(o)) = &tf {
        // Only the values given in this call are checked.
        let given = |k: &str| p.get("textFrame").is_some_and(|t| t.get(k).is_some());
        if given("columns") && !(1..=40).contains(&o.columns) {
            return Err(bad(cmd, "text frame columns must be 1–40"));
        }
        let lengths = [("gutter", o.gutter), ("columnWidth", o.column_width), ("vjParagraphSpacingLimit", o.vj_paragraph_spacing_limit)];
        let mut lengths = lengths.into_iter().filter(|(k, _)| given(k)).map(|(_, v)| v).chain(o.inset.into_iter().filter(|_| given("inset")));
        if !lengths.all(|v| v.is_finite() && (0.0..=15552.0).contains(&v)) {
            return Err(bad(cmd, "a text frame's gutter, widths and insets must be 0–15552 pt"));
        }
    }
    Ok(tf)
}

/// A true/false parameter, when given.
fn bool_param(cmd: &str, p: &Value, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// `p.stroke` merged over `base` (as [`group_param`]), refused when the weight isn't a number ≥ 0,
/// the tint or gap tint isn't 0–1, the type has a field its kind doesn't take, or a type isn't one
/// `object.stroke` accepts. With `keep_kind` (`base` is the style's own stroke), a type equal to
/// the base's passes unchecked.
fn stroke_param(
    cmd: &str,
    s: &Session,
    p: &Value,
    base: Option<&designcraft_doc::Stroke>,
    keep_kind: bool,
) -> Result<Option<Option<designcraft_doc::Stroke>>> {
    let optional = ["gapOverprint", "gradientAngle", "gradientVector"];
    let base = base.cloned().unwrap_or_default();
    // A type without `kind` keeps the style's kind, so it takes that kind's fields.
    if let Some(t) = p.get("stroke").and_then(|s| s.get("type")) {
        let base_kind = serde_json::to_value(&base.kind).ok().and_then(|k| k.get("kind").and_then(Value::as_str).map(str::to_string));
        crate::cmd::object::check_stroke_type_fields(t, base_kind.as_deref(), cmd)?;
    }
    let st = group_param(cmd, p, "stroke", &base, &optional)?;
    if let Some(Some(st)) = &st {
        if !(st.weight.is_finite() && st.weight >= 0.0) {
            return Err(bad(cmd, "stroke weight must be a number ≥ 0"));
        }
        if !(0.0..=1.0).contains(&st.tint) {
            return Err(bad(cmd, "stroke tint must be 0..1"));
        }
        if !(0.0..=1.0).contains(&st.gap_tint) {
            return Err(bad(cmd, "stroke gap tint must be 0..1"));
        }
        // A type the style already has passes, even one naming a stroke style deleted since. A type
        // given when creating a style is checked, even when the selected object already has it.
        let type_given = p.get("stroke").and_then(|s| s.get("type")).is_some();
        if st.kind != base.kind || (type_given && !keep_kind) {
            crate::cmd::object::check_stroke_kind(&s.doc()?.doc, &st.kind, cmd)?;
        }
    }
    Ok(st)
}

/// `p[key]`, an attribute group of a style, merged field by field over `base`: `None` when not
/// given, `Some(None)` for null (the style no longer sets it). A field `base` doesn't write and
/// `optional` doesn't name is refused.
fn group_param<T: serde::Serialize + serde::de::DeserializeOwned>(
    cmd: &str,
    p: &Value,
    key: &str,
    base: &T,
    optional: &[&str],
) -> Result<Option<Option<T>>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v @ Value::Object(given)) => {
            let mut cur = serde_json::to_value(base).map_err(|e| bad(cmd, e.to_string()))?;
            if let Some(k) = given.keys().find(|k| cur.get(k.as_str()).is_none() && !optional.contains(&k.as_str())) {
                return Err(bad(cmd, format!("{key}.{k} is not a {key} setting")));
            }
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
    let fill = group_param(cmd, p, "fill", base.unwrap_or(&designcraft_doc::Fill::default()), &["gradientAngle", "gradientVector"])?;
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
    if let Some(k) = o.keys().find(|k| !["shape", "size", "corners"].contains(&k.as_str())) {
        return Err(bad(cmd, format!("corners.{k} is not a corner setting (shape, size, corners)")));
    }
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
    // Sizes given in this call must be 0–10000 pt.
    if (o.contains_key("corners") || o.contains_key("size")) && !c.corners.iter().all(|k| k.size.is_finite() && (0.0..=10_000.0).contains(&k.size)) {
        return Err(bad(cmd, "corner sizes must be 0–10000 pt"));
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
    check_swatches(CMD, s, p)?;
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
    if let Some(st) = stroke_param(CMD, s, p, old.stroke.as_ref(), true)? {
        new.stroke = st;
    }
    if let Some(c) = corners_param(CMD, p, old.corners.unwrap_or_default())? {
        new.corners = c;
    }
    if let Some(ps) = paragraph_style_param(CMD, s, p)? {
        new.paragraph_style = ps;
    }
    if let Some(next) = bool_param(CMD, p, "applyNextStyle")? {
        new.apply_next_style = next;
    }
    if let Some(tf) = text_frame_param(CMD, p, old.text_frame.as_ref())? {
        new.text_frame = tf;
    }
    if p.get("rename").is_some_and(|r| !r.is_string()) {
        return Err(bad(CMD, "`rename` must be a style name"));
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
        return Ok(json!({"name": name, "updated": 0, "restyled": 0}));
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
            d.update_placed_items(|it| it.object_style == name, |it| it.object_style = r.clone());
        }
        // An item follows each attribute the style sets while it has the style's value. When the
        // style didn't set it before, items with the default follow (the default styles are on
        // most frames). An attribute the style no longer sets stays as it is on the items.
        let effects = (new.effects != old.effects).then_some(new.effects.clone()).flatten().map(|e| (old.effects.clone().unwrap_or_default(), e));
        let fill = (new.fill != old.fill).then_some(new.fill.clone()).flatten().map(|f| (old.fill.clone().unwrap_or_default(), f));
        let stroke = (new.stroke != old.stroke).then_some(new.stroke.clone()).flatten().map(|f| (old.stroke.clone(), f));
        // With no stroke in the style before, an item with no stroke (a frame's default) or a
        // 1 pt black one (a drawn shape's default) follows.
        let stroke_follows = |it: &Item, old: &Option<designcraft_doc::Stroke>| match old {
            Some(o) => it.stroke == *o,
            None => {
                it.stroke == designcraft_doc::Stroke::none()
                    || (matches!(it.content, designcraft_doc::Content::Unassigned) && it.stroke == designcraft_doc::Stroke::default())
            }
        };
        let corners = (new.corners != old.corners).then_some(new.corners).flatten().map(|c| (old.corners.unwrap_or_default(), c));
        let frame =
            (new.text_frame != old.text_frame).then_some(new.text_frame.clone()).flatten().map(|t| (old.text_frame.clone().unwrap_or_default(), t));
        let follows = |it: &Item| {
            effects.as_ref().is_some_and(|(o, _)| it.effects == *o)
                || fill.as_ref().is_some_and(|(o, _)| it.fill == *o)
                || stroke.as_ref().is_some_and(|(o, _)| stroke_follows(it, o))
                || corners.as_ref().is_some_and(|(o, _)| it.corners == o.named_to_path(&it.path))
                || frame.as_ref().is_some_and(|(o, n)| it.text_frame().is_some_and(|t| t.options.clone().follow_general(o, n)))
        };
        let updated = d.update_placed_items(
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
                    && stroke_follows(it, o)
                {
                    it.stroke = n.clone();
                }
                if let Some((o, n)) = &corners
                    && it.corners == o.named_to_path(&it.path)
                {
                    it.corners = n.named_to_path(&it.path);
                }
                // Each General option follows on its own, so a frame's own inset stays while a
                // new column count reaches it.
                if let Some((o, n)) = &frame
                    && let Some(tf) = it.text_frame_mut()
                {
                    tf.options.follow_general(o, n);
                }
            },
        );
        // Each paragraph of the frames using the style takes the new paragraph style (or chain)
        // while it is in the old one's; when the style set none before, every paragraph does.
        let mut restyled = 0;
        if new.paragraph_style.is_some() && (new.paragraph_style != old.paragraph_style || new.apply_next_style != old.apply_next_style) {
            // Frames on spreads and parents, in groups, and anchored in stories, table cells and
            // notes (as IDML import places them).
            let mut stories: Vec<_> =
                d.placed_items().into_iter().filter(|i| i.object_style == new.name).filter_map(|i| i.text_frame().map(|tf| tf.story)).collect();
            stories.sort_unstable();
            stories.dedup();
            let styles = d.styles.clone();
            for sid in stories {
                let changes = d.story(sid).map(|st| new.restyle_from(&old, &styles, st)).unwrap_or_default();
                if !changes.is_empty()
                    && let Some(st) = d.story_mut(sid)
                {
                    for (i, w) in changes {
                        if let Some(f) = st.paras.get_mut(i) {
                            f.style = w;
                        }
                    }
                    st.rev = st.rev.wrapping_add(1);
                    restyled += 1;
                }
            }
        }
        Ok(json!({"name": new.name, "updated": updated, "restyled": restyled}))
    })
}
