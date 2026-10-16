//! Paragraph / character styles and swatches.

use designcraft_color::{Swatch, SwatchValue};
use designcraft_doc::{CharAttrs, CharacterStyle, ParaAttrs, ParagraphStyle, Styles};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, has_text_or_frames, ok, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "style.compositeFont.list", "List Composite Fonts", [], None, "{} → composite font definitions", has_doc, |s, _| {
            serde_json::to_value(&s.doc()?.doc.styles.composite_fonts).map_err(|e| bad("style.compositeFont.list", e.to_string()))
        }),
        cmd!(
            "style.compositeFont.set",
            "Set Composite Font",
            [],
            None,
            "{name, entries: [{name?, characters?, family, style?, relativeSize?, horizontalScale?, verticalScale?, baselineShift?, scaleOption?}]} — replaces a composite font definition; empty characters selects the base entry",
            has_doc,
            |s, p| {
                let f: designcraft_doc::cjk::CompositeFont =
                    serde_json::from_value(p.clone()).map_err(|e| bad("style.compositeFont.set", e.to_string()))?;
                if f.name.trim().is_empty()
                    || f.entries.is_empty()
                    || f.entries.len() > 1024
                    || !f.entries.iter().any(|e| e.characters.is_empty())
                    || f.entries.iter().any(|e| {
                        e.family.trim().is_empty()
                            || ![e.relative_size, e.horizontal_scale, e.vertical_scale].iter().all(|v| v.is_finite() && *v > 0.0 && *v <= 100.0)
                            || !e.baseline_shift.is_finite()
                            || e.baseline_shift.abs() > 100.0
                    })
                {
                    return Err(bad("style.compositeFont.set", "invalid name, base entry, font, scale or baseline shift"));
                }
                s.edit(|d, _| {
                    let fonts = &mut d.styles_mut().composite_fonts;
                    if let Some(old) = fonts.iter_mut().find(|old| old.name == f.name) {
                        *old = f.clone();
                    } else {
                        fonts.push(f.clone());
                    }
                    Ok(json!({"name": f.name}))
                })
            }
        ),
        cmd!(
            "style.exportTag",
            "Export Tagging",
            [],
            None,
            "{style, character?: bool, tag?: p|h1…h6|blockquote|pre|li|… (span|em|strong|code|sup|sub… for characters; \"\" = automatic), class?} → the tagging",
            has_doc,
            |s, p| {
                let name = str_param(p, "style").ok_or_else(|| bad("style.exportTag", "`style` required"))?.to_string();
                let character = p.get("character").and_then(Value::as_bool).unwrap_or(false);
                let exists = if character { s.doc()?.doc.styles.char_style(&name).is_some() } else { s.doc()?.doc.styles.para(&name).is_some() };
                if !exists {
                    return Err(bad("style.exportTag", format!("no style `{name}`")));
                }
                let key = format!("{}:{name}", if character { "c" } else { "p" });
                let (tag, class) = (str_param(p, "tag").map(str::to_string), str_param(p, "class").map(str::to_string));
                s.edit(|d, _| {
                    let st = d.styles_mut();
                    let e = st.export_tags.entry(key.clone()).or_default();
                    if let Some(t) = tag.clone() {
                        e.tag = t;
                    }
                    if let Some(c) = class.clone() {
                        e.class = c;
                    }
                    let out = serde_json::to_value(&*e).unwrap_or_default();
                    if e.tag.is_empty() && e.class.is_empty() {
                        st.export_tags.remove(&key);
                    }
                    Ok(out)
                })
            }
        ),
        cmd!("style.paragraph.apply", "Apply Paragraph Style", [], None, "{name, clearOverrides?: bool}", has_text_or_frames, apply_para),
        cmd!("style.character.apply", "Apply Character Style", [], None, "{name}", has_text_or_frames, apply_char),
        cmd!(
            "style.paragraph.create",
            "New Paragraph Style…",
            [],
            None,
            "{name, basedOn?, nextStyle?, para?: {paragraph attributes, as type.para names them: align, leftIndent, firstLineIndent, spaceBefore, spaceAfter, …}, chars?: {character attributes, as type.char names them: fontFamily, fontStyle, size, leading, tracking, fill, …}, fromSelection?: bool}",
            has_doc,
            create_para
        ),
        cmd!(
            "style.paragraph.edit",
            "Paragraph Style Options…",
            [],
            None,
            "{name, rename?, basedOn?, nextStyle?, para?: {paragraph attributes, as type.para names them: align, leftIndent, spaceBefore, …; ruleAbove?/ruleBelow?: only the named rule fields change}, chars?: {character attributes, as type.char names them: fontFamily, fontStyle, size, leading, tracking, fill, …}}",
            has_doc,
            edit_para
        ),
        cmd!("style.paragraph.delete", "Delete Paragraph Style", [], None, "{name, replaceWith?}", has_doc, delete_para),
        cmd!(
            "style.character.create",
            "New Character Style…",
            [],
            None,
            "{name, basedOn?: style | \"[None]\", chars?: {character attributes, as type.char names them: fontFamily, fontStyle, size, leading, tracking, fill, …}, fromSelection?: bool (default false)} — if true, capture the first selected character's style/local overrides (caret: typing format), then overlay chars (null unsets); only explicit fill names are validated; paragraph formatting stays inherited; no text target: chars only → {name} (made unique)",
            has_doc,
            create_char
        ),
        cmd!(
            "style.character.edit",
            "Character Style Options…",
            [],
            None,
            "{name, rename?, basedOn?: style | \"[None]\" | null, chars?: {character attribute (as type.char names them: fontFamily, fontStyle, size, …): value | null (no longer set)}} — renaming updates every use",
            has_doc,
            edit_char
        ),
        cmd!("style.character.delete", "Delete Character Style", [], None, "{name, replaceWith?}", has_doc, delete_char),
        cmd!(query "style.list", "List Styles", [], None, "{} → paragraph and character style names", has_doc, |s, _| {
            let st = s.doc()?;
            Ok(json!({
                "paragraph": st.doc.styles.paragraph.iter().map(|p| &p.name).collect::<Vec<_>>(),
                "character": st.doc.styles.character.iter().map(|p| &p.name).collect::<Vec<_>>(),
                "object": st.doc.styles.object.iter().map(|p| &p.name).collect::<Vec<_>>(),
                "cell": st.doc.styles.cell.iter().map(|p| &p.name).collect::<Vec<_>>(),
                "table": st.doc.styles.table.iter().map(|p| &p.name).collect::<Vec<_>>(),
            }))
        }),
        cmd!("style.object.apply", "Apply Object Style", [], None, "{name, ids?}", super::has_selection, apply_object),
        cmd!("style.object.create", "New Object Style…", [], None, "{name, fromSelection?: true, fill?, paragraphStyle?}", has_doc, create_object),
        cmd!(
            "swatch.create",
            "New Color Swatch…",
            [],
            None,
            "{name?, color: \"#rrggbb\"|{c,m,y,k}(0..100)|[r,g,b]|{model: cmyk|rgb|gray, …}(0..1), spot?: bool}",
            has_doc,
            create_swatch
        ),
        cmd!(
            "object.gradient",
            "Gradient",
            [],
            None,
            "{kind?: linear|radial, stops?: [{location: 0–100, color: \"#rrggbb\"|{c,m,y,k}(0..100)|[r,g,b]|{model, …}(0..1), opacity?: 0–100, midpoint?: 13–87}], reverse?: bool, angle?: degrees, from?: [x,y], to?: [x,y] (spread coords: the Gradient Swatch tool's drag), target?: fill|stroke, ids?} → {swatch, kind, stops} — edits the fill's (or the stroke's) gradient (an unnamed gradient unless it equals a gradient swatch)",
            super::has_selection,
            apply_gradient
        ),
        cmd!(
            "style.group",
            "Move to Group",
            [],
            None,
            "{kind: paragraph|character, names: [style names], group: \"Heads\" | \"Heads/Display\" | \"\" (out of any group)} — styles are named Group/Name; every use is renamed → {renamed: {old: new}}",
            has_doc,
            group_styles
        ),
        cmd!(
            "swatch.load",
            "Load Swatches…",
            [],
            None,
            "{path?|base64?, replace?: bool} — colour swatches from a swatch exchange (.ase) file; names already in the document are kept unless `replace` → {added, replaced}",
            has_doc,
            load_swatches
        ),
        cmd!(
            noundo "swatch.save",
            "Save Swatches for Exchange…",
            [],
            None,
            "{path?, names?: [swatch names] (default: all colour swatches)} → {base64} or writes `path` (.ase)",
            has_doc,
            save_swatches
        ),
        cmd!(
            "object.color",
            "Apply Color",
            [],
            None,
            "{color: \"#rrggbb\"|{c,m,y,k}(0..100)|[r,g,b](0..255)|{model: cmyk|rgb|gray, …}(0..1), target?: fill|stroke, text?: bool, ids?} — an unnamed colour (not in the Swatches panel until Add to Swatches) on the selection; `text`: on the selected text, or the text of the selected frames (Formatting Affects Text)",
            has_text_or_selection,
            apply_color
        ),
        cmd!(
            "color.sample",
            "Sample Color (Eyedropper)",
            [],
            None,
            "{at: [x,y] (spread coords), spread?, target?: fill|stroke, ids?} → {color: \"#rrggbb\", swatch, from: image id} — the Eyedropper on a placed image: the image's colour at the point (an unnamed colour) fills the selected text, else the selected objects",
            has_text_or_selection,
            sample_color
        ),
        cmd!(
            "swatch.addToSwatches",
            "Add to Swatches",
            [],
            None,
            "{swatch?: name (default: the selection's fill), target?: fill|stroke, name?: new name} — makes an unnamed colour a swatch",
            has_doc,
            add_to_swatches
        ),
        cmd!("swatch.addUnnamed", "Add Unnamed Colors", [], None, "{} — every unnamed colour used becomes a swatch", has_doc, |s, _| {
            s.edit(|d, _| {
                let mut n = 0;
                for w in &mut d.swatches {
                    if w.hidden {
                        w.hidden = false;
                        n += 1;
                    }
                }
                Ok(json!({"added": n}))
            })
        }),
        cmd!("swatch.delete", "Delete Swatch", [], None, "{name}", has_doc, |s, p| {
            let name = str_param(p, "name").unwrap_or("").to_string();
            s.edit(|d, _| {
                if d.swatch(&name).is_some_and(|w| w.locked) {
                    return Err(bad("swatch.delete", "special swatches can't be deleted"));
                }
                d.swatches.retain(|w| w.name != name);
                for g in &mut d.color_groups {
                    g.swatches.retain(|w| *w != name);
                }
                ok()
            })
        }),
        cmd!(query "ink.list", "Ink Manager", [], None, "{} → {allToProcess, inks: [{name, process: bool (printed as process), alias?}]} — the spot inks", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            let inks: Vec<Value> = d
                .swatches
                .iter()
                .filter(|w| matches!(w.value, designcraft_color::swatch::SwatchValue::Color { color_type: designcraft_color::swatch::ColorType::Spot, .. }))
                .map(|w| {
                    let alias = d.inks.aliases.iter().find(|(a, _)| *a == w.name).map(|(_, t)| t.clone());
                    json!({"name": w.name, "process": d.inks.resolve(&w.name).1, "alias": alias})
                })
                .collect();
            Ok(json!({"allToProcess": d.inks.all_to_process, "inks": inks}))
        }),
        cmd!(
            "ink.options",
            "Ink Manager",
            [],
            None,
            "{allToProcess?: bool, ink?: spot swatch, toProcess?: bool, alias?: spot swatch | null} — output: spots to process, ink aliases",
            has_doc,
            |s, p| {
                let all = p.get("allToProcess").and_then(Value::as_bool);
                let ink = str_param(p, "ink").map(str::to_string);
                let to_process = p.get("toProcess").and_then(Value::as_bool);
                let alias = p.get("alias").cloned();
                s.edit(|d, _| {
                    if let Some(v) = all {
                        d.inks.all_to_process = v;
                    }
                    if let Some(name) = &ink {
                        let is_spot = |n: &str| {
                            d.swatch(n).is_some_and(|w| {
                                matches!(
                                    w.value,
                                    designcraft_color::swatch::SwatchValue::Color { color_type: designcraft_color::swatch::ColorType::Spot, .. }
                                )
                            })
                        };
                        if !is_spot(name) {
                            return Err(bad("ink.options", format!("`{name}` isn't a spot ink")));
                        }
                        if let Some(Value::String(to)) = &alias
                            && (!is_spot(to) || to == name)
                        {
                            return Err(bad("ink.options", format!("can't alias to `{to}`")));
                        }
                        if let Some(v) = to_process {
                            d.inks.to_process.retain(|x| x != name);
                            if v {
                                d.inks.to_process.push(name.clone());
                            }
                        }
                        match alias {
                            Some(Value::String(to)) => {
                                d.inks.aliases.retain(|(a, _)| a != name);
                                d.inks.aliases.push((name.clone(), to));
                            }
                            Some(Value::Null) => d.inks.aliases.retain(|(a, _)| a != name),
                            _ => {}
                        }
                    }
                    ok()
                })
            }
        ),
        cmd!(
            "swatch.newColorGroup",
            "New Color Group",
            [],
            None,
            "{name?, swatches?: [names]} — a Swatches panel folder (the swatches move into it) → {name}",
            has_doc,
            |s, p| {
                let names: Vec<String> = p
                    .get("swatches")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                let want = str_param(p, "name").map(str::to_string);
                s.edit(|d, _| {
                    for n in &names {
                        if d.swatch(n).is_none_or(|w| w.locked) {
                            return Err(bad("swatch.newColorGroup", format!("`{n}` can't go in a group")));
                        }
                    }
                    let taken = |n: &str| d.color_groups.iter().any(|g| g.name == n);
                    let name = match want {
                        Some(n) if taken(&n) => return Err(bad("swatch.newColorGroup", format!("a group named `{n}` exists"))),
                        Some(n) => n,
                        None => (1..=d.color_groups.len() + 1)
                            .map(|i| format!("Color Group {i}"))
                            .find(|n| !taken(n))
                            .ok_or_else(|| bad("swatch.newColorGroup", "no free group name"))?,
                    };
                    for g in &mut d.color_groups {
                        g.swatches.retain(|w| !names.contains(w));
                    }
                    d.color_groups.push(designcraft_doc::ColorGroup { name: name.clone(), swatches: names });
                    Ok(json!({"name": name}))
                })
            }
        ),
        cmd!("swatch.moveToGroup", "Move to Color Group", [], None, "{swatches: [names], group: name | null (top level)}", has_doc, |s, p| {
            let names: Vec<String> = p
                .get("swatches")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let group = str_param(p, "group").map(str::to_string);
            s.edit(|d, _| {
                if let Some(g) = &group
                    && !d.color_groups.iter().any(|x| x.name == *g)
                {
                    return Err(bad("swatch.moveToGroup", format!("no color group `{g}`")));
                }
                for g in &mut d.color_groups {
                    g.swatches.retain(|w| !names.contains(w));
                }
                if let Some(g) = group.and_then(|g| d.color_groups.iter_mut().find(|x| x.name == g)) {
                    g.swatches.extend(names.iter().filter(|n| d.swatches.iter().any(|w| w.name == **n && !w.locked)).cloned());
                }
                ok()
            })
        }),
        cmd!("swatch.ungroupColorGroup", "Ungroup Color Group", [], None, "{name} — its swatches go back to the top level", has_doc, |s, p| {
            let name = str_param(p, "name").unwrap_or("").to_string();
            s.edit(|d, _| {
                let n = d.color_groups.len();
                d.color_groups.retain(|g| g.name != name);
                if d.color_groups.len() == n {
                    return Err(bad("swatch.ungroupColorGroup", format!("no color group `{name}`")));
                }
                ok()
            })
        }),
        cmd!("swatch.renameColorGroup", "Color Group Options", [], None, "{name, to}", has_doc, |s, p| {
            let (name, to) = (str_param(p, "name").unwrap_or("").to_string(), str_param(p, "to").unwrap_or("").trim().to_string());
            s.edit(|d, _| {
                if to.is_empty() || d.color_groups.iter().any(|g| g.name == to) {
                    return Err(bad("swatch.renameColorGroup", format!("can't rename to `{to}`")));
                }
                let g = d
                    .color_groups
                    .iter_mut()
                    .find(|g| g.name == name)
                    .ok_or_else(|| bad("swatch.renameColorGroup", format!("no color group `{name}`")))?;
                g.name = to;
                ok()
            })
        }),
    ]
}

/// The `group` (`chars`, `para`) object of command `cmd`'s params as attributes; a bad attribute
/// is that command's parameter error, naming the group.
fn attrs<T: Default>(cmd: &str, group: &str, p: &Value, set: impl Fn(&mut T, &str, &Value) -> std::result::Result<(), String>) -> Result<T> {
    let mut a = T::default();
    match p.get(group) {
        None | Some(Value::Null) => {}
        Some(Value::Object(o)) => {
            for (k, v) in o {
                set(&mut a, k, v).map_err(|e| bad(cmd, format!("{group}: {e}")))?;
            }
        }
        Some(_) => return Err(bad(cmd, format!("{group} must be an object of attributes"))),
    }
    Ok(a)
}

fn apply_para(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.paragraph.apply", "missing name"))?.to_string();
    let clear = p.get("clearOverrides").and_then(Value::as_bool).unwrap_or(false);
    if s.doc()?.doc.styles.para(&name).is_none() {
        return Err(bad("style.paragraph.apply", format!("no paragraph style `{name}`")));
    }
    let targets = super::text::format_targets_pub(s);
    s.edit(|d, _| {
        for t in &targets {
            let r = &t.range;
            if let Some(st) = d.text_story_mut(t.story, t.cell) {
                st.format_paras(r.clone(), |f| {
                    f.style = name.clone();
                    if clear {
                        f.para = ParaAttrs::default();
                        f.chars = CharAttrs::default();
                    }
                });
                if clear {
                    let (a, b) = (st.para_ranges()[st.para_at(r.start)].start, st.para_ranges()[st.para_at(r.end)].end);
                    st.format_chars(a..b, |f| f.over = CharAttrs::default());
                }
            }
        }
        ok()
    })
}

fn apply_char(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.character.apply", "missing name"))?.to_string();
    if s.doc()?.doc.styles.char_style(&name).is_none() {
        return Err(bad("style.character.apply", format!("no character style `{name}`")));
    }
    let targets = super::text::format_targets_pub(s);
    s.edit(|d, _| {
        for t in &targets {
            if let Some(st) = d.text_story_mut(t.story, t.cell) {
                st.format_chars(t.range.clone(), |f| f.style = name.clone());
            }
        }
        ok()
    })
}

fn create_para(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Paragraph Style 1").to_string();
    let mut para: ParaAttrs = attrs("style.paragraph.create", "para", p, |a: &mut ParaAttrs, k, v| a.set_json(k, v))?;
    let mut chars: CharAttrs = attrs("style.paragraph.create", "chars", p, |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    // Validate caller-supplied fills, not selection fills retained after a swatch is deleted.
    super::text::validate_char_fill(&s.doc()?.doc, &chars, "style.paragraph.create")?;
    if p.get("fromSelection").and_then(Value::as_bool).unwrap_or(false)
        && let Ok(cur) = s.execute("type.selectionAttrs", &json!({}))
        && !cur.is_null()
    {
        if let Ok(pp) = serde_json::from_value::<designcraft_doc::ParaProps>(cur["para"].clone()) {
            let mut full = ParaProps_to_attrs(&pp);
            full.merge(&para);
            para = full;
        }
        if let Ok(cp) = serde_json::from_value::<designcraft_doc::CharProps>(cur["chars"].clone()) {
            let mut full = CharProps_to_attrs(&cp);
            full.merge(&chars);
            chars = full;
        }
    }
    let based_on = str_param(p, "basedOn").map(str::to_string);
    let next = str_param(p, "nextStyle").map(str::to_string);
    s.edit(|d, _| {
        let name = Styles::unique_name(|n| d.styles.para(n).is_some(), &base);
        if let Some(b) = &based_on
            && d.styles.para(b).is_none()
        {
            return Err(bad("style.paragraph.create", format!("no style `{b}`")));
        }
        d.styles_mut().paragraph.push(ParagraphStyle { name: name.clone(), based_on, next_style: next, para, chars, shortcut: String::new() });
        Ok(json!({"name": name}))
    })
}

#[allow(non_snake_case)]
fn ParaProps_to_attrs(p: &designcraft_doc::ParaProps) -> ParaAttrs {
    designcraft_doc::ParaProps::common([p])
}
#[allow(non_snake_case)]
fn CharProps_to_attrs(p: &designcraft_doc::CharProps) -> CharAttrs {
    designcraft_doc::CharProps::common([p])
}

fn edit_para(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.paragraph.edit", "missing name"))?.to_string();
    let chars: CharAttrs = attrs("style.paragraph.edit", "chars", p, |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    super::text::validate_char_fill(&s.doc()?.doc, &chars, "style.paragraph.edit")?;
    let rename = str_param(p, "rename").map(str::to_string);
    let based = p.get("basedOn").cloned();
    s.edit(|d, _| {
        if let Some(Value::String(b)) = &based
            && d.styles.para_based_on_cycles(&name, b)
        {
            return Err(bad("style.paragraph.edit", "based-on would create a cycle"));
        }
        // A rule object changes only the rule fields it names, over the style's resolved rule.
        let (current, _) = d.styles.resolve_para_style(&name);
        let para: ParaAttrs = attrs("style.paragraph.edit", "para", p, |a: &mut ParaAttrs, k, v| a.set_json_over(k, v, &current))?;
        let st = d.styles_mut().para_mut(&name).ok_or_else(|| bad("style.paragraph.edit", format!("no style `{name}`")))?;
        st.para.merge(&para);
        st.chars.merge(&chars);
        match based {
            Some(Value::String(b)) => st.based_on = Some(b),
            Some(Value::Null) => st.based_on = None,
            _ => {}
        }
        if let Some(n) = rename.clone() {
            // Every use: other styles, stories, table cells, footnotes, object styles.
            rename_style(d, true, &name, &n);
        }
        ok()
    })
}

/// Rename a paragraph or character style everywhere it's used.
fn rename_style(d: &mut designcraft_doc::Document, para: bool, from: &str, to: &str) {
    if para {
        for s in &mut d.styles_mut().paragraph {
            if s.name == from {
                s.name = to.to_string();
            }
        }
    } else {
        for s in &mut d.styles_mut().character {
            if s.name == from {
                s.name = to.to_string();
            }
        }
    }
    redirect_style_refs(d, para, from, to);
}

/// Point every reference to style `from` at `to`, except the style's own registry entry (its
/// `name` field) — shared by renaming (which also updates the entry's name) and deletion (which
/// instead removes the entry, after its former uses point at the replacement).
fn redirect_style_refs(d: &mut designcraft_doc::Document, para: bool, from: &str, to: &str) {
    let st = d.styles_mut();
    // A style is never based on itself: when `to` was based on `from`, it takes `from`'s parent.
    let parent_of = |based_on: Option<&String>| based_on.filter(|b| b.as_str() != to).cloned();
    if para {
        let from_parent = parent_of(st.paragraph.iter().find(|x| x.name == from).and_then(|x| x.based_on.as_ref()));
        for s in &mut st.paragraph {
            if s.based_on.as_deref() == Some(from) {
                s.based_on = if s.name == to { from_parent.clone() } else { Some(to.to_string()) };
            }
            if s.next_style.as_deref() == Some(from) {
                s.next_style = Some(to.to_string());
            }
        }
        for o in &mut st.object {
            if o.paragraph_style.as_deref() == Some(from) {
                o.paragraph_style = Some(to.to_string());
            }
        }
    } else {
        let from_parent = parent_of(st.character.iter().find(|x| x.name == from).and_then(|x| x.based_on.as_ref()));
        for s in &mut st.character {
            if s.based_on.as_deref() == Some(from) {
                s.based_on = if s.name == to { from_parent.clone() } else { Some(to.to_string()) };
            }
        }
        for s in &mut st.paragraph {
            rename_char_refs(&mut s.para, from, to);
        }
        if d.footnote_options.ref_char_style == from {
            d.footnote_options.ref_char_style = to.to_string();
        }
    }
    for sid in d.stories.keys().copied().collect::<Vec<_>>() {
        let Some(story) = d.story_mut(sid) else { continue };
        story.for_each_text_mut(&mut |st| {
            if para {
                for f in st.paras.iter_mut().filter(|f| f.style == from) {
                    f.style = to.to_string();
                }
            } else {
                for r in st.chars.iter_mut().filter(|r| r.format.style == from) {
                    r.format.style = to.to_string();
                }
                for f in &mut st.paras {
                    rename_char_refs(&mut f.para, from, to);
                }
            }
            st.rev += 1;
        });
    }
}

/// Point the nested, nested line and GREP styles, and the drop cap style, of paragraph
/// attributes at a renamed (or deleted, pointed at its replacement) character style.
fn rename_char_refs(p: &mut ParaAttrs, from: &str, to: &str) {
    let names = p.nested_styles.iter_mut().flatten().map(|n| &mut n.style);
    let names = names.chain(p.nested_line_styles.iter_mut().flatten().map(|n| &mut n.style));
    for n in names.chain(p.grep_styles.iter_mut().flatten().map(|g| &mut g.style)) {
        if n == from {
            *n = to.to_string();
        }
    }
    if p.drop_cap_style.as_deref() == Some(from) {
        p.drop_cap_style = Some(to.to_string());
    }
}

fn group_styles(s: &mut Session, p: &Value) -> Result<Value> {
    let para = match str_param(p, "kind").unwrap_or("paragraph") {
        "paragraph" => true,
        "character" => false,
        k => return Err(bad("style.group", format!("unknown kind `{k}`"))),
    };
    let names: Vec<String> =
        p.get("names").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let group = str_param(p, "group").unwrap_or("").trim_matches('/').to_string();
    s.edit(|d, _| {
        let mut renamed = serde_json::Map::new();
        for n in &names {
            if n.starts_with('[') {
                return Err(bad("style.group", format!("`{n}` is built in")));
            }
            let exists = if para { d.styles.para(n).is_some() } else { d.styles.character.iter().any(|c| c.name == *n) };
            if !exists {
                return Err(bad("style.group", format!("no style `{n}`")));
            }
            let base = n.rsplit('/').next().unwrap_or(n);
            let mut to = if group.is_empty() { base.to_string() } else { format!("{group}/{base}") };
            let taken = |d: &designcraft_doc::Document, x: &str| {
                if para { d.styles.para(x).is_some() } else { d.styles.character.iter().any(|c| c.name == x) }
            };
            if to != *n {
                to = Styles::unique_name(|x| taken(d, x), &to);
                rename_style(d, para, n, &to);
                renamed.insert(n.clone(), json!(to));
            }
        }
        Ok(json!({"renamed": renamed}))
    })
}

fn delete_para(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").filter(|n| !n.is_empty()).ok_or_else(|| bad("style.paragraph.delete", "missing name"))?.to_string();
    let repl = str_param(p, "replaceWith").unwrap_or(designcraft_doc::BASIC_PARAGRAPH).to_string();
    if name.starts_with('[') {
        return Err(bad("style.paragraph.delete", "built-in styles can't be deleted"));
    }
    if repl == name {
        return Err(bad("style.paragraph.delete", "`replaceWith` can't be the deleted style"));
    }
    s.edit(|d, _| {
        if d.styles.para(&name).is_none() {
            return Err(bad("style.paragraph.delete", format!("no paragraph style `{name}`")));
        }
        if d.styles.para(&repl).is_none() {
            return Err(bad("style.paragraph.delete", format!("no paragraph style `{repl}`")));
        }
        redirect_style_refs(d, true, &name, &repl);
        d.styles_mut().paragraph.retain(|x| x.name != name);
        ok()
    })
}

fn create_char(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Character Style 1").to_string();
    let mut chars: CharAttrs = attrs("style.character.create", "chars", p, |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    // Validate caller-supplied fills; captured formatting may retain a deleted swatch name.
    super::text::validate_char_fill(&s.doc()?.doc, &chars, "style.character.create")?;
    if p.get("fromSelection").and_then(Value::as_bool).unwrap_or(false)
        && let Some(target) = super::text::format_targets_pub(s).first()
    {
        let st = s.doc()?;
        // Match paragraph fromSelection's first-character/caret policy, but keep paragraph
        // attributes inherited instead of freezing the fully resolved character properties.
        let format = super::text::effective_char_format(s, target)?;
        let mut captured = st.doc.styles.char_style_attrs(&format.style);
        captured.merge(&format.over);
        // Replay the explicit fields so null clears a captured value rather than looking absent.
        if let Some(overrides) = p.get("chars").and_then(Value::as_object) {
            for (key, value) in overrides {
                captured.set_json(key, value).map_err(|e| bad("style.character.create", format!("chars: {e}")))?;
            }
        }
        chars = captured;
    }
    // Based on [None] is based on nothing.
    let based_on = str_param(p, "basedOn").filter(|b| *b != designcraft_doc::NO_CHAR_STYLE).map(str::to_string);
    s.edit(|d, _| {
        if let Some(b) = &based_on
            && d.styles.char_style(b).is_none()
        {
            return Err(bad("style.character.create", format!("no character style `{b}`")));
        }
        let name = Styles::unique_name(|n| d.styles.char_style(n).is_some(), &base);
        d.styles_mut().character.push(CharacterStyle { name: name.clone(), based_on, chars, shortcut: String::new() });
        Ok(json!({"name": name}))
    })
}

fn edit_char(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.character.edit", "missing name"))?.to_string();
    if name == designcraft_doc::NO_CHAR_STYLE {
        return Err(bad("style.character.edit", "[None] can't be edited"));
    }
    let chars: CharAttrs = attrs("style.character.edit", "chars", p, |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    super::text::validate_char_fill(&s.doc()?.doc, &chars, "style.character.edit")?;
    // `null` unsets an attribute (the style no longer sets it).
    let unset: Vec<String> = p
        .get("chars")
        .and_then(Value::as_object)
        .map(|o| o.iter().filter(|(_, v)| v.is_null()).map(|(k, _)| k.clone()).collect())
        .unwrap_or_default();
    let rename = str_param(p, "rename").filter(|n| *n != name).map(|n| n.trim().to_string());
    let based: Option<Option<String>> = match p.get("basedOn") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(b)) if b == designcraft_doc::NO_CHAR_STYLE => Some(None),
        Some(Value::String(b)) => Some(Some(b.clone())),
        Some(_) => return Err(bad("style.character.edit", "`basedOn` is a style name or null")),
    };
    s.edit(|d, _| {
        if let Some(Some(b)) = &based {
            if d.styles.char_style(b).is_none() {
                return Err(bad("style.character.edit", format!("no character style `{b}`")));
            }
            if d.styles.char_based_on_cycles(&name, b) {
                return Err(bad("style.character.edit", "based-on would create a cycle"));
            }
        }
        if let Some(n) = &rename
            && (n.is_empty() || n.starts_with('[') || d.styles.char_style(n).is_some())
        {
            return Err(bad("style.character.edit", format!("can't rename to `{n}`")));
        }
        let st = d.styles_mut().char_style_mut(&name).ok_or_else(|| bad("style.character.edit", format!("no character style `{name}`")))?;
        st.chars.merge(&chars);
        for k in &unset {
            st.chars.set_json(k, &Value::Null).map_err(|e| bad("style.character.edit", e))?;
        }
        if let Some(b) = based.clone() {
            st.based_on = b;
        }
        if let Some(n) = &rename {
            // Every use: other styles, stories, table cells, nested and GREP styles, footnotes.
            rename_style(d, false, &name, n);
        }
        ok()
    })
}

fn delete_char(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").filter(|n| !n.is_empty()).ok_or_else(|| bad("style.character.delete", "missing name"))?.to_string();
    let repl = str_param(p, "replaceWith").unwrap_or(designcraft_doc::NO_CHAR_STYLE).to_string();
    if name.starts_with('[') {
        return Err(bad("style.character.delete", "built-in styles can't be deleted"));
    }
    if repl == name {
        return Err(bad("style.character.delete", "`replaceWith` can't be the deleted style"));
    }
    s.edit(|d, _| {
        if d.styles.char_style(&name).is_none() {
            return Err(bad("style.character.delete", format!("no character style `{name}`")));
        }
        if d.styles.char_style(&repl).is_none() {
            return Err(bad("style.character.delete", format!("no character style `{repl}`")));
        }
        redirect_style_refs(d, false, &name, &repl);
        d.styles_mut().character.retain(|x| x.name != name);
        ok()
    })
}

fn create_swatch(s: &mut Session, p: &Value) -> Result<Value> {
    let c = p.get("color").ok_or_else(|| bad("swatch.create", "missing color"))?;
    let color = parse_color(c).ok_or_else(|| bad("swatch.create", "bad color"))?;
    let spot = p.get("spot").and_then(Value::as_bool).unwrap_or(false);
    let name = str_param(p, "name").map(str::to_string).unwrap_or_else(|| match color {
        designcraft_color::Color::Cmyk { c, m, y, k } => designcraft_color::swatch::cmyk_name(c, m, y, k),
        other => {
            let [r, g, b] = other.to_rgb();
            format!("R={} G={} B={}", (r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round())
        }
    });
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

pub fn parse_color(v: &Value) -> Option<designcraft_color::Color> {
    use designcraft_color::Color;
    match v {
        Value::String(s) => Color::from_hex(s),
        // One scale per colour, not per component: [1, 128, 255] is 0..255. Only an RGB array whose
        // components are all within 0..1 is read as fractions (the older form).
        Value::Array(a) if a.len() >= 3 => {
            let v = [a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?];
            let scale = if v.iter().any(|&x| x > 1.0) { 255.0 } else { 1.0 };
            Some(Color::rgb((v[0] / scale) as f32, (v[1] / scale) as f32, (v[2] / scale) as f32))
        }
        // `{model, …}` is the colour's own form (as `color.convert` takes and returns it): fractions.
        Value::Object(o) if o.contains_key("model") => serde_json::from_value(v.clone()).ok(),
        // Bare CMYK is always 0..100, as documented: C 1 % is a real tint, so values of 1 or less can't
        // mean fractions (#910).
        Value::Object(o) => {
            let g = |k: &str| o.get(k).and_then(Value::as_f64).map(|x| (x / 100.0) as f32);
            Some(Color::cmyk(g("c")?, g("m")?, g("y")?, g("k")?))
        }
        _ => None,
    }
}

fn apply_object(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.object.apply", "missing name"))?.to_string();
    let os = s.doc()?.doc.styles.object_style(&name).cloned().ok_or_else(|| bad("style.object.apply", format!("no object style `{name}`")))?;
    let ids = super::targets(s, p)?;
    s.edit(|d, _| {
        let mut stories = Vec::new();
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            it.object_style = name.clone();
            if let Some(f) = &os.fill {
                it.fill = f.clone();
            }
            if let Some(st) = &os.stroke {
                it.stroke = st.clone();
            }
            if let Some(tf) = it.text_frame_mut() {
                if let Some(o) = &os.text_frame {
                    tf.options = o.clone();
                }
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

fn create_object(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Object Style 1").to_string();
    let from_sel = p.get("fromSelection").and_then(Value::as_bool).unwrap_or(true);
    let src = if from_sel { s.doc()?.selection.items.first().and_then(|i| s.doc().ok()?.doc.item(*i).cloned()) } else { None };
    let mut os = designcraft_doc::ObjectStyle::default();
    if let Some(it) = &src {
        os.fill = Some(it.fill.clone());
        os.stroke = Some(it.stroke.clone());
        os.text_frame = it.text_frame().map(|t| t.options.clone());
    }
    if let Some(sw) = str_param(p, "fill") {
        os.fill = Some(designcraft_doc::Fill::swatch(sw));
    }
    if let Some(ps) = str_param(p, "paragraphStyle") {
        os.paragraph_style = Some(ps.to_string());
    }
    s.edit(|d, _| {
        let name = Styles::unique_name(|n| d.styles.object_style(n).is_some(), &base);
        os.name = name.clone();
        d.styles_mut().object.push(os.clone());
        Ok(json!({"name": name}))
    })
}

/// The swatch name of a colour value ("C=… M=… Y=… K=…" / "R=… G=… B=…").
fn value_name(color: designcraft_color::Color) -> String {
    match color {
        designcraft_color::Color::Cmyk { c, m, y, k } => designcraft_color::swatch::cmyk_name(c, m, y, k),
        other => {
            let [r, g, b] = other.to_rgb();
            format!("R={} G={} B={}", (r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round())
        }
    }
}

fn load_swatches(s: &mut Session, p: &Value) -> Result<Value> {
    let bytes = match (str_param(p, "path"), str_param(p, "base64")) {
        (_, Some(b)) => super::file::base64_decode(b),
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
        let (mut added, mut replaced) = (0, 0);
        for w in incoming {
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

fn save_swatches(s: &mut Session, p: &Value) -> Result<Value> {
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
        None => Ok(json!({"base64": super::file::base64_encode(&bytes)})),
    }
}

fn gradient_json(name: &str, g: &designcraft_color::Gradient) -> Value {
    let stops: Vec<Value> = g
        .stops
        .iter()
        .map(|s| json!({"location": (s.offset as f64 * 1000.0).round() / 10.0, "color": s.color.to_hex(), "opacity": (s.opacity * 100.0).round(), "midpoint": (s.midpoint * 100.0).round()}))
        .collect();
    json!({"swatch": name, "kind": if g.kind == designcraft_color::GradientKind::Radial { "radial" } else { "linear" }, "stops": stops})
}

fn apply_gradient(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_color::{GradientKind, GradientStop};
    let kind = match str_param(p, "kind") {
        Some("radial") => Some(GradientKind::Radial),
        Some("linear") => Some(GradientKind::Linear),
        Some(k) => return Err(bad("object.gradient", format!("unknown kind {k}"))),
        None => None,
    };
    let stops = match p.get("stops").and_then(Value::as_array) {
        Some(a) => {
            let mut v = Vec::new();
            for st in a {
                let color = st.get("color").and_then(parse_color).ok_or_else(|| bad("object.gradient", "a stop needs a color"))?;
                let pct = |k: &str, d: f64| st.get(k).and_then(Value::as_f64).unwrap_or(d).clamp(0.0, 100.0) as f32 / 100.0;
                v.push(GradientStop {
                    offset: pct("location", 0.0),
                    color,
                    opacity: pct("opacity", 100.0),
                    midpoint: pct("midpoint", 50.0).clamp(0.13, 0.87),
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
    let (from, to) = (super::point_param(p, "from"), super::point_param(p, "to"));
    let ids = super::targets(s, p)?;
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

fn apply_color(s: &mut Session, p: &Value) -> Result<Value> {
    let color = p.get("color").and_then(parse_color).ok_or_else(|| bad("object.color", "missing or bad color"))?;
    let stroke = str_param(p, "target") == Some("stroke");
    let named = p.get("ids").is_some() || p.get("id").is_some();
    let text = !named
        && (p.get("text").and_then(Value::as_bool).unwrap_or(false) || s.doc()?.selection.text.is_some() || s.doc()?.selection.cells.is_some());
    let (ids, texts) = if text { (vec![], super::text::format_targets_pub(s)) } else { (super::targets(s, p)?, vec![]) };
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
            super::text::apply_char_attrs(d, &texts, &a, &[]);
        }
        Ok(json!({"swatch": name}))
    })?;
    if caret && let Some(name) = r.get("swatch").cloned() {
        let attrs = if stroke { json!({"stroke": name, "strokeTint": 1.0}) } else { json!({"fill": name, "fillTint": 1.0}) };
        super::text::format_chars(s, &attrs)?;
    }
    Ok(r)
}

/// A process swatch holding exactly `color`, else a new unnamed one (hidden from the Swatches
/// panel until Add to Swatches).
fn color_swatch(d: &mut designcraft_doc::Document, color: designcraft_color::Color) -> String {
    let existing = d
        .swatches
        .iter()
        .find(|w| matches!(&w.value, SwatchValue::Color { color: c, color_type: designcraft_color::ColorType::Process } if *c == color))
        .map(|w| w.name.clone());
    if let Some(n) = existing {
        return n;
    }
    let name = Styles::unique_name(|n| d.swatch(n).is_some(), &value_name(color));
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

fn has_text_or_selection(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.text.is_some() || d.selection.cells.is_some()) { Ok(()) } else { super::has_selection(s) }
}

/// The Eyedropper on a placed image (InDesign samples the pixel under the pointer): that colour,
/// as an unnamed colour, fills the selected text (characters), or the selected objects.
fn sample_color(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "color.sample";
    let at = super::point_param(p, "at").ok_or_else(|| bad(ID, "missing at"))?;
    let sr = super::spread_param(p, "spread")?;
    let st = s.doc()?;
    let (from, color) = image_color_at(&st.doc, sr, at).ok_or_else(|| bad(ID, "no placed image under the point"))?;
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
        Content::Group { items } => items.iter().rev().find_map(|c| sample_item(d, c, xf, at, depth + 1)),
        Content::Graphic(g) => {
            let inner = xf.inverse() * at;
            if !designcraft_geom::hit::fill_contains(&it.path.to_bezpath(), designcraft_geom::FillRule::NonZero, inner) {
                return None;
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

fn add_to_swatches(s: &mut Session, p: &Value) -> Result<Value> {
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
        let w = d.swatches.iter_mut().find(|w| w.name == name).ok_or_else(|| bad("swatch.addToSwatches", format!("no swatch `{name}`")))?;
        w.name = new.clone();
        w.named = true;
        for g in &mut d.color_groups {
            for w in &mut g.swatches {
                if *w == name {
                    *w = new.clone();
                }
            }
        }
        // Everything using the colour follows the rename.
        for sp in d.spreads.iter_mut().chain(d.parents.iter_mut()) {
            let sp = std::sync::Arc::make_mut(sp);
            for top in &mut sp.items {
                rename_in(std::sync::Arc::make_mut(top), &name, &new);
            }
        }
        for st in d.stories.values_mut() {
            let mut copy = (**st).clone();
            if rename_in_story(&mut copy, &name, &new, 0)? {
                *st = std::sync::Arc::new(copy);
            }
        }
        Ok(json!({"name": new}))
    })?;
    if let Some(new) = result.get("name").and_then(Value::as_str) {
        super::text::rename_typing_swatch(s, &name, new);
    }
    Ok(result)
}

/// Point text colours (character fill, stroke, underline and strikethrough; paragraph shading,
/// border and rules; table cells, borders and alternating fills) at a renamed swatch, in a story
/// and the texts inside it (table cells, footnotes, endnotes). Returns whether anything changed.
fn rename_in_story(st: &mut designcraft_doc::Story, from: &str, to: &str, depth: usize) -> Result<bool> {
    // Refuse the whole edit instead of leaving deeper references pointing at a removed name.
    if depth > 32 {
        return Err(bad("swatch.addToSwatches", "story nesting exceeds 32 levels"));
    }
    fn fix(s: &mut String, from: &str, to: &str) -> bool {
        let hit = s == from;
        if hit {
            *s = to.to_string();
        }
        hit
    }
    let mut n = 0;
    let mut f = |s: &mut String| n += usize::from(fix(s, from, to));
    for r in &mut st.chars {
        let o = &mut r.format.over;
        for c in [&mut o.fill, &mut o.stroke, &mut o.underline_color, &mut o.strikethrough_color].into_iter().flatten() {
            f(c);
        }
    }
    for pf in &mut st.paras {
        let a = &mut pf.para;
        for c in [&mut a.shading_color, &mut a.border_color].into_iter().flatten() {
            f(c);
        }
        for r in [&mut a.rule_above, &mut a.rule_below].into_iter().flatten() {
            f(&mut r.color);
        }
    }
    let mut nested = false;
    for t in st.tables.values_mut() {
        let t = std::sync::Arc::make_mut(t);
        let o = &mut t.options;
        f(&mut o.border.color);
        for b in o.borders.iter_mut().flatten() {
            f(&mut b.color);
        }
        for a in [&mut o.alt_rows, &mut o.alt_cols].into_iter().flatten() {
            f(&mut a.first_color);
            f(&mut a.next_color);
        }
        for k in [&mut o.row_strokes, &mut o.column_strokes].into_iter().flatten() {
            f(&mut k.first_stroke.color);
            f(&mut k.next_stroke.color);
        }
        for c in &mut t.cells {
            f(&mut c.fill);
            for k in &mut c.strokes {
                f(&mut k.color);
            }
            nested |= rename_in_story(&mut c.text, from, to, depth + 1)?;
        }
    }
    for note in st.notes.iter_mut().chain(st.endnotes.iter_mut()) {
        nested |= rename_in_story(&mut std::sync::Arc::make_mut(note).text, from, to, depth + 1)?;
    }
    let changed = n > 0 || nested;
    if changed {
        st.rev = st.rev.wrapping_add(1);
    }
    Ok(changed)
}

/// Point fills and strokes of an item (and its children) at a renamed swatch.
fn rename_in(it: &mut designcraft_doc::Item, from: &str, to: &str) {
    if it.fill.swatch == from {
        it.fill.swatch = to.to_string();
    }
    if it.stroke.swatch == from {
        it.stroke.swatch = to.to_string();
    }
    if let Some(kids) = it.children_mut() {
        for k in kids {
            rename_in(std::sync::Arc::make_mut(k), from, to);
        }
    }
}

/// #777: the params descriptions name the attributes, and a wrong one is the command's error.
#[cfg(test)]
mod attribute_error_tests {
    use super::*;
    use crate::cmd::find_command;

    #[test]
    fn a_wrong_attribute_names_the_command_and_the_right_attribute() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let err = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_err().to_string();
        assert_eq!(
            err(&mut s, "style.paragraph.create", json!({"name": "H", "chars": {"font": "Source Sans 3"}})),
            "invalid parameters for `style.paragraph.create`: chars: unknown attribute `font`; did you mean `fontFamily`?"
        );
        assert_eq!(
            err(&mut s, "style.paragraph.create", json!({"name": "H", "para": {"spaceBefor": 6}})),
            "invalid parameters for `style.paragraph.create`: para: unknown attribute `spaceBefor`; did you mean `spaceBefore`?"
        );
        assert!(
            err(&mut s, "style.character.create", json!({"name": "C", "chars": {"font": "X"}}))
                .starts_with("invalid parameters for `style.character.create`: chars:")
        );
        assert!(err(&mut s, "style.paragraph.create", json!({"name": "H", "chars": "bold"})).contains("chars must be an object of attributes"));
        s.execute("style.paragraph.create", &json!({"name": "H", "chars": {"fontFamily": "Source Sans 3", "size": 14}})).unwrap();
        assert!(
            err(&mut s, "style.paragraph.edit", json!({"name": "H", "chars": {"font": "X"}}))
                .starts_with("invalid parameters for `style.paragraph.edit`: chars:")
        );
        s.execute("style.character.create", &json!({"name": "C", "chars": {"fontStyle": "Bold"}})).unwrap();
        assert!(
            err(&mut s, "style.character.edit", json!({"name": "C", "chars": {"font": "X"}}))
                .starts_with("invalid parameters for `style.character.edit`: chars:")
        );
        for id in ["style.paragraph.create", "style.paragraph.edit", "style.character.create", "style.character.edit"] {
            let doc = find_command(id).unwrap().params;
            assert!(doc.contains("as type.char names them: fontFamily"), "{id}: {doc}");
        }
        assert!(find_command("style.paragraph.create").unwrap().params.contains("as type.para names them: align"));
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;

    fn fill_session() -> Session {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let frame = session.execute("frame.create", &json!({"rect": [0, 0, 200, 100], "content": "text", "text": "Hi"})).unwrap();
        session.execute("text.select", &json!({"story": frame["story"], "anchor": 0, "focus": 2})).unwrap();
        session.execute("style.paragraph.create", &json!({"name": "Existing"})).unwrap();
        session.execute("style.character.create", &json!({"name": "Existing"})).unwrap();
        session.execute("type.char", &json!({"size": 18})).unwrap();
        session.execute("edit.undo", &json!({})).unwrap();
        session
    }

    fn assert_fill_rejected(session: &mut Session, command: &str, params: Value) {
        let before = session.doc().unwrap().clone();
        let journal = session.journal.clone();
        let fill = params
            .pointer("/chars/fill")
            .or_else(|| params.pointer("/attrs/fill"))
            .or_else(|| params.get("fill"))
            .and_then(Value::as_str)
            .unwrap_or("Missing Ink");

        let result = session.execute(command, &params);

        match result {
            Err(crate::EngineError::BadParams { cmd, msg }) => {
                assert_eq!(cmd, command);
                assert!(msg.contains(fill), "{msg}");
                assert!(msg.contains("swatch.create") && msg.contains("returned name"), "{msg}");
            }
            other => panic!("expected BadParams for {command}, got {other:?}"),
        }
        let after = session.doc().unwrap();
        assert!(std::sync::Arc::ptr_eq(&before.doc, &after.doc));
        assert_eq!(before.selection, after.selection);
        assert_eq!(before.revision, after.revision);
        assert_eq!(format!("{:?}", before.history), format!("{:?}", after.history));
        assert_eq!(journal, session.journal);
    }

    #[test]
    fn char_fill_rejects_unknown_flat_and_nested_names_when_formatting() {
        for fill in ["Missing Ink", "#ff0000", "[black]", " [Black]"] {
            for params in [json!({"fill": fill, "size": 24}), json!({"attrs": {"fill": fill, "size": 24}})] {
                let mut session = fill_session();
                assert_fill_rejected(&mut session, "type.char", params);
            }
        }
    }

    #[test]
    fn char_fill_rejects_unknown_names_when_creating_or_editing_styles() {
        for command in ["style.paragraph.create", "style.paragraph.edit", "style.character.create", "style.character.edit"] {
            for fill in ["Missing Ink", "#ff0000"] {
                let mut session = fill_session();
                session.execute("style.paragraph.apply", &json!({"name": "Existing"})).unwrap();
                session.execute("style.character.apply", &json!({"name": "Existing"})).unwrap();
                assert_fill_rejected(&mut session, command, json!({"name": "Existing", "rename": "Renamed", "chars": {"fill": fill, "size": 24}}));
            }
        }
    }

    #[test]
    fn char_fill_preserves_deleted_swatch_when_creating_from_selection() {
        for params in
            [json!({"name": "Inherited", "fromSelection": true}), json!({"name": "Inherited", "fromSelection": true, "chars": {"size": 24}})]
        {
            let mut session = fill_session();
            let swatch = session.execute("swatch.create", &json!({"name": "Deleted Ink", "color": "#108080"})).unwrap();
            let fill = swatch["name"].as_str().unwrap();
            session.execute("type.char", &json!({"fill": fill})).unwrap();
            session.execute("swatch.delete", &json!({"name": fill})).unwrap();
            assert_eq!(session.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], fill);

            session.execute("style.paragraph.create", &params).unwrap();

            let chars = &session.doc().unwrap().doc.styles.para("Inherited").unwrap().chars;
            assert_eq!(chars.fill.as_deref(), Some(fill));
            if params.get("chars").is_some() {
                assert_eq!(chars.size, Some(24.0));
            }
        }
    }

    #[test]
    fn char_fill_rejects_explicit_unknown_name_when_creating_from_selection() {
        for fill in ["Missing Ink", "#ff0000"] {
            let mut session = fill_session();

            assert_fill_rejected(
                &mut session,
                "style.paragraph.create",
                json!({"name": "Explicit", "fromSelection": true, "chars": {"fill": fill, "size": 24}}),
            );
        }
    }

    #[test]
    fn char_fill_accepts_valid_override_when_selection_has_unknown_name() {
        let mut session = fill_session();
        session
            .edit(|document, selection| {
                let selected = selection.text.unwrap();
                document.story_mut(selected.story).unwrap().format_chars(selected.range(), |format| format.over.fill = Some("Missing Ink".into()));
                Ok(())
            })
            .unwrap();

        session.execute("style.paragraph.create", &json!({"name": "Override", "fromSelection": true, "chars": {"fill": "[Paper]"}})).unwrap();

        assert_eq!(session.doc().unwrap().doc.styles.para("Override").unwrap().chars.fill.as_deref(), Some("[Paper]"));
    }

    #[test]
    fn char_fill_rejects_unknown_name_when_changing_text_even_without_output() {
        for (find, change) in [("Hi", "Bye"), ("Hi", ""), ("Absent", "Bye")] {
            for fill in ["Missing Ink", "#ff0000"] {
                let mut session = fill_session();
                assert_fill_rejected(&mut session, "find.change", json!({"find": find, "change": change, "attrs": {"fill": fill}}));
            }
        }
    }

    #[test]
    fn char_fill_preserves_exact_registered_names_when_used_at_all_boundaries() {
        for fill in ["Brand Ink", "#ff0000", "[None]", "[Black]"] {
            let mut session = fill_session();
            if !fill.starts_with('[') {
                let created = session.execute("swatch.create", &json!({"name": fill, "color": "#108080"})).unwrap();
                assert_eq!(created["name"], fill);
            }
            for params in [json!({"fill": fill}), json!({"attrs": {"fill": fill}})] {
                session.execute("type.char", &params).unwrap();
                assert_eq!(session.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], fill);
            }
            for kind in ["paragraph", "character"] {
                session.execute(&format!("style.{kind}.create"), &json!({"name": "Colored", "chars": {"fill": fill}})).unwrap();
                session.execute(&format!("style.{kind}.edit"), &json!({"name": "Existing", "chars": {"fill": fill}})).unwrap();
                let document = &session.doc().unwrap().doc;
                for name in ["Colored", "Existing"] {
                    let attrs = if kind == "paragraph" {
                        &document.styles.para(name).unwrap().chars
                    } else {
                        &document.styles.char_style(name).unwrap().chars
                    };
                    assert_eq!(attrs.fill.as_deref(), Some(fill));
                }
            }
            assert_eq!(session.execute("find.change", &json!({"find": "Hi", "change": "Bye", "attrs": {"fill": fill}})).unwrap()["count"], 1);
            let selected = session.doc().unwrap().selection.text.unwrap();
            let story = session.doc().unwrap().doc.story(selected.story).unwrap();
            assert_eq!(story.text, "Bye");
            assert_eq!(story.format_after(0).over.fill.as_deref(), Some(fill));
        }
    }

    #[test]
    fn char_fill_accepts_omitted_and_null_attributes_when_used_at_all_boundaries() {
        for chars in [json!({"size": 18}), json!({"fill": null})] {
            let mut session = fill_session();
            session.execute("type.char", &json!({"attrs": chars})).unwrap();
            for kind in ["paragraph", "character"] {
                session.execute(&format!("style.{kind}.create"), &json!({"name": "Nullable", "chars": chars})).unwrap();
                session.execute(&format!("style.{kind}.edit"), &json!({"name": "Existing", "chars": chars})).unwrap();
            }
            assert_eq!(session.execute("find.change", &json!({"find": "Hi", "change": "Bye", "attrs": chars})).unwrap()["count"], 1);
        }
    }

    #[test]
    fn percent_components_at_most_one_stay_percent() {
        use designcraft_color::Color;
        // {"c": 0.7, ..., "y": 12.2} is a pale cream (C 0.7 %, Y 12.2 %), not C 70 %.
        let Some(Color::Cmyk { c, m, y, k }) = parse_color(&json!({"c": 0.7, "m": 0, "y": 12.2, "k": 0})) else { panic!("not CMYK") };
        assert!((c - 0.007).abs() < 1e-6 && m == 0.0 && (y - 0.122).abs() < 1e-6 && k == 0.0, "got {c} {m} {y} {k}");
        // Fractions take the colour's own form, with its model.
        let Some(Color::Cmyk { c, k, .. }) = parse_color(&json!({"model": "cmyk", "c": 0.5, "m": 0, "y": 0, "k": 1})) else { panic!("not CMYK") };
        assert!((c - 0.5).abs() < 1e-6 && (k - 1.0).abs() < 1e-6);
        // RGB arrays follow the same rule: [1, 128, 255] is 0..255.
        let Some(Color::Rgb { r, g, b }) = parse_color(&json!([1, 128, 255])) else { panic!("not RGB") };
        assert!((r - 1.0 / 255.0).abs() < 1e-6 && (g - 128.0 / 255.0).abs() < 1e-6 && (b - 1.0).abs() < 1e-6);
    }

    /// #910: `{c: 1, m: 0, y: 0, k: 0}` is a 1 % cyan tint, not solid cyan, though every component is within 0..1.
    #[test]
    fn cmyk_percentages_of_one_or_less_stay_percent() {
        use designcraft_color::Color;
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("swatch.create", &json!({"name": "Tint1", "color": {"c": 1, "m": 0, "y": 0, "k": 0}})).unwrap();
        s.execute("object.fill", &json!({"ids": [id], "swatch": "Tint1"})).unwrap();
        let d = &s.doc().unwrap().doc;
        let Some(Color::Cmyk { c, m, y, k }) = d.resolve_color("Tint1", 1.0) else { panic!("not CMYK") };
        assert!((c - 0.01).abs() < 1e-6 && m == 0.0 && y == 0.0 && k == 0.0, "got {c} {m} {y} {k}");
        let pdf = designcraft_pdf::export_pdf(d, &s.cache, &designcraft_pdf::PdfOptions { compress: false, ..Default::default() }).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        // Operands of every `k` (DeviceCMYK fill), last first: [k, y, m, c]. Inks are written in 1/255 steps (C 1 % is 3/255).
        let fills: Vec<Vec<f32>> =
            text.split(" k ").filter_map(|before| before.rsplit(' ').take(4).map(|x| x.parse().ok()).collect::<Option<Vec<f32>>>()).collect();
        let cyan = |want: f32| fills.iter().any(|f| (f[3] - want).abs() < 0.5 / 255.0 + 1e-6 && f[..3] == [0.0; 3]);
        assert!(cyan(0.01), "C 1 % in the PDF: {fills:?}");
        assert!(!cyan(1.0), "no solid cyan: {fills:?}");
        // A light tint whose components are all 1 or less, through object.color.
        s.execute("selection.set", &json!({"ids": [id]})).unwrap();
        s.execute("object.color", &json!({"color": {"c": 0.5, "m": 0.5, "y": 0, "k": 0}})).unwrap();
        let d = &s.doc().unwrap().doc;
        let fill = &d.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch;
        let Some(Color::Cmyk { c, m, .. }) = d.resolve_color(fill, 1.0) else { panic!("not CMYK") };
        assert!((c - 0.005).abs() < 1e-6 && (m - 0.005).abs() < 1e-6, "got {c} {m}");
    }

    #[test]
    fn style_groups_rename_every_use() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Sub", "basedOn": "Head"})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hi"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 2})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Head"})).unwrap();
        let r = s.execute("style.group", &json!({"kind": "paragraph", "names": ["Head", "Sub"], "group": "Titles"})).unwrap();
        assert_eq!(r["renamed"]["Head"], "Titles/Head");
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.story(designcraft_doc::StoryId(sid)).unwrap().paras[0].style, "Titles/Head");
        assert_eq!(d.styles.para("Titles/Sub").unwrap().based_on.as_deref(), Some("Titles/Head"));
        // Out of the group again.
        s.execute("style.group", &json!({"kind": "paragraph", "names": ["Titles/Head"], "group": ""})).unwrap();
        assert!(s.doc().unwrap().doc.styles.para("Head").is_some());
        assert!(s.execute("style.group", &json!({"names": ["[Basic Paragraph]"], "group": "X"})).is_err());
    }

    #[test]
    fn character_style_edit_sets_unsets_rebases_and_renames_every_use() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Base", "chars": {"fontStyle": "Bold"}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Emphasis", "chars": {"fontStyle": "Italic", "tracking": 20}})).unwrap();
        s.execute(
            "style.paragraph.create",
            &json!({"name": "Lead", "para": {
                "nestedStyles": [{"style": "Emphasis", "through": true, "count": 1, "until": {"kind": "words"}}],
                "grepStyles": [{"style": "Emphasis", "pattern": "\\d+"}],
                "nestedLineStyles": [{"style": "Emphasis", "lines": 1}]}}),
        )
        .unwrap();
        s.execute("footnote.options", &json!({"refCharStyle": "Emphasis"})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hi there"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 2})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Emphasis"})).unwrap();

        s.execute(
            "style.character.edit",
            &json!({"name": "Emphasis", "rename": "Strong", "basedOn": "Base", "chars": {"size": 14, "fill": "[Paper]", "fontStyle": null}}),
        )
        .unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.styles.char_style("Emphasis").is_none());
        let st = d.styles.char_style("Strong").unwrap();
        assert_eq!(st.based_on.as_deref(), Some("Base"));
        // Exactly the attributes set: `null` unset the font style, the rest stayed unset.
        assert_eq!(st.chars, CharAttrs { size: Some(14.0), fill: Some("[Paper]".into()), tracking: Some(20.0), ..Default::default() });
        let story = d.story(designcraft_doc::StoryId(sid)).unwrap();
        assert!(story.chars.iter().any(|r| r.format.style == "Strong"));
        assert!(story.chars.iter().all(|r| r.format.style != "Emphasis"));
        let lead = d.styles.para("Lead").unwrap();
        assert_eq!(lead.para.nested_styles.as_ref().unwrap()[0].style, "Strong");
        assert_eq!(lead.para.grep_styles.as_ref().unwrap()[0].style, "Strong");
        assert_eq!(lead.para.nested_line_styles.as_ref().unwrap()[0].style, "Strong");
        assert_eq!(d.footnote_options.ref_char_style, "Strong");

        // Based On [None] (null or the name) clears it.
        s.execute("style.character.edit", &json!({"name": "Strong", "basedOn": null})).unwrap();
        assert_eq!(s.doc().unwrap().doc.styles.char_style("Strong").unwrap().based_on, None);
        s.execute("style.character.edit", &json!({"name": "Strong", "basedOn": "Base"})).unwrap();
        s.execute("style.character.edit", &json!({"name": "Strong", "basedOn": "[None]"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.styles.char_style("Strong").unwrap().based_on, None);

        // Refused: a based-on cycle, an unknown parent, a taken or empty name, editing [None].
        s.execute("style.character.edit", &json!({"name": "Strong", "basedOn": "Base"})).unwrap();
        assert!(s.execute("style.character.edit", &json!({"name": "Base", "basedOn": "Strong"})).is_err());
        assert!(s.execute("style.character.edit", &json!({"name": "Strong", "basedOn": "Nope"})).is_err());
        assert!(s.execute("style.character.edit", &json!({"name": "Strong", "rename": "Base"})).is_err());
        assert!(s.execute("style.character.edit", &json!({"name": "Strong", "rename": "  "})).is_err());
        assert!(s.execute("style.character.edit", &json!({"name": "[None]", "chars": {"size": 9}})).is_err());
        assert!(s.doc().unwrap().doc.styles.char_style("[None]").unwrap().chars.is_empty());
    }

    #[test]
    fn new_character_style_checks_its_based_on() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert!(s.execute("style.character.create", &json!({"name": "Orphan", "basedOn": "Nope"})).is_err());
        s.execute("style.character.create", &json!({"name": "Plain", "basedOn": "[None]", "chars": {"size": 9}})).unwrap();
        let st = s.doc().unwrap().doc.styles.char_style("Plain").cloned().unwrap();
        assert_eq!((st.based_on, st.chars), (None, CharAttrs { size: Some(9.0), ..Default::default() }));
    }

    #[test]
    fn swatches_save_and_load_ase() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("swatch.create", &json!({"name": "Brand Teal", "color": "#108080"})).unwrap();
        let b64 = s.execute("swatch.save", &json!({"names": ["Brand Teal"]})).unwrap()["base64"].as_str().unwrap().to_string();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("swatch.load", &json!({"base64": b64})).unwrap();
        assert_eq!(r["added"], 1);
        assert!(s.doc().unwrap().doc.swatch("Brand Teal").is_some());
        let r = s.execute("swatch.load", &json!({"base64": b64})).unwrap();
        assert_eq!((r["added"].as_u64(), r["replaced"].as_u64()), (Some(0), Some(0)));
        assert!(s.execute("swatch.load", &json!({"base64": "AAAA"})).is_err());
    }

    #[test]
    fn loading_swatches_matches_the_first_swatch_of_each_name() {
        use designcraft_color::{Color, ColorType};
        let ink = |k: f32| SwatchValue::Color { color: Color::cmyk(0.0, 0.0, 0.0, k), color_type: ColorType::Process };
        let sw = |name: &str, k: f32| Swatch { name: name.into(), value: ink(k), locked: false, named: true, hidden: false };
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // Two swatches share a name, and an unnamed color is hidden.
        s.edit(|d, _| {
            d.swatches.extend([sw("Twin", 0.1), sw("Twin", 0.2), Swatch { named: false, hidden: true, ..sw("Unnamed", 0.3) }]);
            Ok(())
        })
        .unwrap();
        let file = designcraft_color::ase::write(&[sw("New", 0.4), sw("New", 0.5), sw("Twin", 0.6), sw("Unnamed", 0.7), sw("[Black]", 0.8)]);
        let b64 = crate::cmd::base64_encode(&file);
        let named = |s: &Session, name: &str| -> Vec<(SwatchValue, bool)> {
            s.doc().unwrap().doc.swatches.iter().filter(|w| w.name == name).map(|w| (w.value.clone(), w.hidden)).collect()
        };
        // With `replace`, the file's second "New" replaces its first, only the first "Twin" changes, the hidden
        // swatch becomes visible, and the locked [Black] stays.
        let r = s.execute("swatch.load", &json!({"base64": b64, "replace": true})).unwrap();
        assert_eq!((r["added"].as_u64(), r["replaced"].as_u64()), (Some(1), Some(3)));
        assert_eq!(named(&s, "New"), [(ink(0.5), false)]);
        assert_eq!(named(&s, "Twin"), [(ink(0.6), false), (ink(0.2), false)]);
        assert_eq!(named(&s, "Unnamed"), [(ink(0.7), false)]);
        assert_eq!(named(&s, "[Black]"), [(ink(1.0), false)]);
        // Without it, every name is already there and nothing changes.
        let before = s.doc().unwrap().doc.swatches.clone();
        let r = s.execute("swatch.load", &json!({"base64": b64})).unwrap();
        assert_eq!((r["added"].as_u64(), r["replaced"].as_u64()), (Some(0), Some(0)));
        assert_eq!(s.doc().unwrap().doc.swatches, before);
    }

    #[test]
    fn unnamed_colors_then_add_to_swatches() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [id]})).unwrap();
        let listed = |s: &Session| s.doc().unwrap().doc.swatches.iter().filter(|w| !w.hidden).count();
        let before = listed(&s);
        let r = s.execute("object.color", &json!({"color": {"c": 10, "m": 20, "y": 30, "k": 0}})).unwrap();
        assert_eq!(r["swatch"], "C=10 M=20 Y=30 K=0");
        assert_eq!(listed(&s), before, "unnamed: not in the Swatches panel");
        // The same colour again reuses it; the stroke target works too.
        let n = s.doc().unwrap().doc.swatches.len();
        s.execute("object.color", &json!({"color": {"c": 10, "m": 20, "y": 30, "k": 0}, "target": "stroke"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.swatches.len(), n);
        let d = &s.doc().unwrap().doc;
        let it = d.item(designcraft_doc::ItemId(id)).unwrap();
        assert_eq!(it.fill.swatch, it.stroke.swatch);
        // Add to Swatches with a name: listed, and the object follows the rename.
        s.execute("swatch.addToSwatches", &json!({"name": "Sand"})).unwrap();
        assert_eq!(listed(&s), before + 1);
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, "Sand");
        // A colour equal to an existing swatch applies that swatch.
        s.execute("object.color", &json!({"color": {"c": 10, "m": 20, "y": 30, "k": 0}})).unwrap();
        assert_eq!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, "Sand");
    }

    #[test]
    fn add_to_swatches_renames_the_colour_in_text_cells_and_notes() {
        use std::sync::Arc;
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Hello world"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 11})).unwrap();
        s.execute("footnote.insert", &json!({"text": "note"})).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 1, "focus": 4})).unwrap();
        let old = s.execute("object.color", &json!({"color": {"c": 10, "m": 20, "y": 30, "k": 0}})).unwrap()["swatch"].as_str().unwrap().to_string();
        // The same unnamed colour on a cell, the cell's text and the footnote's text.
        s.edit(|d, _| {
            let st = d.story_mut(sid).unwrap();
            let t = Arc::make_mut(st.tables.values_mut().next().unwrap());
            t.cells[0].fill = old.clone();
            t.cells[0].text.chars[0].format.over.fill = Some(old.clone());
            Arc::make_mut(&mut st.notes[0]).text.chars[0].format.over.stroke = Some(old.clone());
            Ok(())
        })
        .unwrap();
        s.execute("swatch.addToSwatches", &json!({"swatch": old, "name": "Sand"})).unwrap();
        let d = &s.doc().unwrap().doc;
        let st = d.story(sid).unwrap();
        assert_eq!(st.char_format_at(2).over.fill.as_deref(), Some("Sand"));
        let cell = &st.tables.values().next().unwrap().cells[0];
        assert_eq!(cell.fill, "Sand");
        assert_eq!(cell.text.chars[0].format.over.fill.as_deref(), Some("Sand"));
        assert_eq!(st.notes[0].text.chars[0].format.over.stroke.as_deref(), Some("Sand"));
        d.check().unwrap();
    }
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
            String::from_utf8_lossy(&super::super::file::base64_decode(&b)).into_owned()
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
        assert!(back.inks.resolve("Pantone A").1, "IDML ConvertToProcess");
        assert!(back.inks.aliases.iter().any(|(a, b)| a == "Pantone A" && b == "Pantone B"), "IDML AliasInkName");
        assert!(s.execute("ink.options", &json!({"ink": "[Black]", "toProcess": true})).is_err());
    }
}

#[cfg(test)]
mod export_tag_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn export_tagging_shapes_epub_markup() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Chapter Title", "chars": {"size": 24}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Key Term", "chars": {"fontStyle": "Bold"}})).unwrap();
        s.execute("style.exportTag", &json!({"style": "Chapter Title", "tag": "h1", "class": "chapter"})).unwrap();
        s.execute("style.exportTag", &json!({"style": "Key Term", "character": true, "tag": "strong"})).unwrap();
        assert!(s.execute("style.exportTag", &json!({"style": "Nope"})).is_err());
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Origins\nA word"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Chapter Title"})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 10, "focus": 14})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Key Term"})).unwrap();
        let html = s.execute("file.exportHtml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(html.contains("<h1 class=\"chapter\">Origins</h1>"), "{html}");
        assert!(html.contains("<strong class=\"key-term\">word</strong>"), "{html}");
        // Tagged PDF: a heading element and a paragraph element.
        let r = s.execute("file.exportPdf", &json!({"tagged": true, "compress": false})).unwrap();
        let pdf = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/S/H1") && text.contains("/S/P"), "structure elements");
        assert!(designcraft_render::pdf_page_count(&pdf) == Some(1));
    }
}

#[cfg(test)]
mod cjk_tests {
    use super::*;
    #[test]
    fn cjk_composite_font_command_invalidates_layout_and_undo_restores_it() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 300, 150], "content": "text", "text": "AB"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let definition = |scale| json!({"name": "Mixed", "entries": [{"family": "Source Serif 4", "relativeSize": scale}]});
        s.execute("style.compositeFont.set", &definition(1.0)).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 2})).unwrap();
        s.execute("type.char", &json!({"fontFamily": "Mixed", "leadingAki": 0.25, "jidori": 4})).unwrap();
        let old = s.cache.get(&s.doc().unwrap().doc, sid, None);
        s.execute("style.compositeFont.set", &definition(2.0)).unwrap();
        let new = s.cache.get(&s.doc().unwrap().doc, sid, None);
        assert!(!std::sync::Arc::ptr_eq(&old, &new));
        assert!(new.frames[0].lines[0].end_x > old.frames[0].lines[0].end_x);
        s.execute("edit.undo", &json!({})).unwrap();
        let restored = s.cache.get(&s.doc().unwrap().doc, sid, None);
        assert_eq!(old.frames[0].lines[0].end_x, restored.frames[0].lines[0].end_x);
        assert!(s.execute("style.compositeFont.set", &definition(-1.0)).is_err());
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
        let b64 = super::super::file::base64_encode(&png);
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
    fn nothing_to_sample_is_refused_without_an_edit() {
        let (mut s, _) = session_with_image();
        let f = s.execute("frame.create", &json!({"rect": [300, 300, 500, 360], "content": "text", "text": "Hi"})).unwrap();
        let fid = ItemId(f["id"].as_u64().unwrap());
        let story = s.doc().unwrap().doc.item(fid).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": story.0, "anchor": 1, "focus": 1})).unwrap();
        let sampled = s.execute("color.sample", &json!({"at": [100, 100]})).unwrap();
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], sampled["swatch"]);
        assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "Hi", "caret color waits for typing");
        let undo = s.doc().unwrap().history.undo.len();
        s.execute("text.select", &json!({"story": story.0, "anchor": 0, "focus": 2})).unwrap();
        let none = s.execute("color.sample", &json!({"at": [600, 600]})).unwrap_err().to_string();
        assert!(none.contains("no placed image"), "{none}");
        // Just past the image's right edge.
        assert!(s.execute("color.sample", &json!({"at": [216.5, 100]})).is_err());
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }

    #[test]
    fn sampling_and_explicit_colour_use_the_same_text_application_path() {
        for command in ["color.sample", "object.color"] {
            let (mut s, _) = session_with_image();
            let f = s.execute("frame.create", &json!({"rect": [300, 300, 500, 360], "content": "text"})).unwrap();
            let story = designcraft_doc::StoryId(f["story"].as_u64().unwrap());
            s.execute("text.select", &json!({"story": story.0, "anchor": 0})).unwrap();
            let undo = s.doc().unwrap().history.undo.len();
            let r = s.execute(command, &json!({"at": [100, 100], "color": "#ff0000"})).unwrap();
            assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "{command}");
            s.execute("text.insert", &json!({"text": "Hi"})).unwrap();
            let text = s.doc().unwrap().doc.story(story).unwrap();
            assert_eq!(text.format_after(0).over.fill.as_deref(), r["swatch"].as_str(), "empty-story typing: {command}");
            // Explicit ids override text targeting, including with a text selection active.
            let fid = ItemId(f["id"].as_u64().unwrap());
            s.execute("text.select", &json!({"story": story.0, "anchor": 0, "focus": 2})).unwrap();
            let r = s.execute(command, &json!({"at": [180, 100], "color": "#0000ff", "id": fid.0})).unwrap();
            assert_eq!(s.doc().unwrap().doc.item(fid).unwrap().fill.swatch, r["swatch"].as_str().unwrap());
            let text = s.doc().unwrap().doc.story(story).unwrap();
            assert_ne!(text.format_after(0).over.fill.as_deref(), r["swatch"].as_str(), "text unchanged: {command}");
            let r = s.execute(command, &json!({"at": [180, 100], "color": "#0000ff", "target": "stroke"})).unwrap();
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

#[cfg(test)]
#[path = "text_color_tests.rs"]
mod text_color_tests;
