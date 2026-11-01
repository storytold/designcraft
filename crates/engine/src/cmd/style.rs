//! Paragraph / character styles and swatches.

use designcraft_color::{Swatch, SwatchValue};
use designcraft_doc::{CharAttrs, CharacterStyle, Effects, Item, ObjectStyle, ParaAttrs, ParagraphStyle, Styles};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, has_text_or_frames, ok, str_param};
use crate::{Result, Session};

mod color;
mod object;
mod paragraph;
mod swatches;

use color::{apply_color, apply_gradient, has_text_or_selection, sample_color};
use object::{apply_object, clear_object_overrides, create_object, edit_object};
use paragraph::{
    apply_char, apply_para, create_char, create_para, delete_char, delete_para, edit_char, edit_para, export_tag_key, group_styles, redefine,
};
pub(crate) use paragraph::{apply_char_style, apply_para_style, check_grep_styles, grep_style_warnings};
pub use swatches::parse_color;
use swatches::{add_to_swatches, create_swatch, load_swatches, new_swatch, save_swatches, swatch_options};

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
            "{style, character?: bool, tag?: p|h1…h6|blockquote|pre|li|… (span|em|strong|code|sup|sub… for characters; \"\" = automatic), class?, split?: bool (paragraphs: Split Document, EPUB only)} → the tagging",
            has_doc,
            |s, p| {
                let name = str_param(p, "style").ok_or_else(|| bad("style.exportTag", "`style` required"))?.to_string();
                let character = p.get("character").and_then(Value::as_bool).unwrap_or(false);
                let exists = if character { s.doc()?.doc.styles.char_style(&name).is_some() } else { s.doc()?.doc.styles.para(&name).is_some() };
                if !exists {
                    return Err(bad("style.exportTag", format!("no style `{name}`")));
                }
                let key = export_tag_key(!character, &name);
                let (tag, class) = (str_param(p, "tag").map(str::to_string), str_param(p, "class").map(str::to_string));
                let split = p.get("split").and_then(Value::as_bool);
                if character && split == Some(true) {
                    return Err(bad("style.exportTag", "`split` is for paragraph styles"));
                }
                s.edit(|d, _| {
                    let st = d.styles_mut();
                    let e = st.export_tags.entry(key.clone()).or_default();
                    if let Some(t) = tag.clone() {
                        e.tag = t;
                    }
                    if let Some(c) = class.clone() {
                        e.class = c;
                    }
                    if let Some(v) = split {
                        e.split = v;
                    }
                    let out = serde_json::to_value(&*e).unwrap_or_default();
                    if *e == designcraft_doc::ExportTag::default() {
                        st.export_tags.remove(&key);
                    }
                    Ok(out)
                })
            }
        ),
        cmd!("style.paragraph.apply", "Apply Paragraph Style", [], None, "{name, clearOverrides?: bool}", has_text_or_frames, apply_para),
        cmd!("style.character.apply", "Apply Character Style", [], None, "{name}", has_text_or_frames, apply_char),
        cmd!(
            "style.paragraph.redefine",
            "Redefine Paragraph Style",
            [],
            None,
            "{name?} — incorporate the first selected paragraph/character's local formatting into its paragraph style; retain character styles; one undo step",
            has_text_or_frames,
            |s, p| redefine(s, p, true)
        ),
        cmd!(
            "style.character.redefine",
            "Redefine Character Style",
            [],
            None,
            "{name?} — incorporate the first selected character's local formatting into its character style without capturing paragraph attributes; one undo step",
            has_text_or_frames,
            |s, p| redefine(s, p, false)
        ),
        cmd!(
            "style.paragraph.create",
            "New Paragraph Style…",
            [],
            None,
            "{name, basedOn?, nextStyle?, para?: {paragraph attributes, as type.para names them: align, leftIndent, firstLineIndent, spaceBefore, spaceAfter, ignoreOpticalMargin, …; startAt?: null = continue numbering; grepStyles?: [{style, pattern}]: a pattern that can't compile is an error}, chars?: {character attributes, as type.char names them: fontFamily, fontStyle, size, leading, tracking, fill, …}, fromSelection?: bool}",
            has_doc,
            create_para
        ),
        cmd!(
            "style.paragraph.edit",
            "Paragraph Style Options…",
            [],
            None,
            "{name, rename?, basedOn?, nextStyle?: style | null ([Same style]), resetToBase?: bool (Reset To Base: the style sets nothing of its own; `para`/`chars` then apply over its Based On), para?: {paragraph attributes, as type.para names them: align, leftIndent, spaceBefore, ignoreOpticalMargin, …; ruleAbove?/ruleBelow?: only the named rule fields change; startAt?: null = continue numbering; kinsoku?: set | null (default rules); a grepStyles pattern that can't compile is an error}, chars?: {character attributes, as type.char names them: fontFamily, fontStyle, size, leading, tracking, fill, …}}",
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
        cmd!(
            "style.object.apply",
            "Apply Object Style",
            [],
            None,
            "{name, ids?} — sets the style's fill, stroke, effects and text frame options on the items",
            super::has_selection,
            apply_object
        ),
        cmd!(
            "style.object.create",
            "New Object Style…",
            [],
            None,
            "{name, fromSelection?: true (fill, stroke, effects and text frame options of the first selected item), fill?, paragraphStyle?, effects?: {dropShadow?, feather?, innerShadow?, outerGlow?, innerGlow?, bevel?, satin?, gradientFeather?, directionalFeather?}, dropShadow?: {on?, color?, opacity? (0–1), angle?, globalLight?, distance?, size?, spread?}, feather?: width, innerShadow?, outerGlow?, innerGlow?, bevel?, satin?, gradientFeather?, directionalFeather?} → {name} (made unique)",
            has_doc,
            create_object
        ),
        cmd!(
            "style.object.edit",
            "Object Style Options…",
            [],
            None,
            "{name, effects?: {…} | null (the style no longer sets effects), dropShadow?, feather?, innerShadow?, outerGlow?, innerGlow?, bevel?, satin?, gradientFeather?, directionalFeather?} — effect objects merge field by field into the style's effects (fields as in `document.inspect` item effects); values that aren't finite numbers are refused, the others clamped (opacities 0–1, sizes and distances ≥ 0); items using the style whose effects match the style's (no effects, when the style set none) take the new effects, items with their own effects keep them → {name, updated}",
            has_doc,
            edit_object
        ),
        cmd!(
            "style.object.clearOverrides",
            "Clear Object Style Overrides",
            [],
            None,
            "{ids?} — each item takes its object style's fill, stroke, effects and text frame options again → {changed}",
            super::has_selection,
            clear_object_overrides
        ),
        cmd!(
            "swatch.create",
            "New Color Swatch…",
            [],
            None,
            "{name?, color: \"#rrggbb\"|{c,m,y,k}(0..100)|[r,g,b](0..255)|{model: rgb|cmyk|gray, …}(0..1), spot?: bool} | {name? (\"New Gradient Swatch\"), gradient: {kind?: linear|radial, stops: [{location: 0–100, swatch?: a colour swatch's name | color?, opacity?: 0–100, midpoint?: 13–87}]}} → {name}",
            has_doc,
            create_swatch
        ),
        cmd!(
            "swatch.new",
            "New Swatch",
            [],
            None,
            "{swatch?: name, target?: fill|stroke} → {name} — the Swatches panel's New Swatch: `swatch` (default: the selection's fill or stroke) as a new swatch. An unnamed colour or gradient becomes a named one (a gradient: \"New Gradient Swatch\"), still applied; a named swatch is duplicated (\"… copy\"); with nothing selected and no `swatch`, a new colour swatch",
            has_doc,
            new_swatch
        ),
        cmd!(
            "object.gradient",
            "Gradient",
            [],
            None,
            "{kind?: linear|radial, stops?: [{location: 0–100, color: \"#rrggbb\"|{c,m,y,k}(0..100)|[r,g,b](0..255)|{model: rgb|cmyk|gray, …}(0..1) | swatch: a color swatch's name (as dropped on the Gradient panel: its color on screen, its inks in print), opacity?: 0–100, midpoint?: 13–87}], reverse?: bool, angle?: degrees, from?: [x,y], to?: [x,y] (spread coords: the Gradient Swatch tool's drag), target?: fill|stroke, ids?} → {swatch, kind, stops: [{location, color, swatch?, opacity, midpoint}]} — edits the fill's (or the stroke's) gradient (an unnamed gradient unless it equals a gradient swatch)",
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
            "{path?|base64?, replace?: bool} — colour swatches from a swatch exchange (.ase) file; a name the file repeats gets a number (\"Red 2\") and an unnamed color is named by its value; names already in the document are kept unless `replace` → {added, replaced}",
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
            "{color: \"#rrggbb\"|{c,m,y,k}(0..100)|[r,g,b](0..255)|{model: rgb|cmyk|gray, r,g,b|c,m,y,k|k}(0..1), target?: fill|stroke, text?: bool, ids?} — an unnamed colour (not in the Swatches panel until Add to Swatches) on the selection (with nothing selected, the colour of new shapes); `text`: on the selected text, or the text of the selected frames (Formatting Affects Text)",
            has_doc,
            apply_color
        ),
        cmd!(
            "color.sample",
            "Sample Color (Eyedropper)",
            [],
            None,
            "{at: [x,y] (spread coords), spread?, target?: fill|stroke, ids?, matchFrom?: id} → {color: \"#rrggbb\", swatch, from: image id} — the Eyedropper on a placed image: the image's colour at the point (an unnamed colour) fills the selected text, else the selected objects. With no opaque image pixel there, `matchFrom` gives the object whose attributes apply instead (object.matchAttributes)",
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
        cmd!(
            "swatch.options",
            "Swatch Options…",
            [],
            None,
            "{name, to?: new name, nameWithValue?: bool, color?: \"#rrggbb\"|{c,m,y,k}(0..100)|[r,g,b]|{gray}(0..100), spot?: bool, tint?: 0–1 (tint swatches), base?: colour swatch (tint swatches)} → {name} — every use follows a rename; [None], [Registration], [Black] and [Paper] keep their names",
            has_doc,
            swatch_options
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
                d.forget_gradient_stop_swatch(&name);
                for g in &mut d.color_groups {
                    g.swatches.retain(|w| *w != name);
                }
                ok()
            })
        }),
        cmd!(query "ink.list", "Ink Manager", [], None, "{} → {allToProcess, inks: [{name, process: bool (printed as process), alias?}]} — the spot inks", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            let inks = designcraft_pdf::inks::ink_set(d);
            let inks: Vec<Value> = d
                .swatches
                .iter()
                .filter(|w| matches!(w.value, designcraft_color::swatch::SwatchValue::Color { color_type: designcraft_color::swatch::ColorType::Spot, .. }))
                .map(|w| {
                    let alias = d.inks.aliases.iter().find(|(a, _)| *a == w.name).map(|(_, t)| t.clone());
                    json!({"name": w.name, "process": designcraft_pdf::inks::spot_output(d, &inks, &w.name).1, "alias": alias})
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
                                let mut set = designcraft_pdf::inks::ink_set(d);
                                let ids = (set.lookup(name), set.lookup(&to));
                                if let (Some(from), Some(target)) = ids
                                    && !set.alias(from, target)
                                {
                                    return Err(bad("ink.options", format!("aliasing `{name}` to `{to}` would make a loop")));
                                }
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

#[cfg(test)]
mod color_tests;

#[cfg(test)]
#[path = "text_color_tests.rs"]
mod text_color_tests;
