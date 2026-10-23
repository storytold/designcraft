//! Paragraph / character styles and swatches.

#[cfg(test)]
mod redefine_tests {
    use super::*;

    fn sample() -> (Session, Value, Value) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect":[36,36,300,200],"content":"text","text":"First header"})).unwrap();
        let b = s.execute("frame.create", &json!({"rect":[36,240,300,400],"content":"text","text":"Second header"})).unwrap();
        s.execute("style.paragraph.create", &json!({"name":"Header","chars":{"size":18,"fill":"[Paper]"}})).unwrap();
        for story in [&a["story"], &b["story"]] {
            s.execute("text.select", &json!({"story":story,"anchor":0,"focus":5})).unwrap();
            s.execute("style.paragraph.apply", &json!({"name":"Header"})).unwrap();
        }
        s.execute("text.select", &json!({"story":a["story"],"anchor":0,"focus":5})).unwrap();
        (s, a["story"].clone(), b["story"].clone())
    }

    #[test]
    fn redefine_paragraph_updates_other_stories_and_clears_the_source_overrides() {
        let (mut s, a, b) = sample();
        s.execute("style.character.create", &json!({"name":"Emphasis","chars":{"underline":true}})).unwrap();
        s.execute("style.character.apply", &json!({"name":"Emphasis"})).unwrap();
        s.execute("type.char", &json!({"size":20,"fill":"[Black]"})).unwrap();
        s.execute("type.para", &json!({"spaceAfter":9})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        s.execute("style.paragraph.redefine", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        assert!(s.doc().unwrap().doc.styles.para("Header").unwrap().chars.underline.is_none());
        for story in [&a, &b] {
            s.execute("text.select", &json!({"story":story,"anchor":0,"focus":5})).unwrap();
            let attrs = s.execute("type.selectionAttrs", &json!({})).unwrap();
            assert_eq!(attrs["chars"]["size"], 20.0);
            assert_eq!(attrs["chars"]["fill"], "[Black]");
            assert_eq!(attrs["para"]["spaceAfter"], 9.0);
            assert_eq!(attrs["chars"]["underline"], story == &a);
        }
        s.execute("style.paragraph.edit", &json!({"name":"Header","chars":{"size":22}})).unwrap();
        s.execute("text.select", &json!({"story":a,"anchor":0,"focus":5})).unwrap();
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"], 22.0);
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
    }

    #[test]
    fn redefine_character_does_not_capture_paragraph_font_size() {
        let (mut s, a, b) = sample();
        s.execute("style.character.create", &json!({"name":"Underline","chars":{"underline":true}})).unwrap();
        for story in [&a, &b] {
            s.execute("text.select", &json!({"story":story,"anchor":0,"focus":5})).unwrap();
            s.execute("style.character.apply", &json!({"name":"Underline"})).unwrap();
        }
        s.execute("type.char", &json!({"fill":"[Black]"})).unwrap();
        s.execute("style.character.redefine", &json!({"name":"Underline"})).unwrap();
        let style = s.doc().unwrap().doc.styles.char_style("Underline").unwrap();
        assert!(style.chars.size.is_none());
        assert_eq!(style.chars.underline, Some(true));
        for story in [&a, &b] {
            s.execute("text.select", &json!({"story":story,"anchor":0,"focus":5})).unwrap();
            assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], "[Black]");
        }
    }

    #[test]
    fn redefine_rejects_the_wrong_style_and_the_none_entry_atomically() {
        let (mut s, _, _) = sample();
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        assert!(s.execute("style.paragraph.redefine", &json!({"name":"Other"})).is_err());
        assert!(s.execute("style.character.redefine", &json!({})).is_err());
        assert_eq!(s.doc().unwrap().doc, before);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }

    #[test]
    fn redefine_character_at_a_caret_cleans_the_source_run_but_keeps_other_overrides() {
        let (mut s, story, _) = sample();
        s.execute("style.character.create", &json!({"name":"Emphasis","chars":{"underline":true}})).unwrap();
        for (start, end, size) in [(0, 5, 24), (6, 12, 32)] {
            s.execute("text.select", &json!({"story":story,"anchor":start,"focus":end})).unwrap();
            s.execute("style.character.apply", &json!({"name":"Emphasis"})).unwrap();
            s.execute("type.char", &json!({"size":size})).unwrap();
        }
        let before = s.doc().unwrap().doc.clone();
        s.execute("text.select", &json!({"story":story,"anchor":2,"focus":2})).unwrap();
        s.execute("style.character.redefine", &json!({"name":"Emphasis"})).unwrap();
        s.execute("style.character.edit", &json!({"name":"Emphasis","chars":{"size":30}})).unwrap();
        for (start, end, size) in [(0, 5, 30.0), (6, 12, 32.0)] {
            s.execute("text.select", &json!({"story":story,"anchor":start,"focus":end})).unwrap();
            assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"], size);
        }
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
    }
}

#[cfg(test)]
mod application_inheritance_tests {
    use super::*;

    fn setup(text: &str) -> (Session, Value) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [36,36,300,300], "content": "text", "text": text})).unwrap();
        (s, f["story"].clone())
    }
    fn select(s: &mut Session, story: &Value, start: usize, end: usize) {
        s.execute("text.select", &json!({"story": story, "anchor": start, "focus": end})).unwrap();
    }
    fn attrs(s: &mut Session) -> Value {
        s.execute("type.selectionAttrs", &json!({})).unwrap()
    }

    #[test]
    fn paragraph_capture_source_follows_later_style_edits_and_undo() {
        let (mut s, story) = setup("First\nSecond");
        select(&mut s, &story, 0, 5);
        s.execute("type.char", &json!({"size": 18, "fill": "[Paper]"})).unwrap();
        s.execute("type.para", &json!({"spaceAfter": 6})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Header", "fromSelection": true})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        s.execute("style.paragraph.apply", &json!({"name": "Header"})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
        s.execute("edit.redo", &json!({})).unwrap();
        select(&mut s, &story, 6, 6);
        s.execute("style.paragraph.apply", &json!({"name": "Header"})).unwrap();
        s.execute("style.paragraph.edit", &json!({"name": "Header", "chars": {"size": 20, "fill": "[Black]"}, "para": {"spaceAfter": 12}})).unwrap();
        for pos in [0, 6] {
            select(&mut s, &story, pos, pos + 1);
            let a = attrs(&mut s);
            assert_eq!(a["chars"]["size"], 20.0);
            assert_eq!(a["chars"]["fill"], "[Black]");
            assert_eq!(a["para"]["spaceAfter"], 12.0);
        }
    }

    #[test]
    fn applying_styles_keeps_different_overrides_and_named_character_styles() {
        let (mut s, story) = setup("First\nSecond");
        select(&mut s, &story, 0, 5);
        s.execute("type.char", &json!({"size": 14})).unwrap();
        s.execute("type.para", &json!({"spaceAfter": 9})).unwrap();
        s.execute("style.character.create", &json!({"name": "Emphasis", "chars": {"underline": true}})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Emphasis"})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body", "chars": {"size": 10}, "para": {"spaceAfter": 6}})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Body"})).unwrap();
        let a = attrs(&mut s);
        assert_eq!(a["chars"]["size"], 14.0);
        assert_eq!(a["para"]["spaceAfter"], 9.0);
        assert_eq!(a["characterStyle"], "Emphasis");
        assert_eq!(a["chars"]["underline"], true);
    }

    #[test]
    fn character_capture_source_remains_sparse_and_follows_style_edits() {
        let (mut s, story) = setup("First\nSecond");
        select(&mut s, &story, 0, 5);
        s.execute("type.char", &json!({"underline": true})).unwrap();
        s.execute("style.character.create", &json!({"name": "Emphasis", "fromSelection": true})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Emphasis"})).unwrap();
        s.execute("style.character.edit", &json!({"name": "Emphasis", "chars": {"underline": false}})).unwrap();
        assert_eq!(attrs(&mut s)["chars"]["underline"], false);
        assert!(s.doc().unwrap().doc.styles.char_style("Emphasis").unwrap().chars.size.is_none());
        s.execute("style.paragraph.edit", &json!({"name": designcraft_doc::BASIC_PARAGRAPH, "chars": {"size": 20}})).unwrap();
        assert_eq!(attrs(&mut s)["chars"]["size"], 20.0);
    }

    #[test]
    fn paragraph_selection_excludes_the_next_paragraph_at_its_start() {
        let (mut s, story) = setup("é\nNext");
        select(&mut s, &story, 0, 3);
        s.execute("style.paragraph.create", &json!({"name": "Header"})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Header", "clearOverrides": true})).unwrap();
        select(&mut s, &story, 3, 4);
        assert_eq!(attrs(&mut s)["paragraphStyle"], designcraft_doc::BASIC_PARAGRAPH);
        select(&mut s, &story, 0, 2);
        assert_eq!(attrs(&mut s)["paragraphStyle"], "Header");
    }

    #[test]
    fn clearing_a_partial_selection_preserves_character_overrides_outside_it() {
        let (mut s, story) = setup("First Second");
        select(&mut s, &story, 0, 12);
        s.execute("type.char", &json!({"size": 18})).unwrap();
        select(&mut s, &story, 0, 5);
        s.execute("style.paragraph.apply", &json!({"name": designcraft_doc::BASIC_PARAGRAPH, "clearOverrides": true})).unwrap();
        assert_ne!(attrs(&mut s)["chars"]["size"], 18.0);
        select(&mut s, &story, 6, 12);
        assert_eq!(attrs(&mut s)["chars"]["size"], 18.0);
    }
}

use designcraft_color::{Swatch, SwatchValue};
use designcraft_doc::{CharAttrs, CharacterStyle, Effects, Item, ObjectStyle, ParaAttrs, ParagraphStyle, Styles};
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
            "{name, basedOn?, nextStyle?, para?: {paragraph attributes, as type.para names them: align, leftIndent, firstLineIndent, spaceBefore, spaceAfter, …; startAt?: null = continue numbering; grepStyles?: [{style, pattern}]: a pattern that can't compile is an error}, chars?: {character attributes, as type.char names them: fontFamily, fontStyle, size, leading, tracking, fill, …}, fromSelection?: bool}",
            has_doc,
            create_para
        ),
        cmd!(
            "style.paragraph.edit",
            "Paragraph Style Options…",
            [],
            None,
            "{name, rename?, basedOn?, nextStyle?, para?: {paragraph attributes, as type.para names them: align, leftIndent, spaceBefore, …; ruleAbove?/ruleBelow?: only the named rule fields change; startAt?: null = continue numbering; kinsoku?: set | null (default rules); a grepStyles pattern that can't compile is an error}, chars?: {character attributes, as type.char names them: fontFamily, fontStyle, size, leading, tracking, fill, …}}",
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

fn redefine(s: &mut Session, p: &Value, paragraph: bool) -> Result<Value> {
    let command = if paragraph { "style.paragraph.redefine" } else { "style.character.redefine" };
    let targets = super::text::format_targets_pub(s);
    let target = targets.first().ok_or_else(|| bad(command, "select text using the style to redefine"))?;
    let state = s.doc()?;
    let story = state.doc.text_story(target.story, target.cell).ok_or(designcraft_doc::DocError::NoStory(target.story))?;
    let pf = story.paras.get(story.para_at(target.range.start)).ok_or_else(|| bad(command, "no paragraph at the selection"))?;
    let cf = super::text::effective_char_format(s, target)?;
    let selected = if paragraph { &pf.style } else { &cf.style };
    let name = str_param(p, "name").unwrap_or(selected).to_string();
    if name != *selected || name == if paragraph { designcraft_doc::NO_PARA_STYLE } else { designcraft_doc::NO_CHAR_STYLE } {
        return Err(bad(command, "select text using the named style; the no-style entry cannot be redefined"));
    }
    let (base_para, base_chars) = state.doc.styles.resolve_para_style(&pf.style);
    let (resolved_para, paragraph_chars) = state.doc.styles.resolve_para(pf);
    let para_changes = base_para.delta(&resolved_para);
    let mut current = if paragraph {
        paragraph_chars.clone()
    } else {
        state.doc.styles.resolve_char(&paragraph_chars, &designcraft_doc::CharFormat { style: name.clone(), over: CharAttrs::default() })
    };
    let inherited = if paragraph { base_chars } else { current.clone() };
    // A named character style is an intentional layer, not a paragraph override to absorb.
    current.apply(&cf.over);
    let char_changes = inherited.delta(&current);
    let result = s.edit(|d, _| {
        if paragraph {
            let style = d.styles_mut().para_mut(&name).ok_or_else(|| bad(command, "paragraph style no longer exists"))?;
            style.para.merge(&para_changes);
            style.chars.merge(&char_changes);
        } else {
            let style =
                d.styles_mut().character.iter_mut().find(|st| st.name == name).ok_or_else(|| bad(command, "character style no longer exists"))?;
            style.chars.merge(&char_changes);
        }
        let styles = d.styles.clone();
        for target in &targets {
            let Some(story) = d.text_story_mut(target.story, target.cell) else { continue };
            // At a character-style caret, the sampled format belongs to the containing run
            // (the preceding run at a boundary). Normalize that source run too, otherwise its
            // old local values would mask the next edit of the redefined style.
            let mut selected_range = target.range.clone();
            if !paragraph && selected_range.is_empty() {
                let caret = selected_range.start;
                let mut start = 0usize;
                for run in &story.chars {
                    let end = start.saturating_add(run.len).min(story.len());
                    if caret < end || (caret == end && caret > start) {
                        selected_range = start..end;
                        break;
                    }
                    start = end;
                }
            }
            let end = if selected_range.is_empty() { selected_range.start } else { selected_range.end.saturating_sub(1) };
            let (first, last) = (story.para_at(selected_range.start), story.para_at(end));
            for (index, range) in story.para_ranges().into_iter().enumerate().take(last.saturating_add(1)).skip(first) {
                let Some(pf) = story.paras.get_mut(index) else { continue };
                if paragraph && pf.style != name {
                    continue;
                }
                if paragraph {
                    let (pp, cp) = styles.resolve_para(pf);
                    let (base_p, base_c) = styles.resolve_para_style(&name);
                    pf.para = base_p.delta(&pp);
                    pf.chars = base_c.delta(&cp);
                }
                let (_, base) = styles.resolve_para(pf);
                let end = range.end.saturating_add(1).min(story.len());
                let range = if paragraph { range.start..end } else { range.start.max(selected_range.start)..end.min(selected_range.end) };
                story.format_chars(range, |format| {
                    if !paragraph && format.style != name {
                        return;
                    }
                    let mut inherited = base.clone();
                    inherited.apply(&styles.char_style_attrs(&format.style));
                    let resolved = styles.resolve_char(&base, format);
                    format.over = inherited.delta(&resolved);
                });
            }
        }
        Ok(json!({"name":name}))
    });
    if result.is_ok() {
        s.typing_format = None;
    }
    result
}

fn apply_para(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.paragraph.apply", "missing name"))?.to_string();
    let clear = p.get("clearOverrides").and_then(Value::as_bool).unwrap_or(false);
    if s.doc()?.doc.styles.para(&name).is_none() {
        return Err(bad("style.paragraph.apply", format!("no paragraph style `{name}`")));
    }
    let targets = super::text::format_targets_pub(s);
    s.edit(|d, _| {
        let styles = d.styles.clone();
        let (style_para, style_chars) = styles.resolve_para_style(&name);
        for t in &targets {
            let r = &t.range;
            if let Some(st) = d.text_story_mut(t.story, t.cell) {
                // Text selections are half-open: a selection ending at the next paragraph's
                // start must not apply a style to that paragraph too.
                let end = if r.is_empty() { r.start } else { r.end.saturating_sub(1) };
                let (first, last) = (st.para_at(r.start), st.para_at(end));
                st.format_paras(r.start..end, |f| {
                    f.style = name.clone();
                    if clear {
                        f.para = ParaAttrs::default();
                        f.chars = CharAttrs::default();
                    } else {
                        f.para.retain_differences(&style_para);
                        f.chars.retain_differences(&style_chars);
                    }
                });
                for (pi, range) in st.para_ranges().into_iter().enumerate().take(last.saturating_add(1)).skip(first) {
                    let Some(pf) = st.paras.get(pi) else { continue };
                    let (_, base) = styles.resolve_para(pf);
                    let end = range.end.saturating_add(1).min(st.len());
                    let range = if clear && !r.is_empty() { range.start.max(r.start)..end.min(r.end) } else { range.start..end };
                    st.format_chars(range, |f| {
                        if clear {
                            f.over = CharAttrs::default();
                        } else {
                            let mut inherited = base.clone();
                            inherited.apply(&styles.char_style_attrs(&f.style));
                            f.over.retain_differences(&inherited);
                        }
                    });
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
        let styles = d.styles.clone();
        for t in &targets {
            if let Some(st) = d.text_story_mut(t.story, t.cell) {
                for (pi, range) in st.para_ranges().into_iter().enumerate() {
                    let range = range.start.max(t.range.start)..range.end.saturating_add(1).min(st.len()).min(t.range.end);
                    if range.is_empty() {
                        continue;
                    }
                    let Some(pf) = st.paras.get(pi) else { continue };
                    let (_, mut inherited) = styles.resolve_para(pf);
                    inherited.apply(&styles.char_style_attrs(&name));
                    st.format_chars(range, |f| {
                        f.style = name.clone();
                        f.over.retain_differences(&inherited);
                    });
                }
            }
        }
        ok()
    })
}

/// In a style, `startAt: null` is Continue from Previous Number: it is stored as an explicit
/// "continue" so it also overrides a Start At the style would inherit.
fn continue_numbering(p: Option<&Value>, para: &mut ParaAttrs) {
    let explicit =
        p.and_then(Value::as_object).is_some_and(|o| o.iter().any(|(k, v)| v.is_null() && k.replace('_', "").eq_ignore_ascii_case("startAt")));
    if explicit {
        para.start_at = Some(None);
    }
}

fn create_para(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Paragraph Style 1").to_string();
    let mut para: ParaAttrs =
        attrs("style.paragraph.create", "para", p, |a: &mut ParaAttrs, k, v| a.set_json_over(k, v, &designcraft_doc::ParaProps::default()))?;
    continue_numbering(p.get("para"), &mut para);
    explicit_default_kinsoku(&mut para, p.get("para"));
    check_grep_styles("style.paragraph.create", &para)?;
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

/// `kinsoku: null` sets the composer's default kinsoku rules, also over a Based On style's set.
/// For the other attributes `null` leaves the style's value as it is.
fn explicit_default_kinsoku(para: &mut ParaAttrs, given: Option<&Value>) {
    if given.and_then(|o| o.get("kinsoku")).is_some_and(Value::is_null) {
        para.kinsoku = Some(None);
    }
}

/// The compile error of the first GREP style in `a` whose pattern can't compile.
pub(crate) fn grep_style_error(a: &ParaAttrs) -> Option<String> {
    a.grep_styles
        .iter()
        .flatten()
        .filter(|g| !g.pattern.is_empty())
        .find_map(|g| designcraft_compose::grep::Grep::new(&g.pattern, Default::default()).err().map(|e| format!("{e} (pattern {:?})", g.pattern)))
}

/// Rejects GREP styles a command would set with a pattern that can't compile.
pub(crate) fn check_grep_styles(cmd: &str, a: &ParaAttrs) -> Result<()> {
    grep_style_error(a).map_or(Ok(()), |e| Err(bad(cmd, e)))
}

/// One warning for each paragraph style or paragraph whose GREP style pattern can't compile.
/// Such a GREP style stays in the document and composition skips it.
pub(crate) fn grep_style_warnings(d: &designcraft_doc::Document) -> Vec<String> {
    let mut out: Vec<String> = d
        .styles
        .paragraph
        .iter()
        .filter_map(|st| grep_style_error(&st.para).map(|e| format!("paragraph style {:?}: GREP style not applied: {e}", st.name)))
        .collect();
    for st in d.stories.values() {
        for (i, pf) in st.paras.iter().enumerate() {
            if let Some(e) = grep_style_error(&pf.para) {
                out.push(format!("story {} paragraph {}: GREP style not applied: {e}", st.id.0, i + 1));
            }
        }
    }
    out
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
        let mut para: ParaAttrs = attrs("style.paragraph.edit", "para", p, |a: &mut ParaAttrs, k, v| a.set_json_over(k, v, &current))?;
        continue_numbering(p.get("para"), &mut para);
        explicit_default_kinsoku(&mut para, p.get("para"));
        check_grep_styles("style.paragraph.edit", &para)?;
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
    let rename = |r: &mut String| {
        if r == from {
            *r = to.to_string();
        }
    };
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
        let objects = st.object.iter_mut().map(|o| &mut o.paragraph_style);
        for r in objects.chain(st.cell.iter_mut().map(|c| &mut c.paragraph_style)) {
            if r.as_deref() == Some(from) {
                *r = Some(to.to_string());
            }
        }
        rename(&mut st.default_paragraph);
        let (fo, eo) = (&mut d.footnote_options, &mut d.endnote_options);
        [&mut fo.para_style, &mut eo.para_style, &mut eo.heading_style].into_iter().for_each(rename);
        d.toc.iter_mut().flat_map(|t| t.entries.iter_mut().map(|e| &mut e.style)).for_each(rename);
        for t in d.toc.iter_mut() {
            let formats = t.entries.iter_mut().map(|e| &mut e.entry_style).chain([&mut t.title_style]);
            for r in formats.filter(|r| r.as_deref() == Some(from)) {
                *r = Some(to.to_string());
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
        rename(&mut st.default_character);
        rename(&mut d.footnote_options.ref_char_style);
    }
    // The table of contents' character styles ([None] is stored as no style).
    if !para && let Some(toc) = d.toc.as_mut() {
        for e in &mut toc.entries {
            for n in [&mut e.page_number_style, &mut e.separator_style] {
                if n.as_deref() == Some(from) {
                    *n = Some(to.to_string()).filter(|t| t != designcraft_doc::NO_CHAR_STYLE);
                }
            }
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
            st.rev = st.rev.wrapping_add(1);
        });
    }
}

/// Point the nested, nested line and GREP styles, and the drop cap style, of paragraph
/// attributes at a renamed (or deleted, pointed at its replacement) character style.
fn rename_char_refs(p: &mut ParaAttrs, from: &str, to: &str) {
    let names = p.nested_styles.iter_mut().flatten().map(|n| &mut n.style);
    let names = names.chain(p.nested_line_styles.iter_mut().flatten().map(|n| &mut n.style));
    let names = names.chain(p.drop_cap_style.iter_mut());
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
    delete_style(s, p, true)
}

fn delete_char(s: &mut Session, p: &Value) -> Result<Value> {
    delete_style(s, p, false)
}

/// Delete a paragraph (`para`) or character style; everything that used it uses `replaceWith`
/// (default [Basic Paragraph] or [None]) instead, in one undo step.
fn delete_style(s: &mut Session, p: &Value, para: bool) -> Result<Value> {
    let (id, default) =
        if para { ("style.paragraph.delete", designcraft_doc::BASIC_PARAGRAPH) } else { ("style.character.delete", designcraft_doc::NO_CHAR_STYLE) };
    let name = str_param(p, "name").filter(|n| !n.is_empty()).ok_or_else(|| bad(id, "missing name"))?.to_string();
    let repl = str_param(p, "replaceWith").unwrap_or(default).to_string();
    if name.starts_with('[') {
        return Err(bad(id, "built-in styles can't be deleted"));
    }
    if repl == name {
        return Err(bad(id, "`replaceWith` can't be the deleted style"));
    }
    s.edit(|d, _| {
        let st = &d.styles;
        let exists = |n: &str| if para { st.para(n).is_some() } else { n == designcraft_doc::NO_CHAR_STYLE || st.char_style(n).is_some() };
        if !exists(&name) {
            return Err(bad(id, format!("no style `{name}`")));
        }
        if !exists(&repl) {
            return Err(bad(id, format!("no style `{repl}` to replace it with")));
        }
        // The deleted style's children are based on the replacement, or on the deleted style's
        // own parent when the replacement descends from it (so no based-on chain loops).
        let (own_parent, descends) = if para {
            (st.para(&name).and_then(|x| x.based_on.clone()), st.para_based_on_cycles(&name, &repl))
        } else {
            (st.char_style(&name).and_then(|x| x.based_on.clone()), st.char_based_on_cycles(&name, &repl))
        };
        let parent = if descends {
            own_parent.filter(|b| *b != name)
        } else if repl == designcraft_doc::NO_CHAR_STYLE || repl == designcraft_doc::NO_PARA_STYLE {
            None
        } else {
            Some(repl.clone())
        };
        let st = d.styles_mut();
        let based = if para {
            st.paragraph.retain(|x| x.name != name);
            st.paragraph.iter_mut().map(|x| &mut x.based_on).collect::<Vec<_>>()
        } else {
            st.character.retain(|x| x.name != name);
            st.character.iter_mut().map(|x| &mut x.based_on).collect()
        };
        for b in based.into_iter().filter(|b| b.as_deref() == Some(name.as_str())) {
            *b = parent.clone();
        }
        // Every other use: styles, stories, table cells, nested and GREP styles, notes, defaults.
        rename_style(d, para, &name, &repl);
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

fn create_swatch(s: &mut Session, p: &Value) -> Result<Value> {
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
fn new_swatch(s: &mut Session, p: &Value) -> Result<Value> {
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

fn apply_object(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.object.apply", "missing name"))?.to_string();
    let os = s.doc()?.doc.styles.object_style(&name).cloned().ok_or_else(|| bad("style.object.apply", format!("no object style `{name}`")))?;
    let ids = super::targets(s, p)?;
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

fn clear_object_overrides(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = super::targets(s, p)?;
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

fn create_object(s: &mut Session, p: &Value) -> Result<Value> {
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

fn edit_object(s: &mut Session, p: &Value) -> Result<Value> {
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

/// The swatch name of a colour value ("C=… M=… Y=… K=…" / "R=… G=… B=…").
fn value_name(color: designcraft_color::Color) -> String {
    designcraft_color::swatch::value_name(color)
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
    // Nothing selected: the colour of new shapes (InDesign's document defaults).
    if !text && super::object::nothing_targeted(s, p)? {
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

fn has_text_or_selection(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.text.is_some() || d.selection.cells.is_some()) { Ok(()) } else { super::has_selection(s) }
}

/// The Eyedropper on a placed image (InDesign samples the pixel under the pointer): that colour,
/// as an unnamed colour, fills the selected text (characters), or the selected objects.
fn sample_color(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "color.sample";
    let at = super::point_param(p, "at").ok_or_else(|| bad(ID, "missing at"))?;
    let sr = super::spread_param(p, ID)?;
    let st = s.doc()?;
    let Some((from, color)) = image_color_at(&st.doc, sr, at) else {
        // No opaque image pixel there (an empty part of an image frame, a transparent pixel): the
        // Eyedropper applies the clicked object's attributes instead.
        return match p.get("matchFrom") {
            Some(from) => {
                let mut params = p.clone();
                params["from"] = from.clone();
                super::object::match_attributes(s, &params)
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
        check_text_nesting(d, "swatch.addToSwatches")?;
        let w = d.swatches.iter_mut().find(|w| w.name == name).ok_or_else(|| bad("swatch.addToSwatches", format!("no swatch `{name}`")))?;
        w.name = new.clone();
        w.named = true;
        d.rename_swatch_refs(&name, &new);
        Ok(json!({"name": new}))
    })?;
    if let Some(new) = result.get("name").and_then(Value::as_str) {
        super::text::rename_typing_swatch(s, &name, new);
    }
    Ok(result)
}

/// Renaming a swatch walks every text inside a story (table cells, notes): refuse the whole edit
/// for nesting deeper than 32 levels rather than leave deeper references at a removed name.
fn check_text_nesting(d: &designcraft_doc::Document, id: &str) -> Result<()> {
    fn within(st: &designcraft_doc::Story, depth: usize) -> bool {
        depth <= 32
            && st.tables.values().all(|t| t.cells.iter().all(|c| within(&c.text, depth + 1)))
            && st.notes.iter().chain(&st.endnotes).all(|n| within(&n.text, depth + 1))
    }
    if d.stories.values().all(|st| within(st, 0)) { Ok(()) } else { Err(bad(id, "story nesting exceeds 32 levels")) }
}

/// Swatch Options: rename, Name with Color Value, colour type and value, a tint swatch's tint
/// and base — one undo step. Every use of the swatch follows a rename.
fn swatch_options(s: &mut Session, p: &Value) -> Result<Value> {
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
        super::text::rename_typing_swatch(s, &name, new);
    }
    Ok(result)
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
                let mut params = json!({"name": "Existing", "chars": {"fill": fill, "size": 24}});
                // Only the edit commands take `rename`.
                if command.ends_with(".edit") {
                    params["rename"] = json!("Renamed");
                }
                assert_fill_rejected(&mut session, command, params);
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
    fn a_colour_with_a_model_is_read_as_fractions_without_guessing() {
        use designcraft_color::Color;
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [id]})).unwrap();
        let fill = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            d.resolve_color(&d.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, 1.0).unwrap()
        };
        // M 1 % from the Color panel: not M 100 % (magenta), though every component is within 0..1.
        s.execute("object.color", &json!({"color": {"model": "cmyk", "c": 0, "m": 0.01, "y": 0, "k": 0}})).unwrap();
        let Color::Cmyk { c, m, y, k } = fill(&s) else { panic!("not CMYK") };
        assert!(c == 0.0 && (m - 0.01).abs() < 1e-6 && y == 0.0 && k == 0.0, "got {c} {m} {y} {k}");
        // R 1 of 255 is a near black, not pure red.
        s.execute("object.color", &json!({"color": {"model": "rgb", "r": 1.0 / 255.0, "g": 0, "b": 0}})).unwrap();
        let Color::Rgb { r, g, b } = fill(&s) else { panic!("not RGB") };
        assert!((r - 1.0 / 255.0).abs() < 1e-6 && g == 0.0 && b == 0.0, "got {r} {g} {b}");
        // Out of range or not a model: refused rather than guessed.
        assert!(s.execute("object.color", &json!({"color": {"model": "cmyk", "c": 0, "m": 50, "y": 0, "k": 0}})).is_err());
        assert!(s.execute("object.color", &json!({"color": {"model": "hsb", "h": 0, "s": 0, "b": 0}})).is_err());
    }

    #[test]
    fn colours_that_round_to_the_same_name_share_one_swatch() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [id]})).unwrap();
        let n = |s: &Session| s.doc().unwrap().doc.swatches.len();
        let first = s.execute("object.color", &json!({"color": {"model": "cmyk", "c": 0.1, "m": 0.5, "y": 1, "k": 0}})).unwrap();
        let before = n(&s);
        for m in [0.501, 0.499, 0.5041] {
            let r = s.execute("object.color", &json!({"color": {"model": "cmyk", "c": 0.1, "m": m, "y": 1, "k": 0}})).unwrap();
            assert_eq!(r["swatch"], first["swatch"], "M {m}");
        }
        s.execute("object.color", &json!({"color": {"model": "rgb", "r": 0.5, "g": 0.25, "b": 0}})).unwrap();
        s.execute("object.color", &json!({"color": {"model": "rgb", "r": 0.5005, "g": 0.25, "b": 0}})).unwrap();
        assert_eq!(n(&s), before + 1, "no hidden twins for values a fraction of a step apart");
        // A whole percent apart is a different colour.
        let r = s.execute("object.color", &json!({"color": {"model": "cmyk", "c": 0.1, "m": 0.51, "y": 1, "k": 0}})).unwrap();
        assert_eq!(r["swatch"], "C=10 M=51 Y=100 K=0");
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
    fn continue_numbering_in_a_style_overrides_an_inherited_start_at() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Steps", "para": {"listType": "numbers", "startAt": 5}})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "More Steps", "basedOn": "Steps"})).unwrap();
        let start = |s: &Session, name: &str| s.doc().unwrap().doc.styles.resolve_para_style(name).0.start_at;
        assert_eq!(start(&s, "More Steps"), Some(5), "inherited");
        s.execute("style.paragraph.edit", &json!({"name": "More Steps", "para": {"startAt": null}})).unwrap();
        assert_eq!(s.doc().unwrap().doc.styles.para("More Steps").unwrap().para.start_at, Some(None));
        assert_eq!(start(&s, "More Steps"), None, "continues");
        assert_eq!(start(&s, "Steps"), Some(5), "the parent keeps its Start At");
        s.execute("style.paragraph.edit", &json!({"name": "Steps", "para": {"startAt": null}})).unwrap();
        assert_eq!(start(&s, "Steps"), None);
        s.execute("style.paragraph.create", &json!({"name": "Fresh", "basedOn": "More Steps", "para": {"start_at": null}})).unwrap();
        assert_eq!(s.doc().unwrap().doc.styles.para("Fresh").unwrap().para.start_at, Some(None));
    }

    #[test]
    fn null_kinsoku_sets_the_default_rules_over_a_based_on_set() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let hard = serde_json::to_value(designcraft_doc::cjk::Kinsoku::named("HardKinsoku").unwrap()).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Base", "para": {"kinsoku": hard}})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body", "basedOn": "Base", "para": {"kinsoku": hard}})).unwrap();
        s.execute("style.paragraph.edit", &json!({"name": "Body", "para": {"kinsoku": null}})).unwrap();
        let styles = s.doc().unwrap().doc.styles.clone();
        assert_eq!(styles.para("Body").unwrap().para.kinsoku, Some(None));
        assert_eq!(styles.resolve_para_style("Body").0.kinsoku, None, "Base's set no longer applies");
        // An edit without the attribute keeps it.
        s.execute("style.paragraph.edit", &json!({"name": "Body", "para": {"rensuuji": false}})).unwrap();
        assert_eq!(s.doc().unwrap().doc.styles.para("Body").unwrap().para.kinsoku, Some(None));
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
        // With `replace`, the file's second "New" is added as "New 2", only the first "Twin" changes, the hidden
        // swatch becomes visible, and the locked [Black] stays.
        let r = s.execute("swatch.load", &json!({"base64": b64, "replace": true})).unwrap();
        assert_eq!((r["added"].as_u64(), r["replaced"].as_u64()), (Some(2), Some(2)));
        assert_eq!(named(&s, "New"), [(ink(0.4), false)]);
        assert_eq!(named(&s, "New 2"), [(ink(0.5), false)]);
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
    fn loading_swatches_numbers_repeated_names_and_names_unnamed_colors() {
        use designcraft_color::{Color, ColorType};
        let ink = |k: f32| SwatchValue::Color { color: Color::cmyk(0.0, 0.0, 0.0, k), color_type: ColorType::Process };
        let sw = |name: &str, k: f32| Swatch { name: name.into(), value: ink(k), locked: false, named: true, hidden: false };
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.edit(|d, _| {
            d.swatches.push(sw("Kept", 0.1));
            Ok(())
        })
        .unwrap();
        // "Red 2" comes before the second "Red", which takes "Red 3". Two unnamed colors share a value.
        let file = designcraft_color::ase::write(&[
            sw("Red", 0.2),
            sw("Red 2", 0.3),
            sw("Red", 0.4),
            sw("", 0.55),
            sw("", 0.55),
            sw("", 0.65),
            sw("Kept", 0.7),
        ]);
        let b64 = crate::cmd::base64_encode(&file);
        let named = |s: &Session, name: &str| -> Vec<(SwatchValue, bool)> {
            s.doc().unwrap().doc.swatches.iter().filter(|w| w.name == name).map(|w| (w.value.clone(), w.named)).collect()
        };
        // Names already in the document are kept.
        let r = s.execute("swatch.load", &json!({"base64": b64})).unwrap();
        assert_eq!((r["added"].as_u64(), r["replaced"].as_u64()), (Some(6), Some(0)));
        assert_eq!(named(&s, "Red"), [(ink(0.2), true)]);
        assert_eq!(named(&s, "Red 2"), [(ink(0.3), true)]);
        assert_eq!(named(&s, "Red 3"), [(ink(0.4), true)]);
        assert_eq!(named(&s, "C=0 M=0 Y=0 K=55"), [(ink(0.55), false)]);
        assert_eq!(named(&s, "C=0 M=0 Y=0 K=55 2"), [(ink(0.55), false)]);
        assert_eq!(named(&s, "C=0 M=0 Y=0 K=65"), [(ink(0.65), false)]);
        assert_eq!(named(&s, "Kept"), [(ink(0.1), true)]);
        assert!(named(&s, "").is_empty());
        // With `replace`, the same file again replaces each of its colors once.
        let r = s.execute("swatch.load", &json!({"base64": b64, "replace": true})).unwrap();
        assert_eq!((r["added"].as_u64(), r["replaced"].as_u64()), (Some(0), Some(7)));
        assert_eq!(named(&s, "Red"), [(ink(0.2), true)]);
        assert_eq!(named(&s, "Red 3"), [(ink(0.4), true)]);
        assert_eq!(named(&s, "Kept"), [(ink(0.7), true)]);
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
        s.execute("endnote.insert", &json!({"text": "end"})).unwrap();
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
            Arc::make_mut(&mut st.endnotes[0]).text.chars[0].format.over.underline_color = Some(old.clone());
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
        assert_eq!(st.endnotes[0].text.chars[0].format.over.underline_color.as_deref(), Some("Sand"));
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
        assert!(designcraft_pdf::inks::spot_output(&back, &designcraft_pdf::inks::ink_set(&back), "Pantone A").1, "IDML ConvertToProcess");
        assert!(back.inks.aliases.iter().any(|(a, b)| a == "Pantone A" && b == "Pantone B"), "IDML AliasInkName");
        assert!(s.execute("ink.options", &json!({"ink": "[Black]", "toProcess": true})).is_err());
        assert!(s.execute("ink.options", &json!({"ink": "Pantone B", "alias": "Pantone A"})).is_err(), "an alias loop is refused");
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
        let r = s.execute("file.exportPdf", &json!({"tagged": true, "compressImages": false})).unwrap();
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
mod delete_tests {
    use designcraft_doc::{CellStyle, ObjectStyle, StoryId, Toc, TocEntry};
    use serde_json::json;

    use crate::Session;

    /// Is `name` still named anywhere in the document?
    fn named(s: &Session, name: &str) -> bool {
        serde_json::to_string(&*s.doc().unwrap().doc).unwrap().contains(&format!("\"{name}\""))
    }

    fn char_doc() -> (Session, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Base", "chars": {"fontStyle": "Bold"}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Emphasis", "basedOn": "Base", "chars": {"tracking": 20}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Child", "basedOn": "Emphasis"})).unwrap();
        s.execute("style.character.create", &json!({"name": "Strong", "chars": {"size": 14}})).unwrap();
        let refs = json!({
            "nestedStyles": [{"style": "Emphasis", "through": true, "count": 1, "until": {"kind": "words"}}],
            "grepStyles": [{"style": "Emphasis", "pattern": "\\d+"}],
            "nestedLineStyles": [{"style": "Emphasis", "lines": 1}],
            "dropCapLines": 2, "dropCapChars": 1, "dropCapStyle": "Emphasis"});
        s.execute("style.paragraph.create", &json!({"name": "Lead", "para": refs})).unwrap();
        s.execute("footnote.options", &json!({"refCharStyle": "Emphasis"})).unwrap();
        let sid = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hi there"})).unwrap()["story"]
            .as_u64()
            .unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 2})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Emphasis"})).unwrap();
        // Local paragraph overrides name it too.
        s.execute("type.para", &json!({"attrs": refs})).unwrap();
        s.edit(|d, _| {
            d.styles_mut().default_character = "Emphasis".into();
            Ok(())
        })
        .unwrap();
        assert!(named(&s, "Emphasis"));
        (s, sid)
    }

    #[test]
    fn deleting_a_character_style_points_every_use_at_the_replacement() {
        let (mut s, sid) = char_doc();
        let undo = s.doc().unwrap().history.undo.len();
        s.execute("style.character.delete", &json!({"name": "Emphasis", "replaceWith": "Strong"})).unwrap();
        assert!(!named(&s, "Emphasis"), "no dangling reference");
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.styles.char_style("Child").unwrap().based_on.as_deref(), Some("Strong"));
        assert_eq!(d.story(StoryId(sid)).unwrap().chars[0].format.style, "Strong");
        let lead = &d.styles.para("Lead").unwrap().para;
        assert_eq!(lead.nested_styles.as_ref().unwrap()[0].style, "Strong");
        assert_eq!(lead.grep_styles.as_ref().unwrap()[0].style, "Strong");
        assert_eq!(lead.nested_line_styles.as_ref().unwrap()[0].style, "Strong");
        assert_eq!(lead.drop_cap_style.as_deref(), Some("Strong"));
        assert_eq!(d.footnote_options.ref_char_style, "Strong");
        assert_eq!(d.styles.default_character, "Strong");
        // One undo step brings the style and every use back.
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.styles.char_style("Emphasis").is_some());
        assert_eq!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().chars[0].format.style, "Emphasis");
    }

    #[test]
    fn deleting_a_character_style_for_none_unsets_based_on() {
        let (mut s, _) = char_doc();
        s.execute("style.character.delete", &json!({"name": "Emphasis"})).unwrap();
        assert!(!named(&s, "Emphasis"), "no dangling reference");
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.styles.char_style("Child").unwrap().based_on, None);
        assert_eq!(d.footnote_options.ref_char_style, designcraft_doc::NO_CHAR_STYLE);
    }

    /// Replacing a style with its own descendant rebases the deleted style's children on its
    /// parent, so no based-on chain becomes a cycle.
    #[test]
    fn replacing_a_style_with_its_child_makes_no_cycle() {
        let (mut s, _) = char_doc();
        s.execute("style.character.create", &json!({"name": "Grandchild", "basedOn": "Child"})).unwrap();
        s.execute("style.character.delete", &json!({"name": "Emphasis", "replaceWith": "Grandchild"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.styles.char_style("Child").unwrap().based_on.as_deref(), Some("Base"));
        assert_eq!(d.styles.char_style("Grandchild").unwrap().based_on.as_deref(), Some("Child"));
        assert!(!named(&s, "Emphasis"));
    }

    #[test]
    fn bad_style_deletes_are_errors_and_change_nothing() {
        let (mut s, _) = char_doc();
        s.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        for (id, p) in [
            ("style.character.delete", json!({})),
            ("style.character.delete", json!({"name": ""})),
            ("style.character.delete", json!({"name": "Nope"})),
            ("style.character.delete", json!({"name": "[None]"})),
            ("style.character.delete", json!({"name": "Emphasis", "replaceWith": "Nope"})),
            ("style.character.delete", json!({"name": "Emphasis", "replaceWith": "Emphasis"})),
            ("style.paragraph.delete", json!({})),
            ("style.paragraph.delete", json!({"name": ""})),
            ("style.paragraph.delete", json!({"name": "Nope"})),
            ("style.paragraph.delete", json!({"name": "[Basic Paragraph]"})),
            ("style.paragraph.delete", json!({"name": "Head", "replaceWith": "Nope"})),
            ("style.paragraph.delete", json!({"name": "Head", "replaceWith": "Head"})),
        ] {
            assert!(s.execute(id, &p).is_err(), "{id} {p}");
        }
        assert_eq!(*s.doc().unwrap().doc, *before);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }

    #[test]
    fn deleting_a_paragraph_style_points_every_use_at_the_replacement() {
        let mut s = Session::new();
        s.execute("file.newSample", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Sub", "basedOn": "Table Body", "nextStyle": "Table Body"})).unwrap();
        s.edit(|d, _| {
            let story = *d.stories.keys().next().ok_or(crate::EngineError::NoDocument)?;
            let st = d.styles_mut();
            st.default_paragraph = "Table Body".into();
            st.object.push(ObjectStyle { name: "Boxed".into(), paragraph_style: Some("Table Body".into()), ..Default::default() });
            st.cell.push(CellStyle { name: "Body Cell".into(), paragraph_style: Some("Table Body".into()), ..Default::default() });
            d.footnote_options.para_style = "Table Body".into();
            d.endnote_options.para_style = "Table Body".into();
            d.endnote_options.heading_style = "Table Body".into();
            d.toc = Some(Toc {
                story,
                title: "Contents".into(),
                entries: vec![TocEntry { entry_style: Some("Table Body".into()), ..TocEntry::new("Table Body", 1) }],
                page_numbers: false,
                title_style: Some("Table Body".into()),
            });
            Ok(())
        })
        .unwrap();
        assert!(named(&s, "Table Body"));
        s.execute("style.paragraph.delete", &json!({"name": "Table Body", "replaceWith": "Body"})).unwrap();
        assert!(!named(&s, "Table Body"), "no dangling reference");
        let d = s.doc().unwrap().doc.clone();
        let sub = d.styles.para("Sub").unwrap();
        assert_eq!((sub.based_on.as_deref(), sub.next_style.as_deref()), (Some("Body"), Some("Body")));
        assert_eq!(d.styles.default_paragraph, "Body");
        assert_eq!(d.endnote_options.heading_style, "Body");
        let after = s.execute("preflight.run", &json!({})).unwrap();
        assert!(!after.to_string().contains("Table Body"), "{after}");
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
mod grep_style_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn grep_style_pattern_that_cannot_compile_is_rejected() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Keep"})).unwrap();
        let bad = json!([{"style": "Keep", "pattern": "(\\d+"}]);
        let e = s.execute("style.paragraph.create", &json!({"name": "Broken", "para": {"grepStyles": bad}})).unwrap_err().to_string();
        assert!(e.contains("bad GREP") && e.contains("(\\\\d+"), "{e}");
        assert!(s.doc().unwrap().doc.styles.para("Broken").is_none());

        // The lookbehind dialect compiles.
        let good = json!([{"style": "Keep", "pattern": "(?<=\\s)[aiouwzAIOUWZ]\\s"}]);
        s.execute("style.paragraph.create", &json!({"name": "Polish", "para": {"grepStyles": good}})).unwrap();
        let e = s.execute("style.paragraph.edit", &json!({"name": "Polish", "para": {"grepStyles": bad, "spaceAfter": 6}})).unwrap_err().to_string();
        assert!(e.contains("bad GREP"), "{e}");
        let st = s.doc().unwrap().doc.styles.para("Polish").unwrap().clone();
        assert_eq!(st.para.grep_styles.unwrap()[0].pattern, "(?<=\\s)[aiouwzAIOUWZ]\\s");
        assert_eq!(st.para.space_after, None, "nothing changes");

        // A paragraph override too.
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Ala i kot"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        let e = s.execute("type.para", &json!({"grepStyles": bad})).unwrap_err().to_string();
        assert!(e.contains("bad GREP"), "{e}");
    }

    #[test]
    fn idml_import_keeps_a_broken_grep_style_and_warns() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Keep"})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Broken"})).unwrap();
        // Written past the command, as another application could.
        s.edit(|d, _| {
            if let Some(st) = d.styles_mut().para_mut("Broken") {
                st.para.grep_styles = Some(vec![designcraft_doc::GrepStyle { style: "Keep".into(), pattern: "[a-".into() }]);
            }
            Ok(())
        })
        .unwrap();
        let b64 = s.execute("file.exportIdml", &json!({})).unwrap()["base64"].as_str().unwrap().to_string();
        let r = s.execute("file.openIdml", &json!({"base64": b64})).unwrap();
        let warnings = r["warnings"].to_string();
        assert!(warnings.contains("Broken") && warnings.contains("[a-") && warnings.contains("bad GREP"), "{warnings}");
        let st = s.doc().unwrap().doc.styles.para("Broken").unwrap().clone();
        assert_eq!(st.para.grep_styles.unwrap()[0].pattern, "[a-", "the style opens with its pattern");
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

#[cfg(test)]
#[path = "text_color_tests.rs"]
mod text_color_tests;
