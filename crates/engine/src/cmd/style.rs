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
            "{name, basedOn?, nextStyle?, para?: {…}, chars?: {…}, fromSelection?: bool}",
            has_doc,
            create_para
        ),
        cmd!(
            "style.paragraph.edit",
            "Paragraph Style Options…",
            [],
            None,
            "{name, rename?, basedOn?, nextStyle?, para?: {… ruleAbove?/ruleBelow?: only the named rule fields change}, chars?: {…}}",
            has_doc,
            edit_para
        ),
        cmd!("style.paragraph.delete", "Delete Paragraph Style", [], None, "{name, replaceWith?}", has_doc, delete_para),
        cmd!(
            "style.character.create",
            "New Character Style…",
            [],
            None,
            "{name, basedOn?: style | \"[None]\", chars?: {…}} — the style sets only `chars` → {name} (made unique)",
            has_doc,
            create_char
        ),
        cmd!(
            "style.character.edit",
            "Character Style Options…",
            [],
            None,
            "{name, rename?, basedOn?: style | \"[None]\" | null, chars?: {attr: value | null (no longer set)}} — renaming updates every use",
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
            "{name?, color: \"#rrggbb\"|{c,m,y,k}(0..100)|[r,g,b], spot?: bool}",
            has_doc,
            create_swatch
        ),
        cmd!(
            "object.gradient",
            "Gradient",
            [],
            None,
            "{kind?: linear|radial, stops?: [{location: 0–100, color: \"#rrggbb\"|{c,m,y,k}|[r,g,b], opacity?: 0–100, midpoint?: 13–87}], reverse?: bool, angle?: degrees, from?: [x,y], to?: [x,y] (spread coords: the Gradient Swatch tool's drag), ids?} → {swatch, kind, stops} — edits the fill's gradient (an unnamed gradient unless it equals a gradient swatch)",
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
            "{color: \"#rrggbb\"|{c,m,y,k}(0..100)|[r,g,b](0..255), target?: fill|stroke, ids?} — an unnamed colour (not in the Swatches panel until Add to Swatches) on the selection",
            super::has_selection,
            apply_color
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

fn attrs<T: Default>(v: Option<&Value>, set: impl Fn(&mut T, &str, &Value) -> std::result::Result<(), String>) -> Result<T> {
    let mut a = T::default();
    if let Some(o) = v.and_then(Value::as_object) {
        for (k, v) in o {
            set(&mut a, k, v).map_err(|e| bad("style", e))?;
        }
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
    let mut para: ParaAttrs = attrs(p.get("para"), |a: &mut ParaAttrs, k, v| a.set_json(k, v))?;
    let mut chars: CharAttrs = attrs(p.get("chars"), |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
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
    let chars: CharAttrs = attrs(p.get("chars"), |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
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
        let para: ParaAttrs = attrs(p.get("para"), |a: &mut ParaAttrs, k, v| a.set_json_over(k, v, &current))?;
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
    let st = d.styles_mut();
    if para {
        for s in &mut st.paragraph {
            if s.name == from {
                s.name = to.to_string();
            }
            for r in [&mut s.based_on, &mut s.next_style] {
                if r.as_deref() == Some(from) {
                    *r = Some(to.to_string());
                }
            }
        }
        for o in &mut st.object {
            if o.paragraph_style.as_deref() == Some(from) {
                o.paragraph_style = Some(to.to_string());
            }
        }
    } else {
        for s in &mut st.character {
            if s.name == from {
                s.name = to.to_string();
            }
            if s.based_on.as_deref() == Some(from) {
                s.based_on = Some(to.to_string());
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

/// Point the nested, nested line and GREP styles of paragraph attributes at a renamed character style.
fn rename_char_refs(p: &mut ParaAttrs, from: &str, to: &str) {
    let names = p.nested_styles.iter_mut().flatten().map(|n| &mut n.style);
    let names = names.chain(p.nested_line_styles.iter_mut().flatten().map(|n| &mut n.style));
    for n in names.chain(p.grep_styles.iter_mut().flatten().map(|g| &mut g.style)) {
        if n == from {
            *n = to.to_string();
        }
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
    let name = str_param(p, "name").unwrap_or("").to_string();
    let repl = str_param(p, "replaceWith").unwrap_or(designcraft_doc::BASIC_PARAGRAPH).to_string();
    if name.starts_with('[') {
        return Err(bad("style.paragraph.delete", "built-in styles can't be deleted"));
    }
    s.edit(|d, _| {
        d.styles_mut().paragraph.retain(|x| x.name != name);
        for sid in d.stories.keys().copied().collect::<Vec<_>>() {
            if let Some(story) = d.story_mut(sid) {
                for f in &mut story.paras {
                    if f.style == name {
                        f.style = repl.clone();
                    }
                }
            }
        }
        ok()
    })
}

fn create_char(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Character Style 1").to_string();
    let chars: CharAttrs = attrs(p.get("chars"), |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
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
    let chars: CharAttrs = attrs(p.get("chars"), |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
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
    let name = str_param(p, "name").unwrap_or("").to_string();
    let repl = str_param(p, "replaceWith").unwrap_or(designcraft_doc::NO_CHAR_STYLE).to_string();
    if name.starts_with('[') {
        return Err(bad("style.character.delete", "built-in styles can't be deleted"));
    }
    s.edit(|d, _| {
        d.styles_mut().character.retain(|x| x.name != name);
        for sid in d.stories.keys().copied().collect::<Vec<_>>() {
            if let Some(story) = d.story_mut(sid) {
                story.for_each_text_mut(&mut |st| {
                    for r in st.chars.iter_mut().filter(|r| r.format.style == name) {
                        r.format.style = repl.clone();
                    }
                });
            }
        }
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
        Value::Array(a) if a.len() >= 3 => {
            let f = |i: usize| a[i].as_f64().map(|x| if x > 1.0 { x / 255.0 } else { x } as f32);
            Some(Color::rgb(f(0)?, f(1)?, f(2)?))
        }
        Value::Object(o) => {
            let g = |k: &str| o.get(k).and_then(Value::as_f64).map(|x| if x > 1.0 { x / 100.0 } else { x } as f32);
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
        (Some(path), None) => std::fs::read(path).map_err(|e| bad("swatch.load", format!("{path}: {e}")))?,
        _ => return Err(bad("swatch.load", "give `path` or `base64`")),
    };
    let incoming = designcraft_color::ase::read(&bytes).map_err(|e| bad("swatch.load", e.to_string()))?;
    let replace = p.get("replace").and_then(Value::as_bool).unwrap_or(false);
    s.edit(|d, _| {
        let (mut added, mut replaced) = (0, 0);
        for w in incoming {
            match d.swatches.iter_mut().find(|x| x.name == w.name) {
                Some(x) if x.locked => {}
                Some(x) => {
                    if replace {
                        x.value = w.value;
                        x.hidden = false;
                        replaced += 1;
                    }
                }
                None => {
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
            std::fs::write(path, &bytes).map_err(|e| bad("swatch.save", format!("{path}: {e}")))?;
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
    let (from, to) = (super::point_param(p, "from"), super::point_param(p, "to"));
    let ids = super::targets(s, p)?;
    s.edit(|d, _| {
        let mut out = Value::Null;
        for id in &ids {
            let Some(it) = d.item(*id) else { continue };
            let cur_name = it.fill.swatch.clone();
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
            it.fill.swatch = name.clone();
            it.fill.tint = 1.0;
            if let Some(a) = angle {
                it.fill.gradient_angle = Some(a);
                it.fill.gradient_vector = None;
            }
            if let (Some(a), Some(b), Some(inv)) = (from, to, inv) {
                let (a, b) = (inv * a, inv * b);
                if (b - a).hypot() > 1e-6 {
                    it.fill.gradient_vector = Some([a.x, a.y, b.x, b.y]);
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
    let ids = super::targets(s, p)?;
    s.edit(|d, _| {
        // Reuse a swatch holding exactly this colour, else add an unnamed one.
        let existing = d
            .swatches
            .iter()
            .find(|w| matches!(&w.value, SwatchValue::Color { color: c, color_type: designcraft_color::ColorType::Process } if *c == color))
            .map(|w| w.name.clone());
        let name = match existing {
            Some(n) => n,
            None => {
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
        };
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                if stroke {
                    it.stroke.swatch = name.clone();
                    it.stroke.tint = 1.0;
                } else {
                    it.fill.swatch = name.clone();
                    it.fill.tint = 1.0;
                }
            }
        }
        Ok(json!({"swatch": name}))
    })
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
    s.edit(|d, _| {
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
        Ok(json!({"name": new}))
    })
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

#[cfg(test)]
mod color_tests {
    use super::*;

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
