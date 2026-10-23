//! Object creation and the Object menu: frames, lines, transforms, arrange, group, lock/hide,
//! fill/stroke, content type, Text Frame Options, corners, fitting, text wrap.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use designcraft_color::BlendMode;
use designcraft_doc::{
    AssetId, Content, Document, Effects, Fill, Item, ItemId, ItemLoc, ParaFormat, Selection, Shape, SpreadRef, Story, StoryId, Stroke, TextFrame,
    TextFrameOptions, TextSel,
};
use designcraft_geom::{Affine, Point, Rect, Vec2, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, f64_or, has_doc, has_selection, ok, point_param, rect_param, spread_param, str_param, targets};
use crate::{Result, Session};

/// `name` must be a swatch of the document: an unknown name would paint nothing. The error names
/// the swatch it was probably meant to be (`Paper` → `[Paper]`, a different case).
pub(crate) fn known_swatch(doc: &designcraft_doc::Document, name: &str, cmd: &str, key: &str) -> Result<()> {
    if doc.swatch(name).is_some() {
        return Ok(());
    }
    let bare = |n: &str| n.trim().trim_start_matches('[').trim_end_matches(']').to_lowercase();
    let hint = doc.swatches.iter().find(|s| bare(&s.name) == bare(name)).map(|s| format!(" (did you mean `{}`?)", s.name)).unwrap_or_default();
    Err(bad(cmd, format!("{key}: no swatch named `{name}`{hint}; create one with `swatch.create` and use its returned name")))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "frame.create",
            "Create Frame",
            [],
            None,
            "{spread?: index | {kind: doc|parent, index} (a parent spread: index into `parents` of document.inspect), rect: [x0,y0,x1,y1] (spread coords), shape?: rectangle|ellipse|polygon, content?: graphic|text|unassigned, sides?: 6, starInset?: 0–100 (%), text?: string, caret?: bool, vertical?: bool (text: a new vertical story)}",
            has_doc,
            frame_create
        ),
        cmd!("line.create", "Create Line", [], None, "{spread?, a: [x,y], b: [x,y]}", has_doc, line_create),
        cmd!(
            "transform.move",
            "Move",
            ["Object", "Transform"],
            None,
            "{dx, dy, copy?: bool, ids?, toSpread?, content?: bool (move the placed graphic or pasted-in items inside each frame; the frame stays. Default: whether the selection is content, when neither ids nor copy is given)} → {moved, ids: the objects moved (with copy, the copies)}",
            has_selection,
            transform_move
        ),
        cmd!(
            "transform.resize",
            "Resize",
            [],
            None,
            "{from: rect, to: rect, content?: bool (scale content), distribute?: bool (Live Distribute: objects keep their size, their centres spread with the bounds), ids?}",
            has_selection,
            transform_resize
        ),
        cmd!(
            "transform.rotate",
            "Rotate",
            ["Object", "Transform"],
            None,
            "{angle (degrees, CCW), ref?: 0..8 (reference point of the selection bounds, default 4: centre), ids?}",
            has_selection,
            transform_rotate
        ),
        cmd!("transform.scale", "Scale", ["Object", "Transform"], None, "{sx, sy, ids?} (about the selection centre)", has_selection, |s, p| {
            let sx = f64_or(p, "sx", 1.0);
            let sy = f64_or(p, "sy", sx);
            if sx.abs() < 1e-4 || sy.abs() < 1e-4 {
                return Err(bad("transform.scale", "scale too small"));
            }
            let strokes = s.prefs.scale_strokes;
            let ids = targets(s, p)?;
            s.edit(|d, _| {
                let c = union_bounds(d, &ids).center();
                let m = Affine::translate(c.to_vec2()) * Affine::scale_non_uniform(sx, sy) * Affine::translate(-c.to_vec2());
                for id in &ids {
                    scale_object(d, *id, m, (sx * sy).abs().sqrt(), strokes);
                }
                ok()
            })
        }),
        cmd!(
            "transform.again",
            "Transform Again",
            ["Object", "Transform Again"],
            Some("Cmd+Alt+3"),
            "{individually?: bool, sequence?: bool, ids?} — repeat the last transform (or the whole sequence applied to this selection), on the selection as a whole or on each object about its own centre",
            has_selection,
            transform_again
        ),
        cmd!(
            "transform.clear",
            "Clear Transformations",
            ["Object", "Transform"],
            None,
            "{ids?} — remove rotation, shear and scaling (the object keeps its centre)",
            has_selection,
            clear_transformations
        ),
        cmd!(
            "transform.shear",
            "Shear",
            ["Object", "Transform"],
            None,
            "{angle (degrees), ref?: 0..8 (reference point of the selection bounds, default 4: centre), ids?}",
            has_selection,
            |s, p| {
                let a = f64_or(p, "angle", 0.0).clamp(-85.0, 85.0).to_radians().tan();
                apply_about_ref(s, p, Affine::new([1.0, 0.0, a, 1.0, 0.0, 0.0]))
            }
        ),
        cmd!(
            "object.matchAttributes",
            "Apply Attributes (Eyedropper)",
            [],
            None,
            "{from: id, ids?} — copy fill, stroke, corners, opacity, effects, wrap",
            has_selection,
            match_attributes
        ),
        cmd!(
            "transform.flip",
            "Flip",
            ["Object", "Transform"],
            None,
            "{axis: horizontal|vertical, ref?: 0..8 (reference point of the selection bounds, default 4: centre), ids?}",
            has_selection,
            |s, p| {
                let h = str_param(p, "axis") != Some("vertical");
                apply_about_ref(s, p, if h { Affine::scale_non_uniform(-1.0, 1.0) } else { Affine::scale_non_uniform(1.0, -1.0) })
            }
        ),
        cmd!(
            "transform.set",
            "Transform Panel",
            [],
            None,
            "{x?, y? (measured from the rulers' zero point, view.zeroPoint), width?, height?, scaleX? (%), scaleY? (%), rotation? (°), shear? (°), ref?: 0..8, ids?, content?: bool (the placed graphic inside each frame; the frame stays. Default: whether the selection is content, when no ids are given), bounds?: [x0, y0, x1, y1] (content: the graphic's new bounds on the spread, as its handles drag it)} — reference-point based geometry; rotation and shear are absolute (Transformations are Totals decides whether nested objects measure them on the pasteboard)",
            has_selection,
            transform_set
        ),
        cmd!(query "transform.info", "Transform Values", [], None, "{ids?} → {scaleX, scaleY (%), rotation, shear (°), content?: the same for a placed graphic} of the first target", has_selection, transform_info),
        cmd!("object.arrange", "Arrange", ["Object", "Arrange"], None, "{to: front|forward|backward|back, ids?}", has_selection, arrange),
        cmd!("object.bringToFront", "Bring to Front", ["Object", "Arrange"], Some("Cmd+Shift+]"), "{}", has_selection, |s, _| s
            .execute("object.arrange", &json!({"to": "front"}))),
        cmd!("object.bringForward", "Bring Forward", ["Object", "Arrange"], Some("Cmd+]"), "{}", has_selection, |s, _| s
            .execute("object.arrange", &json!({"to": "forward"}))),
        cmd!("object.sendBackward", "Send Backward", ["Object", "Arrange"], Some("Cmd+["), "{}", has_selection, |s, _| s
            .execute("object.arrange", &json!({"to": "backward"}))),
        cmd!("object.sendToBack", "Send to Back", ["Object", "Arrange"], Some("Cmd+Shift+["), "{}", has_selection, |s, _| s
            .execute("object.arrange", &json!({"to": "back"}))),
        cmd!("object.group", "Group", ["Object"], Some("Cmd+G"), "{ids?}", can_group, group),
        cmd!("object.ungroup", "Ungroup", ["Object"], Some("Cmd+Shift+G"), "{ids?}", has_selection, ungroup),
        cmd!("object.lock", "Lock", ["Object"], Some("Cmd+L"), "{ids?}", has_selection, |s, p| set_flag(s, p, |i| i.locked = true, true)),
        cmd!("object.unlockAll", "Unlock All on Spread", ["Object"], Some("Cmd+Alt+L"), "{}", has_doc, |s, _| all_flag(s, |i| i.locked = false)),
        cmd!("object.hide", "Hide", ["Object"], Some("Cmd+3"), "{ids?}", has_selection, |s, p| set_flag(s, p, |i| i.hidden = true, true)),
        cmd!("object.showAll", "Show All on Spread", ["Object"], Some("Cmd+Alt+3"), "{}", has_doc, |s, _| all_flag(s, |i| i.hidden = false)),
        cmd!(
            "object.setFlags",
            "Set Visibility / Lock",
            [],
            None,
            "{ids: [id], hidden?: bool, locked?: bool} — Layers panel eye/lock per object",
            has_doc,
            |s, p| {
                let hidden = p.get("hidden").and_then(Value::as_bool);
                let locked = p.get("locked").and_then(Value::as_bool);
                set_flag(
                    s,
                    p,
                    move |i| {
                        if let Some(h) = hidden {
                            i.hidden = h;
                        }
                        if let Some(l) = locked {
                            i.locked = l;
                        }
                    },
                    hidden == Some(true) || locked == Some(true),
                )
            }
        ),
        cmd!(
            "object.fill",
            "Fill",
            [],
            None,
            "{swatch? (a swatch's name, e.g. [Paper]), tint?: 0..1, ids?} — at least one; a tint alone keeps each object's own swatch (the Swatches panel's Tint field). With nothing selected: the fill of new shapes",
            has_doc,
            |s, p| {
                let sw = str_param(p, "swatch").map(str::to_string);
                let tint = match p.get("tint") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(v.as_f64().filter(|t| (0.0..=1.0).contains(t)).ok_or_else(|| bad("object.fill", "`tint` is 0..1"))? as f32),
                };
                match &sw {
                    Some(sw) => known_swatch(&s.doc()?.doc, sw, "object.fill", "swatch")?,
                    None if tint.is_none() => return Err(bad("object.fill", "missing swatch")),
                    None => {}
                }
                if nothing_targeted(s, p)? {
                    return set_new_object(s, |c| {
                        if let Some(sw) = &sw {
                            c.fill = sw.clone();
                            c.fill_tint = 1.0;
                        }
                        c.fill_tint = tint.unwrap_or(c.fill_tint);
                    });
                }
                set_flag(
                    s,
                    p,
                    move |i| match &sw {
                        Some(sw) => i.fill = Fill { swatch: sw.clone(), tint: tint.unwrap_or(1.0), ..i.fill.clone() },
                        None => i.fill.tint = tint.unwrap_or(i.fill.tint),
                    },
                    false,
                )
            }
        ),
        cmd!(
            "object.swapFillStroke",
            "Swap Fill and Stroke",
            [],
            None,
            "{ids?} — each object's fill and stroke colours (and tints) trade places; locked objects stay as they are. With nothing selected: those of new shapes",
            has_doc,
            |s, p| {
                if nothing_targeted(s, p)? {
                    return set_new_object(s, |c| {
                        std::mem::swap(&mut c.fill, &mut c.stroke);
                        std::mem::swap(&mut c.fill_tint, &mut c.stroke_tint);
                    });
                }
                has_editable_selection(s).map_err(|e| bad("object.swapFillStroke", e))?;
                edit_unlocked(s, p, swap_fill_stroke)
            }
        ),
        cmd!(
            "object.defaultFillStroke",
            "Default Fill and Stroke",
            [],
            None,
            "{ids?} — no fill and a black stroke; locked objects stay as they are. With nothing selected: for new shapes",
            has_doc,
            |s, p| {
                if nothing_targeted(s, p)? {
                    return set_new_object(s, |c| *c = designcraft_doc::NewObjectColors::default());
                }
                has_editable_selection(s).map_err(|e| bad("object.defaultFillStroke", e))?;
                edit_unlocked(s, p, default_fill_stroke)
            }
        ),
        cmd!(
            "object.stroke",
            "Stroke",
            [],
            None,
            "{swatch?, tint?, weight?, align?: center|inside|outside, type?: {kind: solid|dashed|dotted|thickThin|thinThick|thinThin|thickThick|thinThickThin|thickThinThick|wavy|hashed|stripes|style, pattern?: [dash, gap, …] (dashed: required, points), bands?: [[start, width], …] (stripes: required, fractions of stroke weight), name?: string (style: required, document stroke style name)} (other kinds take no extra fields; unknown type fields are errors), cap?: butt|round|projecting, join?: miter|round|bevel, miterLimit?, start?, end?: none|simple|simpleWide|triangle|triangleWide|barbed|curved|circle|circleSolid|square|squareSolid|bar, gapSwatch?, gapTint?, gapOverprint?: bool, ids?}",
            has_doc,
            |s, p| {
                let kind: Option<designcraft_doc::StrokeType> = p
                    .get("type")
                    .map(|v| {
                        if let Some(fields) = v.as_object() {
                            let field = match v.get("kind").and_then(Value::as_str) {
                                Some("dashed") => Some("pattern"),
                                Some("stripes") => Some("bands"),
                                Some("style") => Some("name"),
                                _ => None,
                            };
                            for key in fields.keys() {
                                if key != "kind" && Some(key.as_str()) != field {
                                    return Err(bad("object.stroke", format!("type: unknown field `{key}`")));
                                }
                            }
                        }
                        serde_json::from_value(v.clone()).map_err(|e| bad("object.stroke", format!("type: {e}")))
                    })
                    .transpose()?;
                for key in ["swatch", "gapSwatch"] {
                    if let Some(name) = p.get(key).and_then(Value::as_str) {
                        known_swatch(&s.doc()?.doc, name, "object.stroke", key)?;
                    }
                }
                // With nothing selected, the swatch and tint are those of new shapes.
                if nothing_targeted(s, p)? {
                    let sw = str_param(p, "swatch").map(str::to_string);
                    let tint = p.get("tint").and_then(Value::as_f64).map(|t| t.clamp(0.0, 1.0) as f32);
                    if sw.is_none() && tint.is_none() {
                        return Err(bad("object.stroke", super::NOTHING_SELECTED));
                    }
                    return set_new_object(s, |c| {
                        if let Some(sw) = &sw {
                            c.stroke = sw.clone();
                            c.stroke_tint = 1.0;
                        }
                        c.stroke_tint = tint.unwrap_or(c.stroke_tint);
                    });
                }
                let p2 = p.clone();
                // A tint alone (the Swatches panel's Tint field) recolours strokes, it doesn't add one.
                let tint_only = p.as_object().is_some_and(|o| o.contains_key("tint") && o.keys().all(|k| k == "tint" || k == "ids"));
                set_flag(
                    s,
                    p,
                    move |i| {
                        if tint_only && i.stroke.is_none() {
                            return;
                        }
                        let mut st = if i.stroke.is_none() && p2.get("swatch").is_none() { Stroke::default() } else { i.stroke.clone() };
                        if let Some(v) = p2.get("swatch").and_then(Value::as_str) {
                            st.swatch = v.into();
                        }
                        if let Some(v) = p2.get("tint").and_then(Value::as_f64) {
                            st.tint = v.clamp(0.0, 1.0) as f32;
                        }
                        if let Some(v) = p2.get("weight").and_then(Value::as_f64) {
                            st.weight = v.max(0.0);
                        }
                        if let Some(v) = p2.get("miterLimit").and_then(Value::as_f64) {
                            st.miter_limit = v.clamp(1.0, 500.0);
                        }
                        if let Some(v) = p2.get("gapSwatch").and_then(Value::as_str) {
                            st.gap_swatch = v.into();
                        }
                        if let Some(v) = p2.get("gapTint").and_then(Value::as_f64) {
                            st.gap_tint = v.clamp(0.0, 1.0) as f32;
                        }
                        if let Some(v) = p2.get("gapOverprint").and_then(Value::as_bool) {
                            st.gap_overprint = v;
                        }
                        if let Some(kind) = &kind {
                            st.kind = kind.clone();
                        }
                        for (k, apply) in [("align", 0), ("cap", 2), ("join", 3), ("start", 4), ("end", 5)] {
                            if let Some(v) = p2.get(k).cloned() {
                                let _ = match apply {
                                    0 => serde_json::from_value(v).map(|x| st.align = x),
                                    2 => serde_json::from_value(v).map(|x| st.cap = x),
                                    3 => serde_json::from_value(v).map(|x| st.join = x),
                                    4 => serde_json::from_value(v).map(|x| st.start = x),
                                    _ => serde_json::from_value(v).map(|x| st.end = x),
                                };
                            }
                        }
                        i.stroke = st;
                    },
                    false,
                )
            }
        ),
        cmd!("object.opacity", "Opacity", [], None, "{opacity: 0..1, blend?: normal|multiply|…, ids?}", has_selection, |s, p| {
            let o = f64_or(p, "opacity", 1.0).clamp(0.0, 1.0) as f32;
            // `BlendMode::parse` takes the documented lowercase forms (`multiply`), the panel
            // labels (`Color Burn`) and the serialized names (`Multiply`). An unknown value is
            // an error rather than a silent no-op that still reports `changed` (#260).
            let blend = match p.get("blend") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    let raw = v.as_str().ok_or_else(|| bad("object.opacity", "blend must be a string"))?;
                    Some(BlendMode::parse(raw).ok_or_else(|| bad("object.opacity", format!("unknown blend mode `{raw}`")))?)
                }
            };
            set_flag(
                s,
                p,
                move |i| {
                    i.opacity = o;
                    if let Some(b) = blend {
                        i.blend = b;
                    }
                },
                false,
            )
        }),
        cmd!(
            "object.dropShadow",
            "Drop Shadow",
            ["Object", "Effects"],
            Some("Cmd+Alt+M"),
            "{on?: bool, distance?, angle?, globalLight?: bool, opacity?, size?, spread? (%), color? (swatch), ids?}",
            has_selection,
            |s, p| {
                let p2 = p.clone();
                set_effects(s, p, "object.dropShadow", move |fx| {
                    let ds = &mut fx.drop_shadow;
                    ds.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!ds.on);
                    ds.distance = f64_or(&p2, "distance", ds.distance);
                    ds.angle = f64_or(&p2, "angle", ds.angle);
                    ds.global_light = p2.get("globalLight").and_then(Value::as_bool).unwrap_or(ds.global_light);
                    ds.opacity = f64_or(&p2, "opacity", ds.opacity as f64) as f32;
                    ds.size = f64_or(&p2, "size", ds.size);
                    ds.spread = f64_or(&p2, "spread", ds.spread);
                    if let Some(c) = str_param(&p2, "color") {
                        ds.color = c.into();
                    }
                })
            }
        ),
        cmd!(
            "object.innerShadow",
            "Inner Shadow",
            ["Object", "Effects"],
            None,
            "{on?: bool, distance?, angle?, globalLight?: bool, opacity?, size?, choke? (%), color?, ids?}",
            has_selection,
            |s, p| {
                let p2 = p.clone();
                set_effects(s, p, "object.innerShadow", move |fx| {
                    let e = &mut fx.inner_shadow;
                    e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
                    e.distance = f64_or(&p2, "distance", e.distance);
                    e.angle = f64_or(&p2, "angle", e.angle);
                    e.global_light = p2.get("globalLight").and_then(Value::as_bool).unwrap_or(e.global_light);
                    e.opacity = f64_or(&p2, "opacity", e.opacity as f64) as f32;
                    e.size = f64_or(&p2, "size", e.size);
                    e.choke = f64_or(&p2, "choke", e.choke);
                    if let Some(c) = str_param(&p2, "color") {
                        e.color = c.into();
                    }
                })
            }
        ),
        cmd!(
            "object.outerGlow",
            "Outer Glow",
            ["Object", "Effects"],
            None,
            "{on?: bool, opacity?, size?, spread? (%), color?, ids?}",
            has_selection,
            |s, p| {
                let p2 = p.clone();
                set_effects(s, p, "object.outerGlow", move |fx| {
                    let e = &mut fx.outer_glow;
                    e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
                    e.opacity = f64_or(&p2, "opacity", e.opacity as f64) as f32;
                    e.size = f64_or(&p2, "size", e.size);
                    e.spread = f64_or(&p2, "spread", e.spread);
                    if let Some(c) = str_param(&p2, "color") {
                        e.color = c.into();
                    }
                })
            }
        ),
        cmd!(
            "object.exportOptions",
            "Object Export Options…",
            ["Object"],
            None,
            "{ids?, artifact?: bool (tagged PDF decoration), rasterize?: bool (EPUB/HTML image), align?: left|center|right|\"\", pageBreakBefore?: bool, altText?}",
            has_selection,
            |s, p| {
                let ids = targets(s, p)?;
                let p = p.clone();
                s.edit(|d, _| {
                    for id in &ids {
                        let Some(it) = d.item_mut(*id) else { continue };
                        let e = &mut it.export_options;
                        if let Some(v) = p.get("artifact").and_then(Value::as_bool) {
                            e.artifact = v;
                        }
                        if let Some(v) = p.get("rasterize").and_then(Value::as_bool) {
                            e.rasterize = v;
                        }
                        if let Some(v) = p.get("align").and_then(Value::as_str) {
                            e.align = v.to_string();
                        }
                        if let Some(v) = p.get("pageBreakBefore").and_then(Value::as_bool) {
                            e.page_break_before = v;
                        }
                        if let Some(v) = p.get("altText").and_then(Value::as_str) {
                            it.alt_text = v.to_string();
                        }
                    }
                    Ok(json!({"objects": ids.len()}))
                })
            }
        ),
        cmd!(
            "object.innerGlow",
            "Inner Glow",
            ["Object", "Effects"],
            None,
            "{on?: bool, opacity?, size?, choke? (%), center?: bool (glow from the centre), color?, ids?}",
            has_selection,
            |s, p| {
                let p2 = p.clone();
                set_effects(s, p, "object.innerGlow", move |fx| {
                    let e = &mut fx.inner_glow;
                    e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
                    e.opacity = f64_or(&p2, "opacity", e.opacity as f64) as f32;
                    e.size = f64_or(&p2, "size", e.size);
                    e.choke = f64_or(&p2, "choke", e.choke);
                    e.center = p2.get("center").and_then(Value::as_bool).unwrap_or(e.center);
                    if let Some(c) = str_param(&p2, "color") {
                        e.color = c.into();
                    }
                })
            }
        ),
        cmd!(
            "object.bevel",
            "Bevel and Emboss",
            ["Object", "Effects"],
            None,
            "{on?: bool, size?, depth? (%), angle?, globalLight?: bool, highlight?, highlightOpacity?, shadow?, shadowOpacity?, ids?}",
            has_selection,
            |s, p| {
                let p2 = p.clone();
                set_effects(s, p, "object.bevel", move |fx| {
                    let e = &mut fx.bevel;
                    e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
                    e.size = f64_or(&p2, "size", e.size);
                    e.depth = f64_or(&p2, "depth", e.depth);
                    e.angle = f64_or(&p2, "angle", e.angle);
                    e.global_light = p2.get("globalLight").and_then(Value::as_bool).unwrap_or(e.global_light);
                    e.highlight_opacity = f64_or(&p2, "highlightOpacity", e.highlight_opacity as f64) as f32;
                    e.shadow_opacity = f64_or(&p2, "shadowOpacity", e.shadow_opacity as f64) as f32;
                    if let Some(c) = str_param(&p2, "highlight") {
                        e.highlight = c.into();
                    }
                    if let Some(c) = str_param(&p2, "shadow") {
                        e.shadow = c.into();
                    }
                })
            }
        ),
        cmd!(
            "object.satin",
            "Satin",
            ["Object", "Effects"],
            None,
            "{on?: bool, opacity?, angle?, distance?, size?, invert?: bool, color?, ids?}",
            has_selection,
            |s, p| {
                let p2 = p.clone();
                set_effects(s, p, "object.satin", move |fx| {
                    let e = &mut fx.satin;
                    e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
                    e.opacity = f64_or(&p2, "opacity", e.opacity as f64) as f32;
                    e.angle = f64_or(&p2, "angle", e.angle);
                    e.distance = f64_or(&p2, "distance", e.distance);
                    e.size = f64_or(&p2, "size", e.size);
                    e.invert = p2.get("invert").and_then(Value::as_bool).unwrap_or(e.invert);
                    if let Some(c) = str_param(&p2, "color") {
                        e.color = c.into();
                    }
                })
            }
        ),
        cmd!(
            "object.directionalFeather",
            "Directional Feather",
            ["Object", "Effects"],
            None,
            "{on?: bool, widths?: [top, left, bottom, right] | number, ids?}",
            has_selection,
            |s, p| {
                let p2 = p.clone();
                set_effects(s, p, "object.directionalFeather", move |fx| {
                    let e = &mut fx.directional_feather;
                    e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
                    match p2.get("widths") {
                        Some(Value::Number(n)) => e.widths = [n.as_f64().unwrap_or(0.0).max(0.0); 4],
                        Some(v @ Value::Array(_)) => {
                            if let Ok(w) = serde_json::from_value::<[f64; 4]>(v.clone()) {
                                e.widths = w.map(|x| x.max(0.0));
                            }
                        }
                        _ => {}
                    }
                    if e.on && e.widths.iter().all(|w| *w <= 0.0) {
                        e.widths = [9.0; 4];
                    }
                })
            }
        ),
        cmd!(
            "object.effects",
            "Effects",
            [],
            None,
            "{effects?: {dropShadow?: {on?, color?, opacity?, angle?, globalLight?, distance?, size?, spread?}, innerShadow?, outerGlow?, innerGlow? {on?, color?, opacity?, size?, choke?, center?, mode? (blend mode: Screen, Multiply, …; Screen by default), technique?: softer|precise}, bevel?, satin?, feather? (width), featherOptions? {choke? (%), corners?: sharp|rounded|diffused}, directionalFeather?, gradientFeather?} (the fields given replace the object's, as the Effects dialog sets them), opacity?: 0..1, blend?, ids?} — every effect at once, one undo step",
            has_selection,
            effects_all
        ),
        cmd!("object.feather", "Basic Feather", ["Object", "Effects"], None, "{width (0 = off), ids?}", has_selection, |s, p| {
            let w = f64_or(p, "width", 9.0).max(0.0);
            set_effects(s, p, "object.feather", move |fx| fx.feather = w)
        }),
        cmd!("object.content", "Content", ["Object", "Content"], None, "{type: graphic|text|unassigned, ids?}", has_selection, content_type),
        cmd!(
            "object.textFrameOptions",
            "Text Frame Options…",
            ["Object"],
            Some("Cmd+B"),
            "{columns?, gutter?, inset?: number|[t,l,b,r] (null keeps that side), verticalJustification?: top|center|bottom|justify, columnsKind?: fixedNumber|fixedWidth, columnWidth? (pt, Fixed Width), firstBaseline?: ascent|capHeight|leading|xHeight|emboxHeight|fixed, firstBaselineMin? (pt), autoSize?: off|heightOnly|widthOnly|heightAndWidth|heightAndWidthProportionally, autoSizeReference?: topLeft|topCenter|topRight|leftCenter|center|rightCenter|bottomLeft|bottomCenter|bottomRight (the point that stays put), useMinimumHeight?: bool, minimumHeight? (pt), useMinimumWidth?: bool, minimumWidth? (pt), noLineBreaks?: bool, ignoreWrap?, balanceColumns?, columnRule?: bool, columnRuleWeight? (pt), columnRuleColor? (swatch name), columnRuleTint? (0..1), columnRuleOffset? (pt, horizontal), columnRuleTopInset? (pt), columnRuleBottomInset? (pt), vertical?: bool (sets the story direction of the frames' stories, as Type ▸ Story Direction), ids?}",
            has_selection,
            text_frame_options
        ),
        cmd!(
            "object.cornerOptions",
            "Corner Options…",
            ["Object"],
            None,
            "{shape: none|rounded|inverseRounded|inset|bevel|fancy, size, corners?: [{shape, size}] (4 entries in path order: top-left, top-right, bottom-right, bottom-left; replaces shape and size), ids?}",
            has_selection,
            corner_options
        ),
        cmd!(
            "object.textWrap",
            "Text Wrap",
            ["Window"],
            Some("Cmd+Alt+W"),
            "{mode?: none|boundingBox|contour|jumpObject|jumpToNextColumn (boundingBox when absent, unless side or contour is given: then unchanged), offset?: number|[t,l,b,r], invert?: bool, side?: bothSides|leftSide|rightSide|towardsSpine|awayFromSpine|largestArea, contour?: boundingBox|graphicFrame|sameAsClipping, content?: bool (the placed graphic instead of its frame; default: whether the selection is the frame's content), ids?}; offset or contour drops an imported custom wrap boundary",
            has_selection,
            text_wrap
        ),
        cmd!(
            "object.fit",
            "Fitting",
            ["Object", "Fitting"],
            None,
            "{mode: fillProportionally (preserves aspect ratio, covers frame, clips overflow)|fitProportionally (preserves aspect ratio, shows whole image, may leave empty space)|fitContentToFrame (stretches content to frame)|centerContent (keeps current size, positions at fitting alignment)|fitFrameToContent (resizes frame to content), axis?: both|height|width (frame-local sides Fit Frame to Content moves; default both), ids?} — locked objects and objects on locked layers are left as they are",
            has_selection,
            fit
        ),
        cmd!(
            "object.fittingOptions",
            "Frame Fitting Options…",
            ["Object", "Fitting"],
            None,
            "{autoFit?: bool, fitting?: none|fillProportionally|fitProportionally|fitContentToFrame|centerContent, align?: 0..8 (reference point, 4 = centre), crop?: n | [t, l, b, r] (negative adds space), ids?}",
            has_selection,
            fitting_options
        ),
        cmd!("object.rename", "Rename", [], None, "{id | ids?, name}", has_doc, |s, p| {
            let name = str_param(p, "name").unwrap_or("").to_string();
            set_flag(s, p, move |i| i.name = name.clone(), false)
        }),
        cmd!(
            "edit.stepAndRepeat",
            "Step and Repeat…",
            ["Edit"],
            Some("Cmd+Alt+U"),
            "{count?: 1..1000, dx?, dy?, rows?, columns?, ids?} — copies of the selection (or ids) offset by (dx, dy); with rows/columns, a grid of at most 1000 cells, and at most 1000 item copies including group contents → {created, ids: the copies}",
            has_selection,
            step_and_repeat
        ),
        cmd!("path.create", "Create Path", [], None, "{spread?, anchors: [{p:[x,y], in?:[x,y], out?:[x,y]}], closed?: bool}", has_doc, path_create),
        cmd!(
            "path.appendAnchor",
            "Add Anchor",
            [],
            None,
            "{id, anchor: {p, in?, out?} (spread coords), subpath?: index (default the last), at?: end|start (default end)} — after the subpath's last anchor, or before its first with `at: start`",
            has_doc,
            path_append
        ),
        cmd!(
            "path.setAnchor",
            "Set Anchor",
            [],
            None,
            "{id, subpath?: index (default the last), anchor: index|\"first\"|\"last\", p?, in?, out? (spread coords)} — moves an anchor or its direction lines; with both lines given and collinear it becomes smooth",
            has_doc,
            path_set_anchor
        ),
        cmd!(
            "path.reverse",
            "Reverse Path",
            ["Object", "Paths"],
            None,
            "{ids?, subpath?: index (only that subpath of the first target)} — the anchors run the other way (the end point becomes the start)",
            has_selection,
            path_reverse
        ),
        cmd!("path.close", "Close Path", ["Object", "Paths"], None, "{id, subpath?: index (default every subpath)}", has_doc, path_close),
        cmd!("path.moveAnchors", "Move Anchors", [], None, "{id, anchors: [[subpath, index]], dx, dy, handle?: in|out}", has_doc, path_move_anchors),
        cmd!(
            "object.align",
            "Align",
            ["Window", "Object & Layout", "Align"],
            None,
            "{edge: left|hcenter|right|top|vcenter|bottom, to?: selection|keyObject|margins|page|spread, ids?}",
            has_selection,
            align
        ),
        cmd!(
            "object.distribute",
            "Distribute",
            ["Window", "Object & Layout", "Align"],
            None,
            "{axis: horizontal|vertical, by?: centers|spacing, spacing?: points, ids?}",
            has_selection,
            distribute
        ),
        cmd!(
            noundo "tool.polygonSettings",
            "Polygon Settings…",
            [],
            None,
            "{sides?: 3–100, starInset?: 0–100 (%)} → the settings new polygons use",
            super::always,
            |s, p| {
                if let Some(n) = p.get("sides").and_then(Value::as_u64) {
                    s.prefs.polygon_sides = (n as u32).clamp(3, 100);
                }
                if let Some(v) = p.get("starInset").and_then(Value::as_f64) {
                    s.prefs.star_inset = (v / 100.0).clamp(0.0, 1.0);
                }
                Ok(json!({"sides": s.prefs.polygon_sides, "starInset": (s.prefs.star_inset * 100.0).round()}))
            }
        ),
        cmd!(
            "gap.move",
            "Move Gap",
            [],
            None,
            "{at: [x, y] (spread), delta?: pt | dx? | dy?: a drag (the part along the gap's axis counts), spread?} — Gap tool: the gap at `at` between objects (or an object and the page edge) moves by `delta`; the objects on both sides resize → {axis, changed}",
            has_doc,
            gap_move
        ),
        cmd!(
            "frame.grid",
            "Gridify",
            [],
            None,
            "{rect, cols, rows, gutter? (pt, 12), shape?, content?, spread?} — a grid of frames filling `rect` (frame tools: arrow keys while dragging)",
            has_doc,
            |s, p| {
                let rect = rect_param(p, "rect").ok_or_else(|| bad("frame.grid", "missing rect"))?;
                let cols = p.get("cols").and_then(Value::as_u64).unwrap_or(1).clamp(1, 100) as usize;
                let rows = p.get("rows").and_then(Value::as_u64).unwrap_or(1).clamp(1, 100) as usize;
                let gutter = p.get("gutter").and_then(Value::as_f64).unwrap_or(12.0).max(0.0);
                let cw = ((rect.width() - gutter * (cols - 1) as f64) / cols as f64).max(0.5);
                let ch = ((rect.height() - gutter * (rows - 1) as f64) / rows as f64).max(0.5);
                let mut ids = Vec::new();
                for r in 0..rows {
                    for c in 0..cols {
                        let x0 = rect.x0 + c as f64 * (cw + gutter);
                        let y0 = rect.y0 + r as f64 * (ch + gutter);
                        let q = super::with_param(&super::with_param(p, "rect", json!([x0, y0, x0 + cw, y0 + ch])), "caret", json!(false));
                        ids.push(frame_create(s, &q)?["id"].clone());
                    }
                }
                let sel: Vec<ItemId> = ids.iter().filter_map(Value::as_u64).map(ItemId).collect();
                s.doc_mut()?.selection = designcraft_doc::Selection::items(sel);
                Ok(json!({"ids": ids}))
            }
        ),
        cmd!(
            "object.clippingPath",
            "Clipping Path…",
            ["Object", "Clipping Path"],
            None,
            "{type?: alpha|edges (default alpha), threshold?: 0–255 (alpha: opaque above; edges: darker than 255 − threshold counts; default 25), tolerance?: px (2), ids?} — traces the placed graphic and makes the outline the frame (Convert Clipping Path to Frame)",
            has_selection,
            clipping_path
        ),
        cmd!(
            "object.clippingPathOptions",
            "Clipping Path Options…",
            ["Object", "Clipping Path"],
            None,
            "{type?: none, invert?: bool, includeInsideEdges?: bool, ids?} — without type, invert or includeInsideEdges, reports each selected graphic's clipping path ({items: [{id, type: none|detectEdges|alphaChannel|photoshopPath|userModifiedPath, anchors, invert, includeInsideEdges, threshold, tolerance, inset, sourceName}]}); type none removes it, invert shows the graphic outside it instead of inside",
            has_selection,
            clipping_path_options
        ),
        cmd!(
            "object.primaryTextFrame",
            "Primary Text Frame",
            ["Object"],
            None,
            "{on?: bool (default: toggle), ids?} — the selected text frame's story becomes the primary story (Smart Text Reflow adds and removes pages for it)",
            has_selection,
            |s, p| {
                let ids = targets(s, p)?;
                let sid = ids
                    .iter()
                    .find_map(|id| s.doc().ok()?.doc.item(*id)?.text_frame().map(|t| t.story))
                    .ok_or_else(|| bad("object.primaryTextFrame", "select a text frame"))?;
                let cur = s.doc()?.doc.settings.primary_story;
                let on = p.get("on").and_then(Value::as_bool).unwrap_or(cur != Some(sid));
                s.edit(|d, _| {
                    d.settings.primary_story = if on {
                        Some(sid)
                    } else if cur == Some(sid) {
                        None
                    } else {
                        cur
                    };
                    Ok(json!({"primary": on, "story": sid.0}))
                })
            }
        ),
        cmd!(
            "object.attributes",
            "Attributes",
            ["Window", "Output", "Attributes"],
            None,
            "{overprintFill?, overprintStroke?, overprintGap?, nonprinting?: bool, ids?} — Attributes panel",
            has_selection,
            |s, p| {
                let b = |k: &str| p.get(k).and_then(Value::as_bool);
                let (of, os, og, np) = (b("overprintFill"), b("overprintStroke"), b("overprintGap"), b("nonprinting"));
                set_flag(
                    s,
                    p,
                    move |i| {
                        if let Some(v) = of {
                            i.fill.overprint = v;
                        }
                        if let Some(v) = os {
                            i.stroke.overprint = v;
                        }
                        if let Some(v) = og {
                            i.stroke.gap_overprint = v;
                        }
                        if let Some(v) = np {
                            i.nonprinting = v;
                        }
                    },
                    false,
                )
            }
        ),
        cmd!(
            "object.globalLight",
            "Global Light…",
            ["Object", "Effects"],
            None,
            "{angle} — the light angle shared by shadows that use Global Light",
            has_doc,
            |s, p| {
                let a = p.get("angle").and_then(Value::as_f64).ok_or_else(|| bad("object.globalLight", "angle required"))?;
                let a = (a + 180.0).rem_euclid(360.0) - 180.0;
                s.edit(|d, _| {
                    d.settings.global_light = a;
                    Ok(json!({"angle": a}))
                })
            }
        ),
        cmd!(
            "object.label",
            "Script Label",
            ["Window", "Utilities", "Script Label"],
            None,
            "{label, ids?} — free text scripts and agents can find objects by",
            has_selection,
            |s, p| {
                let label = str_param(p, "label").unwrap_or("").to_string();
                set_flag(s, p, move |i| i.label = label.clone(), false)
            }
        ),
        cmd!(
            "object.altText",
            "Object Export Options",
            ["Object"],
            None,
            "{text, ids?} — alternative text for tagged PDF and EPUB",
            has_selection,
            |s, p| {
                let text = str_param(p, "text").unwrap_or("").to_string();
                set_flag(s, p, move |i| i.alt_text = text.clone(), false)
            }
        ),
        cmd!(
            "object.transparencyGroup",
            "Group Transparency",
            [],
            None,
            "{isolate?: bool, knockout?: bool, ids?} — Effects panel: Isolate Blending, Knockout Group",
            has_selection,
            |s, p| {
                let iso = p.get("isolate").and_then(Value::as_bool);
                let ko = p.get("knockout").and_then(Value::as_bool);
                set_flag(
                    s,
                    p,
                    move |i| {
                        if let Some(v) = iso {
                            i.isolate = v;
                        }
                        if let Some(v) = ko {
                            i.knockout = v;
                        }
                    },
                    false,
                )
            }
        ),
        cmd!(query "object.findByLabel", "Find by Script Label", [], None, "{label} → ids of objects whose label is `label`", has_doc, |s, p| {
            let label = str_param(p, "label").unwrap_or("");
            let d = &s.doc()?.doc;
            let mut ids = Vec::new();
            for sp in d.spreads.iter().chain(d.parents.iter()) {
                for it in &sp.items {
                    it.walk(&mut |x| {
                        if x.label == label {
                            ids.push(x.id.0);
                        }
                    });
                }
            }
            Ok(json!({"ids": ids}))
        }),
        cmd!(
            "object.setLayer",
            "Move to Layer",
            [],
            None,
            "{layer, ids?} — a group member moves its whole top-level group",
            has_selection,
            set_layer
        ),
        cmd!(
            "object.reorder",
            "Reorder Objects",
            [],
            None,
            "{ids?, above?: id | below?: id | index?: n (0 = top of the layer), layer?} — moves the objects (siblings on one spread) in the stacking order: in front of `above` or behind `below` (onto that object's layer), or to `index` in `layer` counted from the top (default: the first object's layer, index 0). Group members stay in their group. Leaves the document (and undo history) alone when nothing would change.",
            has_doc,
            reorder
        ),
    ]
}

/// No objects named and none selected (and no text): fill and stroke go to new shapes.
pub(crate) fn nothing_targeted(s: &Session, p: &Value) -> Result<bool> {
    let st = s.doc()?;
    Ok(p.get("ids").is_none()
        && p.get("id").is_none()
        && st.selection.items.is_empty()
        && st.selection.text.is_none()
        && st.selection.cells.is_none())
}

/// Change the colours new shapes are drawn with (one undo step).
pub(crate) fn set_new_object(s: &mut Session, f: impl FnOnce(&mut designcraft_doc::NewObjectColors)) -> Result<Value> {
    s.edit(|d, _| {
        f(&mut d.settings.new_object);
        let c = &d.settings.new_object;
        Ok(json!({"newObject": {"fill": c.fill, "fillTint": c.fill_tint, "stroke": c.stroke, "strokeTint": c.stroke_tint}}))
    })
}

/// A new shape's fill and stroke: the document's colours for new objects (a swatch deleted
/// since leaves the shape's own).
fn new_object_colors(d: &designcraft_doc::Document, it: &mut Item) {
    let c = &d.settings.new_object;
    let known = |sw: &str| sw == designcraft_color::swatch::NONE || d.swatches.iter().any(|w| w.name == sw);
    if known(&c.fill) {
        it.fill = Fill { swatch: c.fill.clone(), tint: c.fill_tint, ..it.fill.clone() };
    }
    if known(&c.stroke) {
        it.stroke.swatch = c.stroke.clone();
        it.stroke.tint = c.stroke_tint;
    }
}

fn frame_create(s: &mut Session, p: &Value) -> Result<Value> {
    let rect = rect_param(p, "rect").ok_or_else(|| bad("frame.create", "missing rect"))?;
    let shape = str_param(p, "shape").unwrap_or("rectangle");
    let content = str_param(p, "content").unwrap_or("graphic");
    let sr = spread_param(p, "frame.create")?;
    let lid = s.doc()?.active_layer;
    let text = super::text_param(p, "text");
    let caret = bool_or(p, "caret", content == "text");
    let vertical = bool_or(p, "vertical", false);
    let sides = p.get("sides").and_then(Value::as_u64).map_or(s.prefs.polygon_sides, |v| v as u32).clamp(3, 100);
    let inset = p.get("starInset").and_then(Value::as_f64).map_or(s.prefs.star_inset, |v| v / 100.0).clamp(0.0, 1.0);
    let rect = Rect::new(rect.x0, rect.y0, rect.x1.max(rect.x0 + 0.5), rect.y1.max(rect.y0 + 0.5));
    s.edit(|d, sel| {
        if d.spread(sr).is_none() {
            return Err(bad("frame.create", "no such spread"));
        }
        let (path, sh) = match shape {
            "ellipse" | "oval" => (shapes::ellipse(rect), Shape::Oval),
            "polygon" => (polygon_in(rect, sides, inset), Shape::Polygon),
            _ => (shapes::rectangle(rect), Shape::Rectangle),
        };
        if content == "text" {
            let (id, sid) = d.add_text_frame(sr, rect, lid, &text, ParaFormat { style: d.styles.default_paragraph.clone(), ..Default::default() })?;
            if let Some(it) = d.item_mut(id) {
                it.path = path;
                it.shape = sh;
            }
            super::text::set_story_direction(d, &[sid], vertical);
            *sel = if caret {
                Selection::text(TextSel { story: sid, anchor: text.len(), focus: text.len(), frame: Some(id), cell: None })
            } else {
                Selection::items(vec![id])
            };
            return Ok(json!({"id": id.0, "story": sid.0}));
        }
        let id = ItemId(d.alloc());
        let mut it = Item::new(id, lid, sh, path);
        if content == "graphic" {
            it.object_style = d.styles.default_graphic_frame.clone();
            if let Some(e) = d.styles.object_effects(&it.object_style) {
                it.effects = e.clone();
            }
        } else {
            // Shapes drawn with Rectangle/Ellipse/Polygon get a 1 pt stroke in the document's
            // colours for new objects (no fill and black at first).
            it.stroke = Stroke::default();
            new_object_colors(d, &mut it);
        }
        d.insert_item(sr, it, None)?;
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })
}

/// A polygon (or, with a star inset, a star with `sides` points) fitted to `r`.
pub(crate) fn polygon_in(r: Rect, sides: u32, inset: f64) -> designcraft_geom::PathData {
    let c = r.center();
    let mut p =
        if inset > 0.0 { shapes::star(Point::ZERO, 1.0, (1.0 - inset).max(0.0), sides, 0.0) } else { shapes::polygon(Point::ZERO, 1.0, sides, 0.0) };
    // Fit the unit polygon's bounds to the rect.
    if let Some(b) = p.bounds() {
        let a = Affine::translate(c.to_vec2())
            * Affine::scale_non_uniform(r.width() / b.width().max(1e-9), r.height() / b.height().max(1e-9))
            * Affine::translate(-b.center().to_vec2());
        p.transform(a);
    }
    p
}

fn line_create(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point_param(p, "a").ok_or_else(|| bad("line.create", "missing a"))?;
    let b = point_param(p, "b").ok_or_else(|| bad("line.create", "missing b"))?;
    let sr = spread_param(p, "line.create")?;
    let lid = s.doc()?.active_layer;
    s.edit(|d, sel| {
        let id = ItemId(d.alloc());
        let mut it = Item::new(id, lid, Shape::GraphicLine, shapes::line(a, b));
        it.stroke = Stroke::default();
        new_object_colors(d, &mut it);
        d.insert_item(sr, it, None)?;
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })
}

/// Deep-copy items `ids` from `src` into `dst` spread `to` (fresh ids; text frames get story copies).
pub fn duplicate_from(dst: &mut Document, src: &Document, ids: &[ItemId], to: SpreadRef, off: Vec2) -> Result<Vec<ItemId>> {
    let mut out = Vec::new();
    let mut story_map: HashMap<StoryId, StoryId> = HashMap::new();
    for id in ids {
        let Some(it) = src.item(*id) else { continue };
        let mut copy = it.clone();
        copy.xf = Affine::translate(off) * copy.xf;
        renumber(dst, src, &mut copy, &mut story_map);
        if dst.layer(copy.layer).is_none() {
            copy.layer = dst.default_layer();
        }
        let nid = copy.id;
        dst.insert_item(to, copy, None)?;
        out.push(nid);
    }
    Ok(out)
}

/// The id to reference in `dst` for asset `id` of `src`. When `src` lacks the asset (a missing graphic), the reference
/// gets a fresh id from `dst`, so it stays missing: `src`'s id may already name, or later be allocated to, another
/// image in `dst`.
fn adopt_or_dangle(dst: &mut Document, src: &Document, id: AssetId) -> AssetId {
    dst.adopt_asset(src, id).unwrap_or_else(|| AssetId(dst.alloc()))
}

/// Give `it` (an item of `src`) fresh ids from `dst`, with copies of its stories and the assets it uses.
pub(crate) fn renumber(dst: &mut Document, src: &Document, it: &mut Item, stories: &mut HashMap<StoryId, StoryId>) {
    it.id = ItemId(dst.alloc());
    let my_id = it.id;
    if let Some(m) = it.media.as_mut()
        && let Some(p) = m.poster
    {
        m.poster = Some(adopt_or_dangle(dst, src, p));
    }
    match &mut it.content {
        Content::Text(tf) => {
            let new_sid = match stories.get(&tf.story) {
                Some(n) => *n,
                None => {
                    let n = StoryId(dst.alloc());
                    let mut st = src.story(tf.story).cloned().unwrap_or_else(|| Story::new(n));
                    st.id = n;
                    let mut assets = HashMap::new();
                    for a in designcraft_doc::story_asset_refs(&st) {
                        let na = adopt_or_dangle(dst, src, a);
                        if na != a {
                            assets.insert(a, na);
                        }
                    }
                    designcraft_doc::remap_story_assets(&mut st, &assets);
                    st.frames.clear();
                    dst.stories.insert(n, Arc::new(st));
                    stories.insert(tf.story, n);
                    n
                }
            };
            tf.story = new_sid;
            if let Some(st) = dst.story_mut(new_sid) {
                st.frames.push(my_id);
            }
        }
        Content::Graphic(g) => {
            g.asset = adopt_or_dangle(dst, src, g.asset);
        }
        Content::Group { items } => {
            for c in items.iter_mut() {
                renumber(dst, src, Arc::make_mut(c), stories);
            }
        }
        Content::Unassigned => {}
    }
}

fn transform_move(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let (dx, dy) = (f64_or(p, "dx", 0.0), f64_or(p, "dy", 0.0));
    let copy = bool_or(p, "copy", false);
    let explicit = p.get("ids").is_some() || p.get("id").is_some();
    // Duplicating copies the frame; moving a content selection moves what the frame holds.
    let content = p.get("content").and_then(Value::as_bool).unwrap_or(!explicit && !copy && s.doc()?.selection.content);
    if content {
        return move_content(s, &ids, Vec2::new(dx, dy));
    }
    let to: Option<SpreadRef> = p.get("toSpread").and_then(|v| serde_json::from_value(v.clone()).ok());
    s.edit(|d, sel| {
        let ids = if copy {
            let src = d.clone();
            let mut new = Vec::new();
            for id in &ids {
                let sr = d.find(*id).map(|l| l.spread).unwrap_or(SpreadRef::Doc(0));
                new.extend(duplicate_from(d, &src, &[*id], to.unwrap_or(sr), Vec2::ZERO)?);
            }
            *sel = Selection::items(new.clone());
            new
        } else {
            ids
        };
        for id in &ids {
            if let Some(target) = to
                && !copy
                && let Some(loc) = d.find(*id)
                && loc.spread != target
                && loc.path.len() == 1
            {
                let it = d.remove_item_keep_story(*id)?;
                d.insert_item(target, it, None)?;
            }
            // A nested item moves in its container's space.
            let v = d.find(*id).map_or(Vec2::new(dx, dy), |l| linear_inverse(d.parent_xf(&l), Vec2::new(dx, dy)));
            let it = d.item_mut(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            if it.locked {
                continue;
            }
            it.xf = Affine::translate(v) * it.xf;
        }
        // An object dragged inside a Flex Layout takes the place it was dropped on.
        if !copy {
            for id in &ids {
                super::flex::drop_in_container(d, *id);
            }
        }
        Ok(json!({"moved": ids.len(), "ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}))
    })
}

/// `v` (a spread-space displacement) in the space `m` maps from. Degenerate maps leave it as is.
fn linear_inverse(m: Affine, v: Vec2) -> Vec2 {
    let c = m.as_coeffs();
    let lin = Affine::new([c[0], c[1], c[2], c[3], 0.0, 0.0]);
    if lin.determinant().abs() < 1e-12 {
        return v;
    }
    let r = lin.inverse() * Point::new(v.x, v.y);
    if r.x.is_finite() && r.y.is_finite() { r.to_vec2() } else { v }
}

/// Move what each frame holds — its placed graphic or the items pasted into it — by the
/// spread-space displacement `d`; the frame stays, so the visible part of the content changes.
/// Frames without such content (and groups) move as a whole. Auto-fit stays as it is: the next
/// frame resize refits.
fn move_content(s: &mut Session, ids: &[ItemId], d: Vec2) -> Result<Value> {
    s.edit(|doc, _| {
        let mut moved = Vec::new();
        for id in ids {
            let loc = doc.find(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            let parent = doc.parent_xf(&loc);
            let it = doc.item_mut(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            if it.locked {
                continue;
            }
            let inner = linear_inverse(parent * it.xf, d);
            let nested = it.has_nested_items();
            match &mut it.content {
                Content::Graphic(g) => g.xf = Affine::translate(inner) * g.xf,
                Content::Group { items } if nested => {
                    for c in items.iter_mut() {
                        let c = Arc::make_mut(c);
                        c.xf = Affine::translate(inner) * c.xf;
                    }
                }
                _ => it.xf = Affine::translate(linear_inverse(parent, d)) * it.xf,
            }
            moved.push(id.0);
        }
        Ok(json!({"moved": moved.len(), "ids": moved}))
    })
}

fn transform_resize(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let from = rect_param(p, "from").ok_or_else(|| bad("transform.resize", "missing from"))?;
    let to = rect_param(p, "to").ok_or_else(|| bad("transform.resize", "missing to"))?;
    let scale_content = bool_or(p, "content", false);
    let distribute = bool_or(p, "distribute", false);
    // A flat source (a horizontal or vertical line) has no size to scale on its flat axis: that axis only moves.
    let ratio = |new: f64, old: f64| if old.abs() < 1e-9 { 1.0 } else { new / old };
    let sx = ratio(to.width(), from.width());
    let sy = ratio(to.height(), from.height());
    let sx = if sx.abs() < 1e-3 { 1e-3f64.copysign(sx) } else { sx };
    let sy = if sy.abs() < 1e-3 { 1e-3f64.copysign(sy) } else { sy };
    let m = Affine::translate((to.x0, to.y0)) * Affine::scale_non_uniform(sx, sy) * Affine::translate((-from.x0, -from.y0));
    let strokes = s.prefs.scale_strokes;
    s.edit(|d, _| {
        for id in &ids {
            let it = d.item_mut(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            if it.locked {
                continue;
            }
            if distribute {
                let c = it.bounds().center();
                it.xf = Affine::translate(m * c - c) * it.xf;
            } else if scale_content {
                scale_object(d, *id, m, (sx * sy).abs().sqrt(), strokes);
            } else if it.flex.is_some() {
                resize_container(it, m);
            } else if matches!(it.content, Content::Group { .. }) {
                it.xf = m * it.xf;
            } else {
                // Resize the frame's path (in inner space); content keeps its size (InDesign's
                // default) unless the frame auto-fits it.
                let inner = it.xf.inverse() * m * it.xf;
                bake(it, inner);
                let r = it.inner_bounds();
                if let Content::Graphic(g) = &mut it.content
                    && let Some(xf) = g.fitted(r, g.auto_fit)
                {
                    g.xf = xf;
                }
            }
        }
        Ok(json!({"resized": ids.len()}))
    })
}

/// A Flex Layout container takes the new box (its objects keep their size; the layout places them),
/// and a side whose length changes becomes Fixed, as dragging a container's handle does in InDesign.
fn resize_container(it: &mut Item, m: Affine) {
    let before = it.inner_bounds();
    let inner = it.xf.inverse() * m * it.xf;
    bake(it, inner);
    let after = it.inner_bounds();
    if let Some(f) = it.flex.as_mut() {
        if (after.width() - before.width()).abs() > 1e-6 {
            f.width = designcraft_doc::FlexSize::Fixed;
        }
        if (after.height() - before.height()).abs() > 1e-6 {
            f.height = designcraft_doc::FlexSize::Fixed;
        }
    }
}

fn union_bounds(d: &Document, ids: &[ItemId]) -> Rect {
    ids.iter().filter_map(|i| d.item(*i)).map(Item::bounds).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO)
}

/// Transform an item's geometry in its own space: the path and the gradient vectors (fill and
/// stroke) with it.
pub(crate) fn bake(it: &mut Item, inner: Affine) {
    it.path.transform(inner);
    if let Some(w) = &mut it.wrap.path {
        w.transform(inner);
    }
    for v in [&mut it.fill.gradient_vector, &mut it.stroke.gradient_vector] {
        if let Some([x0, y0, x1, y1]) = *v {
            let (a, b) = (inner * Point::new(x0, y0), inner * Point::new(x1, y1));
            *v = Some([a.x, a.y, b.x, b.y]);
        }
    }
}

/// Multiply the stroke weights of an item and everything nested in it.
fn scale_weights(it: &mut Item, k: f64) {
    it.stroke.weight *= k;
    if let Content::Group { items } = &mut it.content {
        for c in items.iter_mut() {
            scale_weights(Arc::make_mut(c), k);
        }
    }
}

/// Scale an item and its content by the spread-space transform `m` (InDesign's "Apply to
/// Content"): paths and placed graphics take the scale into their geometry, so the stroke weight
/// is scaled by `k` (the mean scale factor) only with Scale Strokes on; text frames and groups are
/// transformed as a whole, so with Scale Strokes off their weights are divided by `k` to keep
/// their look.
pub(crate) fn scale_item(it: &mut Item, m: Affine, k: f64, strokes: bool) {
    let inner = it.xf.inverse() * m * it.xf;
    match &mut it.content {
        Content::Unassigned => {
            bake(it, inner);
            if strokes {
                it.stroke.weight *= k;
            }
        }
        Content::Graphic(g) => {
            g.xf = inner * g.xf;
            bake(it, inner);
            if strokes {
                it.stroke.weight *= k;
            }
        }
        Content::Text(_) | Content::Group { .. } => {
            it.xf = m * it.xf;
            if !strokes && k > 1e-9 {
                scale_weights(it, 1.0 / k);
            }
        }
    }
}

/// Scale the object `id` and its content by `m` (see [`scale_item`]); a text frame's text takes the
/// scale as [`scale_text_frame`] sets it.
fn scale_object(d: &mut Document, id: ItemId, m: Affine, k: f64, strokes: bool) {
    if scale_text_frame(d, id, m, k, strokes) {
        return;
    }
    if let Some(it) = d.item_mut(id) {
        scale_item(it, m, k, strokes);
    }
}

/// Scale a text frame as InDesign's default "When Scaling: Apply to Content" does: the frame's
/// path takes the scale, so it shows 100%, and the text is set larger or smaller. The point size,
/// leading, baseline shift, text stroke and underline weights follow the vertical scale, the
/// horizontal scale takes the difference between the two axes, and the indents and paragraph
/// spacing follow their own axes. False (the frame is then transformed as a whole) for a story that
/// runs through other frames, vertical text, and a scale that flips or isn't along the frame's own
/// axes.
fn scale_text_frame(d: &mut Document, id: ItemId, m: Affine, k: f64, strokes: bool) -> bool {
    let Some(it) = d.item(id) else { return false };
    let Content::Text(tf) = &it.content else { return false };
    let story = tf.story;
    let inner = it.xf.inverse() * m * it.xf;
    let [sx, b, c, sy, _, _] = inner.as_coeffs();
    if b.abs() > 1e-9 || c.abs() > 1e-9 || !(sx > 1e-6 && sy > 1e-6) || !sx.is_finite() || !sy.is_finite() {
        return false;
    }
    let Some(st) = d.story(story) else { return false };
    // Tables, anchored objects and footnotes keep their own sizes, so a frame holding any of them
    // scales as a whole.
    if st.frames.len() != 1 || st.vertical || !st.tables.is_empty() || !st.objects.is_empty() || !st.notes.is_empty() {
        return false;
    }
    // The resolved attributes of every run, split at paragraph ends (a run's size can come from its
    // paragraph's style). Each paragraph keeps its mark.
    let mut runs = Vec::new();
    let mut paras = Vec::new();
    for (i, pr) in st.para_ranges().into_iter().enumerate() {
        let Some(pf) = st.paras.get(i) else { break };
        let (pp, base) = d.styles.resolve_para(pf);
        let end = pr.end.saturating_add(1).min(st.text.len());
        for (r, f) in st.runs() {
            let (a, z) = (r.start.max(pr.start), r.end.min(end));
            if a < z {
                runs.push((a..z, d.styles.resolve_char(&base, f)));
            }
        }
        paras.push((pr.start, pp));
    }
    let h = sx / sy;
    let Some(st) = d.story_mut(story) else { return false };
    for (r, cp) in runs {
        st.format_chars(r, |f| {
            let o = &mut f.over;
            o.size = Some(cp.size * sy);
            if let designcraft_doc::Leading::Points(v) = cp.leading {
                o.leading = Some(designcraft_doc::Leading::Points(v * sy));
            }
            if cp.baseline_shift != 0.0 {
                o.baseline_shift = Some(cp.baseline_shift * sy);
            }
            if (h - 1.0).abs() > 1e-9 {
                o.h_scale = Some(cp.h_scale * h);
            }
            if cp.stroke != "[None]" {
                o.stroke_weight = Some(cp.stroke_weight * sy);
            }
            for (v, w) in [
                (&mut o.underline_weight, cp.underline_weight),
                (&mut o.underline_offset, cp.underline_offset),
                (&mut o.strikethrough_weight, cp.strikethrough_weight),
                (&mut o.strikethrough_offset, cp.strikethrough_offset),
            ] {
                if let Some(w) = w {
                    *v = Some(Some(w * sy));
                }
            }
        });
    }
    for (at, pp) in paras {
        st.format_paras(at..at, |p| {
            let a = &mut p.para;
            for (v, w, s) in [
                (&mut a.left_indent, pp.left_indent, sx),
                (&mut a.right_indent, pp.right_indent, sx),
                (&mut a.first_line_indent, pp.first_line_indent, sx),
                (&mut a.last_line_indent, pp.last_line_indent, sx),
                (&mut a.space_before, pp.space_before, sy),
                (&mut a.space_after, pp.space_after, sy),
            ] {
                if w != 0.0 {
                    *v = Some(w * s);
                }
            }
        });
    }
    let Some(it) = d.item_mut(id) else { return false };
    bake(it, inner);
    if strokes {
        it.stroke.weight *= k;
    }
    true
}

fn transform_again(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let (list, _, _) = &s.transforms;
    if list.is_empty() {
        return Err(bad("transform.again", "no transform to repeat"));
    }
    let steps: Vec<(String, Value)> = if bool_or(p, "sequence", false) { list.clone() } else { list.last().cloned().into_iter().collect() };
    let mut groups: Vec<Vec<ItemId>> = if bool_or(p, "individually", false) { ids.iter().map(|i| vec![*i]).collect() } else { vec![ids.clone()] };
    // Run the steps directly (no recording, one undo step for the whole repeat).
    let saved = s.transforms.clone();
    for (cmd, params) in &steps {
        let spec = super::find_command(cmd).ok_or_else(|| bad("transform.again", "unknown transform"))?;
        for g in &groups {
            let q = super::with_param(params, "ids", json!(g.iter().map(|i| i.0).collect::<Vec<_>>()));
            (spec.run)(s, &q)?;
        }
        // Moving a copy selects the copies: later steps (and the next repeat) act on them.
        if bool_or(params, "copy", false) {
            groups = vec![s.doc()?.selection.items.clone()];
        }
    }
    s.transforms = saved;
    Ok(json!({"repeated": steps.len()}))
}

fn clear_transformations(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, _| {
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            if matches!(it.content, Content::Group { .. }) && it.shape == Shape::Group {
                continue;
            }
            let c = it.bounds().center();
            let inner = it.inner_bounds().center();
            it.xf = Affine::translate(c - inner);
        }
        ok()
    })
}

/// Apply `a` to the targets about the `ref` point (0..8, default the centre) of their combined bounds.
fn apply_about_ref(s: &mut Session, p: &Value, a: Affine) -> Result<Value> {
    let ids = targets(s, p)?;
    let rf = p.get("ref").and_then(Value::as_u64).unwrap_or(4).min(8) as usize;
    s.edit(|d, _| {
        let c = designcraft_geom::reference_point(union_bounds(d, &ids), rf);
        let m = Affine::translate(c.to_vec2()) * a * Affine::translate(-c.to_vec2());
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                it.xf = m * it.xf;
            }
        }
        ok()
    })
}

fn transform_rotate(s: &mut Session, p: &Value) -> Result<Value> {
    let a = f64_or(p, "angle", 0.0);
    apply_about_ref(s, p, Affine::rotate(-a.to_radians()))
}

/// The transform an item's panel values are measured from: its own (relative to its container)
/// or, with Transformations are Totals, the whole chain to the pasteboard.
fn measured_xf(d: &designcraft_doc::Document, id: ItemId, totals: bool) -> Option<(Affine, Affine)> {
    let loc = d.find(id)?;
    let it = d.item_at(&loc)?;
    let parent = if totals { d.parent_xf(&loc) } else { Affine::IDENTITY };
    Some((parent, parent * it.xf))
}

fn values_json(m: Affine) -> Value {
    let v = designcraft_geom::decompose::decompose(m);
    let r = |x: f64| (x * 1e4).round() / 1e4;
    json!({"scaleX": r(v.scale_x * 100.0), "scaleY": r(v.scale_y * 100.0), "rotation": r(v.rotation), "shear": r(v.shear)})
}

/// Whether `transform.set` / `transform.info` act on the placed graphic in each frame: `content`
/// when given, else whether the selection is content (Direct Selection) when no `ids` or `id` name
/// the targets.
fn content_mode(s: &Session, p: &Value) -> Result<bool> {
    if let Some(c) = p.get("content").and_then(Value::as_bool) {
        return Ok(c);
    }
    let explicit = p.get("ids").is_some() || p.get("id").is_some();
    Ok(!explicit && s.doc()?.selection.content)
}

fn transform_info(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let totals = s.prefs.transformations_are_totals;
    let content = content_mode(s, p)?;
    let d = &s.doc()?.doc;
    let id = *ids.first().ok_or_else(|| bad("transform.info", "nothing selected"))?;
    let (_, m) = measured_xf(d, id, totals).ok_or_else(|| bad("transform.info", "no such object"))?;
    let mut out = values_json(m);
    if let Some(g) = d.item(id).and_then(Item::graphic) {
        let c = values_json(if totals { m * g.xf } else { g.xf });
        // With the graphic selected, its values are the ones the Transform fields show and set.
        if content {
            out = c.clone();
        }
        out["content"] = c;
    }
    Ok(out)
}

/// Absolute rotation / shear / scale from the Transform panel, about the reference point.
fn transform_values(s: &mut Session, p: &Value, ids: &[ItemId], rf: usize) -> Result<Value> {
    let totals = s.prefs.transformations_are_totals;
    let strokes = s.prefs.scale_strokes;
    let get = |k: &str| p.get(k).and_then(Value::as_f64);
    let (rot, shear, sx, sy) = (get("rotation"), get("shear").map(|v| v.clamp(-85.0, 85.0)), get("scaleX"), get("scaleY"));
    if [sx, sy].iter().flatten().any(|v| v.abs() < 0.01) {
        return Err(bad("transform.set", "scale too small"));
    }
    s.edit(|d, _| {
        for id in ids {
            let Some((parent, cur)) = measured_xf(d, *id, totals) else { continue };
            let Some(it) = d.item(*id) else { continue };
            // The reference point, on the pasteboard, then in the measuring space.
            let full = d.find(*id).map_or(Affine::IDENTITY, |l| d.parent_xf(&l));
            let spread_a = designcraft_geom::reference_point((full * it.xf).transform_rect_bbox(it.inner_bounds()), rf);
            let a = if totals { spread_a } else { full.inverse() * spread_a };
            let v = designcraft_geom::decompose::decompose(cur);
            let mut t = v;
            if let Some(r) = rot {
                t.rotation = r;
            }
            if let Some(k) = shear {
                t.shear = k;
            }
            let lin = |m: Affine| {
                let c = m.as_coeffs();
                Affine::new([c[0], c[1], c[2], c[3], 0.0, 0.0])
            };
            let delta = designcraft_geom::decompose::compose(&t) * lin(cur).inverse();
            // Δ in measuring space about `a`, brought into the item's container space.
            let m = parent.inverse() * Affine::translate(a.to_vec2()) * delta * Affine::translate(-a.to_vec2()) * parent;
            let it = d.item_mut(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            it.xf = m * it.xf;
            // Scale: relative to what is shown (frames show 100% — their scale lives in the geometry).
            if sx.is_some() || sy.is_some() {
                let (fx, fy) = (sx.map_or(1.0, |x| x / 100.0 / v.scale_x), sy.map_or(1.0, |y| y / 100.0 / v.scale_y));
                let m = Affine::translate(a.to_vec2()) * Affine::scale_non_uniform(fx, fy) * Affine::translate(-a.to_vec2());
                let m = parent.inverse() * m * parent;
                scale_object(d, *id, m, (fx * fy).abs().sqrt(), strokes);
            }
        }
        ok()
    })
}

fn transform_set(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    // Direct Selection of a placed graphic (or `content`): the graphic moves inside its frame.
    if content_mode(s, p)? {
        return transform_content(s, p, &ids);
    }
    if ["rotation", "shear", "scaleX", "scaleY"].iter().any(|k| p.get(*k).is_some()) {
        let rf = p.get("ref").and_then(Value::as_u64).unwrap_or(4) as usize;
        if !["x", "y", "width", "height"].iter().any(|k| p.get(*k).is_some()) {
            return transform_values(s, p, &ids, rf);
        }
        transform_values(s, p, &ids, rf)?;
    }
    let rf = p.get("ref").and_then(Value::as_u64).unwrap_or(0) as usize;
    let with_stroke = s.prefs.dimensions_include_stroke;
    s.edit(|d, _| {
        if !ids.iter().any(|i| d.item(*i).is_some()) {
            return ok();
        }
        // The measured box (with Dimensions Include Stroke Weight, the stroke's outer edge) and
        // the geometry it pads.
        let geom = union_bounds(d, &ids);
        let from = if with_stroke {
            ids.iter().filter_map(|i| d.item(*i)).map(Item::visible_bounds).reduce(|a, b| a.union(b)).unwrap_or(geom)
        } else {
            geom
        };
        let pad = (geom.x0 - from.x0, geom.y0 - from.y0, from.x1 - geom.x1, from.y1 - geom.y1);
        let w = f64_or(p, "width", from.width()).max(0.01);
        let h = f64_or(p, "height", from.height()).max(0.01);
        let anchor = designcraft_geom::reference_point(from, rf);
        // New rect keeping the reference point fixed, then moved so the ref point lands on x/y.
        let fx = [0.0, 0.5, 1.0][rf % 3];
        let fy = [0.0, 0.5, 1.0][(rf / 3).min(2)];
        let mut to = Rect::new(anchor.x - fx * w, anchor.y - fy * h, anchor.x - fx * w + w, anchor.y - fy * h + h);
        // x/y are the reference point's position measured from the rulers' zero point.
        let origin = ids.first().and_then(|i| d.find(*i)).and_then(|l| d.ruler_origin(l.spread, from.center().x)).unwrap_or(Point::ORIGIN);
        if let Some(x) = p.get("x").and_then(Value::as_f64).filter(|v| v.is_finite()) {
            to = to + Vec2::new(x + origin.x - anchor.x, 0.0);
        }
        if let Some(y) = p.get("y").and_then(Value::as_f64).filter(|v| v.is_finite()) {
            to = to + Vec2::new(0.0, y + origin.y - anchor.y);
        }
        let to = Rect::new(to.x0 + pad.0, to.y0 + pad.1, (to.x1 - pad.2).max(to.x0 + pad.0 + 0.01), (to.y1 - pad.3).max(to.y0 + pad.1 + 0.01));
        let m = Affine::translate((to.x0, to.y0))
            * Affine::scale_non_uniform(to.width() / geom.width().max(1e-9), to.height() / geom.height().max(1e-9))
            * Affine::translate((-geom.x0, -geom.y0));
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                if it.flex.is_some() {
                    resize_container(it, m);
                } else if matches!(it.content, Content::Group { .. }) {
                    it.xf = m * it.xf;
                } else {
                    let inner = it.xf.inverse() * m * it.xf;
                    bake(it, inner);
                }
            }
        }
        ok()
    })
}

/// `transform.set` on the placed graphics of `ids` (the frames stay): rotation, shear and scale are
/// absolute, measured as `transform.info` reports the content (on the pasteboard with
/// Transformations are Totals, else in the frame); x, y, width and height are the graphic's bounds
/// on the page, about the reference point. Frames without a placed graphic are left alone.
fn transform_content(s: &mut Session, p: &Value, ids: &[ItemId]) -> Result<Value> {
    let totals = s.prefs.transformations_are_totals;
    let get = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    let (rot, shear, sx, sy) = (get("rotation"), get("shear").map(|v| v.clamp(-85.0, 85.0)), get("scaleX"), get("scaleY"));
    let (x, y, w, h) = (get("x"), get("y"), get("width").map(|v| v.max(0.01)), get("height").map(|v| v.max(0.01)));
    let bounds = rect_param(p, "bounds").filter(|r| [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()));
    if [sx, sy].iter().flatten().any(|v| v.abs() < 0.01) {
        return Err(bad("transform.set", "scale too small"));
    }
    let rf = p.get("ref").and_then(Value::as_u64).unwrap_or(4).min(8) as usize;
    {
        let d = &s.doc()?.doc;
        if !ids.iter().any(|i| d.item(*i).is_some_and(|it| it.graphic().is_some())) {
            return Err(bad("transform.set", "no placed graphic to transform"));
        }
        // Only locked frames: nothing to change, and no undo step.
        if !ids.iter().any(|i| d.item(*i).is_some_and(|it| it.graphic().is_some() && !it.locked)) {
            return Ok(json!({ "changed": 0 }));
        }
    }
    s.edit(|d, _| {
        let mut changed = 0;
        for id in ids {
            let Some(loc) = d.find(*id) else { continue };
            let full = d.parent_xf(&loc);
            let Some(it) = d.item(*id) else { continue };
            if it.locked {
                continue;
            }
            let Some(g) = it.graphic() else { continue };
            let frame = full * it.xf;
            let rect = Rect::new(0.0, 0.0, g.size.0.max(1e-9), g.size.1.max(1e-9));
            // Rotation, shear and scale in the measuring space (pasteboard or frame).
            let q = if totals { frame } else { Affine::IDENTITY };
            let mut cur = q * g.xf;
            let lin = |m: Affine| {
                let c = m.as_coeffs();
                Affine::new([c[0], c[1], c[2], c[3], 0.0, 0.0])
            };
            if rot.is_some() || shear.is_some() || sx.is_some() || sy.is_some() {
                let a = designcraft_geom::reference_point(cur.transform_rect_bbox(rect), rf).to_vec2();
                let v = designcraft_geom::decompose::decompose(cur);
                let mut t = v;
                if let Some(r) = rot {
                    t.rotation = r;
                }
                if let Some(k) = shear {
                    t.shear = k;
                }
                if let Some(x) = sx {
                    t.scale_x = x / 100.0;
                }
                if let Some(y) = sy {
                    t.scale_y = y / 100.0;
                }
                let delta = designcraft_geom::decompose::compose(&t) * lin(cur).inverse();
                cur = Affine::translate(a) * delta * Affine::translate(-a) * cur;
            }
            let mut gx = q.inverse() * cur;
            // The graphic's bounds dragged on the spread (its content handles): scale and move it so
            // its bounding box becomes `bounds` (normalized; at least 0.01 pt each way).
            if let Some(r) = bounds {
                let to =
                    Rect::new(r.x0.min(r.x1), r.y0.min(r.y1), r.x0.max(r.x1).max(r.x0.min(r.x1) + 0.01), r.y0.max(r.y1).max(r.y0.min(r.y1) + 0.01));
                let b = (frame * gx).transform_rect_bbox(rect);
                let m = Affine::translate((to.x0, to.y0))
                    * Affine::scale_non_uniform(to.width() / b.width().max(1e-9), to.height() / b.height().max(1e-9))
                    * Affine::translate((-b.x0, -b.y0));
                gx = frame.inverse() * m * frame * gx;
            }
            // Position and size: the graphic's bounds on the page.
            if x.is_some() || y.is_some() || w.is_some() || h.is_some() {
                let b = (frame * gx).transform_rect_bbox(rect);
                let a = designcraft_geom::reference_point(b, rf);
                let (fx, fy) = (w.map_or(1.0, |w| w / b.width().max(1e-9)), h.map_or(1.0, |h| h / b.height().max(1e-9)));
                // x/y are measured from the rulers' zero point, as for a frame.
                let o = d.ruler_origin(loc.spread, b.center().x).unwrap_or(Point::ORIGIN);
                let to = Point::new(x.map_or(a.x, |x| x + o.x), y.map_or(a.y, |y| y + o.y));
                let m = Affine::translate(to.to_vec2()) * Affine::scale_non_uniform(fx, fy) * Affine::translate(-a.to_vec2());
                gx = frame.inverse() * m * frame * gx;
            }
            if let Some(Content::Graphic(g)) = d.item_mut(*id).map(|it| &mut it.content) {
                g.xf = gx;
                changed += 1;
            }
        }
        Ok(json!({ "changed": changed }))
    })
}

fn arrange(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let to = str_param(p, "to").unwrap_or("front").to_string();
    s.edit(|d, _| {
        if matches!(to.as_str(), "forward" | "backward") {
            let selected: HashSet<ItemId> = ids.iter().copied().collect();
            let mut positions: Vec<_> = selected
                .iter()
                .filter_map(|id| {
                    let loc = d.find(*id)?;
                    match loc.path.as_slice() {
                        [i] => Some((loc.spread, *i)),
                        _ => None,
                    }
                })
                .collect();
            // Move the leading selected items first, independently of selection/click order.
            // Swapping only with unselected siblings leaves the other selected indices valid.
            positions.sort_by_key(|(_, i)| *i);
            let forward = to == "forward";
            if forward {
                positions.reverse();
            }
            for (spread, i) in positions {
                let Some(sp) = d.spread_mut(spread) else { continue };
                let Some(it) = sp.items.get(i) else { continue };
                let layer = it.layer;
                let sibling = if forward {
                    sp.items.iter().enumerate().skip(i.saturating_add(1)).find(|(_, it)| it.layer == layer)
                } else {
                    sp.items.iter().enumerate().take(i).rev().find(|(_, it)| it.layer == layer)
                };
                if let Some((j, it)) = sibling
                    && !selected.contains(&it.id)
                {
                    // Keep every other layer's slots and relative stacking order untouched.
                    sp.items.swap(i, j);
                }
            }
            return ok();
        }
        for id in &ids {
            let Some(loc) = d.find(*id) else { continue };
            if loc.path.len() != 1 {
                continue;
            }
            let Some(sp) = d.spread_mut(loc.spread) else { continue };
            let i = loc.path[0];
            let n = sp.items.len();
            let it = sp.items.remove(i);
            let j = match to.as_str() {
                "front" => n - 1,
                "back" => 0,
                _ => i.saturating_sub(1),
            };
            sp.items.insert(j, it);
        }
        ok()
    })
}

/// An optional non-negative integer parameter; any other value is an error.
fn opt_u64(p: &Value, cmd: &str, key: &str) -> Result<Option<u64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| bad(cmd, format!("{key} must be a non-negative integer"))),
    }
}

/// The sibling list under the group at `parent` (the spread's top level when empty).
fn siblings_mut<'a>(d: &'a mut Document, r: SpreadRef, parent: &[usize]) -> Option<&'a mut Vec<Arc<Item>>> {
    let mut items = &mut d.spread_mut(r)?.items;
    for &i in parent {
        items = Arc::make_mut(items.get_mut(i)?).children_mut()?;
    }
    Some(items)
}

fn reorder(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "object.reorder";
    let mut ids = targets(s, p)?;
    let mut seen = HashSet::new();
    ids.retain(|i| seen.insert(*i));
    let Some(&first) = ids.first() else { return Err(bad(C, "no objects")) };
    let above = opt_u64(p, C, "above")?.map(ItemId);
    let below = opt_u64(p, C, "below")?.map(ItemId);
    let index = opt_u64(p, C, "index")?;
    let layer = opt_u64(p, C, "layer")?.map(designcraft_doc::LayerId);
    if [above.is_some(), below.is_some(), index.is_some()].into_iter().filter(|b| *b).count() > 1 {
        return Err(bad(C, "give only one of above, below and index"));
    }
    let target = above.map(|t| (t, true)).or(below.map(|t| (t, false)));
    let d = s.doc()?.doc.clone();
    let (spread, parent, new_items) = {
        let no_object = |id: ItemId| bad(C, format!("no object {}", id.0));
        let loc = d.find(first).ok_or_else(|| no_object(first))?;
        let Some((_, parent)) = loc.path.split_last() else { return Err(no_object(first)) };
        let parent = parent.to_vec();
        let sibling = |id: ItemId| d.find(id).is_some_and(|l| l.spread == loc.spread && l.path.split_last().is_some_and(|(_, pp)| pp == parent));
        for id in &ids {
            if d.find(*id).is_none() {
                return Err(no_object(*id));
            }
            if !sibling(*id) {
                return Err(bad(C, "the objects must be on one spread and in one group"));
            }
        }
        let in_group = !parent.is_empty();
        let group_layer = if in_group {
            let g = d.item_at(&designcraft_doc::ItemLoc { spread: loc.spread, path: parent.clone() }).ok_or_else(|| no_object(first))?;
            if !g.states.is_empty() {
                return Err(bad(C, "the states of a multi-state object are reordered in the Object States panel"));
            }
            Some(g.layer)
        } else {
            None
        };
        let own_layer = d.item(first).map(|i| i.layer).ok_or_else(|| no_object(first))?;
        let target_layer = match target {
            Some((t, _)) => {
                if ids.contains(&t) {
                    return Err(bad(C, "an object can't move next to itself"));
                }
                if d.find(t).is_none() {
                    return Err(no_object(t));
                }
                if !sibling(t) {
                    return Err(bad(C, "the target must be on the same spread and in the same group"));
                }
                let tl = d.item(t).map(|i| i.layer).ok_or_else(|| no_object(t))?;
                if layer.is_some_and(|l| l != tl) {
                    return Err(bad(C, "layer differs from the target's layer"));
                }
                if in_group { own_layer } else { tl }
            }
            None => layer.unwrap_or(own_layer),
        };
        if d.layer(target_layer).is_none() {
            return Err(bad(C, "no such layer"));
        }
        if let Some(gl) = group_layer
            && layer.is_some_and(|l| l != gl)
        {
            return Err(bad(C, "group members stay on their group's layer"));
        }
        let items = d.spread(loc.spread).map(|sp| sp.items.clone()).ok_or_else(|| no_object(first))?;
        let items = parent.iter().try_fold(items, |items, &i| items.get(i).map(|g| g.children().to_vec())).ok_or_else(|| no_object(first))?;
        // Moved objects keep their relative stacking order.
        let moved: Vec<Arc<Item>> = items.iter().filter(|i| ids.contains(&i.id)).cloned().collect();
        let rest: Vec<Arc<Item>> = items.iter().filter(|i| !ids.contains(&i.id)).cloned().collect();
        let at = match target {
            Some((t, front)) => {
                let i = rest.iter().position(|x| x.id == t).ok_or_else(|| no_object(t))?;
                if front { i + 1 } else { i }
            }
            None => {
                let k = usize::try_from(index.unwrap_or(0)).map_err(|_| bad(C, "index out of range"))?;
                // Positions of the layer's objects (all siblings in a group), topmost first.
                let stack: Vec<usize> = rest.iter().enumerate().filter(|(_, x)| in_group || x.layer == target_layer).map(|(i, _)| i).rev().collect();
                if k > stack.len() {
                    return Err(bad(C, format!("index {k} out of range (0..={})", stack.len())));
                }
                match (stack.get(k), stack.last()) {
                    (Some(&i), _) => i + 1,
                    (None, Some(&backmost)) => backmost,
                    (None, None) => rest.len(),
                }
            }
        };
        let (back, front) = rest.split_at(at.min(rest.len()));
        let moved = moved.into_iter().map(|mut it| {
            if !in_group && it.layer != target_layer {
                set_layer_deep(Arc::make_mut(&mut it), target_layer, 0);
            }
            it
        });
        let new_items: Vec<Arc<Item>> = back.iter().cloned().chain(moved).chain(front.iter().cloned()).collect();
        if new_items.iter().zip(items.iter()).all(|(a, b)| a.id == b.id && a.layer == b.layer) {
            return ok();
        }
        (loc.spread, parent, new_items)
    };
    s.edit(|d, _| {
        let items = siblings_mut(d, spread, &parent).ok_or_else(|| bad(C, "the objects moved"))?;
        *items = new_items;
        ok()
    })
}

/// Sets the layer of `it` and its group members (nesting is bounded).
fn set_layer_deep(it: &mut Item, l: designcraft_doc::LayerId, depth: usize) {
    it.layer = l;
    if depth < 64
        && let Some(kids) = it.children_mut()
    {
        for k in kids {
            set_layer_deep(Arc::make_mut(k), l, depth + 1);
        }
    }
}

/// `object.setLayer`: moves the objects, or the top-level groups of group members, to a layer.
fn set_layer(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "object.setLayer";
    let l = designcraft_doc::LayerId(p.get("layer").and_then(Value::as_u64).ok_or_else(|| bad(C, "missing layer"))?);
    let ids = targets(s, p)?;
    s.edit(|d, _| {
        if d.layer(l).is_none() {
            return Err(bad(C, "no such layer"));
        }
        let mut tops: Vec<ItemId> = Vec::new();
        for id in &ids {
            if let Some(t) = d.top_level_of(*id)
                && !tops.contains(&t)
            {
                tops.push(t);
            }
        }
        for t in &tops {
            if let Some(it) = d.item_mut(*t) {
                set_layer_deep(it, l, 0);
            }
        }
        Ok(json!({"changed": tops.len()}))
    })
}

/// Where a group of `ids` goes: their spread, the path of the group holding them (empty at top
/// level), and their indices in that list, back to front.
struct GroupPlan {
    spread: SpreadRef,
    parent: Vec<usize>,
    indices: Vec<usize>,
}

/// Objects can be grouped when they share a spread and a parent: all top-level, or all in the
/// same group (which then holds the new group). A multi-state object keeps one state per child,
/// so objects inside one stay as they are.
fn group_plan(d: &Document, ids: &[ItemId]) -> std::result::Result<GroupPlan, String> {
    let mut seen = HashSet::new();
    let mut locs = Vec::new();
    for id in ids {
        if seen.insert(*id) {
            locs.push(d.find(*id).ok_or_else(|| format!("no object with id {}", id.0))?);
        }
    }
    if locs.len() < 2 {
        return Err("select at least two objects".into());
    }
    let mut plan: Option<GroupPlan> = None;
    for loc in locs {
        let Some((&i, parent)) = loc.path.split_last() else { return Err("object not found".into()) };
        match &mut plan {
            None => plan = Some(GroupPlan { spread: loc.spread, parent: parent.to_vec(), indices: vec![i] }),
            Some(p) if p.spread == loc.spread && p.parent == parent => p.indices.push(i),
            Some(_) => return Err(GROUP_SAME_LEVEL.into()),
        }
    }
    let Some(mut plan) = plan else { return Err("select at least two objects".into()) };
    if !plan.parent.is_empty() && d.item_at(&ItemLoc { spread: plan.spread, path: plan.parent.clone() }).is_some_and(|g| !g.states.is_empty()) {
        return Err("objects inside a multi-state object can't be grouped".into());
    }
    plan.indices.sort_unstable();
    Ok(plan)
}

const GROUP_SAME_LEVEL: &str = "objects to group must be on the same spread and at the same level (all top-level, or all in the same group)";

/// Menu state of Object › Group: the selection must be groupable.
fn can_group(s: &Session) -> std::result::Result<(), String> {
    has_selection(s)?;
    let st = s.active().ok_or_else(|| "no document open".to_string())?;
    group_plan(&st.doc, &st.selection.items).map(|_| ()).map_err(|_| super::CANT_GROUP_SELECTION.into())
}

fn group(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, sel| {
        let plan = group_plan(d, &ids).map_err(|e| bad("object.group", e))?;
        let missing = || bad("object.group", "the objects to group are gone");
        let siblings = siblings_mut(d, plan.spread, &plan.parent).ok_or_else(missing)?;
        let (Some(&back), Some(&front)) = (plan.indices.first(), plan.indices.last()) else { return Err(missing()) };
        let layer = siblings.get(back).ok_or_else(missing)?.layer;
        // Keep stacking order: remove front to back, then restore back-to-front order.
        let mut kids = Vec::with_capacity(plan.indices.len());
        for &i in plan.indices.iter().rev() {
            if i >= siblings.len() {
                return Err(missing());
            }
            kids.push(siblings.remove(i));
        }
        kids.reverse();
        let gid = ItemId(d.alloc());
        let mut g = Item::new(gid, layer, Shape::Group, designcraft_geom::PathData::default());
        let at = (front + 1).saturating_sub(kids.len());
        g.content = Content::Group { items: kids };
        let siblings = siblings_mut(d, plan.spread, &plan.parent).ok_or_else(missing)?;
        siblings.insert(at.min(siblings.len()), Arc::new(g));
        *sel = Selection::items(vec![gid]);
        Ok(json!({"id": gid.0}))
    })
}

fn ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, sel| {
        let mut out = Vec::new();
        for id in &ids {
            let Some(loc) = d.find(*id) else { continue };
            let Some(g) = d.item(*id).cloned() else { continue };
            // Only groups come apart: a frame holding pasted-into items stays whole, and objects
            // that aren't groups stay selected (as in InDesign).
            let Content::Group { items } = &g.content else {
                out.push(*id);
                continue;
            };
            if !g.is_group() || loc.path.len() != 1 {
                out.push(*id);
                continue;
            }
            let Some(sp) = d.spread_mut(loc.spread) else { continue };
            sp.items.remove(loc.path[0]);
            for (k, c) in items.iter().enumerate() {
                let mut c = (**c).clone();
                c.xf = g.xf * c.xf;
                out.push(c.id);
                sp.items.insert(loc.path[0] + k, Arc::new(c));
            }
        }
        *sel = Selection::items(out);
        ok()
    })
}

fn text_wrap(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_doc::{ContourType, WrapMode, WrapSide};
    let side: Option<WrapSide> = match p.get("side") {
        Some(v) => Some(serde_json::from_value(v.clone()).map_err(|_| bad("object.textWrap", "unknown side"))?),
        None => None,
    };
    // Detected edges, alpha channels and image paths need the image's pixels or paths, which
    // composition doesn't read; those types aren't offered.
    let contour = match p.get("contour").map(|v| serde_json::from_value::<ContourType>(v.clone())) {
        Some(Ok(c @ (ContourType::BoundingBox | ContourType::GraphicFrame | ContourType::SameAsClipping))) => Some(c),
        Some(_) => return Err(bad("object.textWrap", "unsupported contour type")),
        None => None,
    };
    let mode: Option<WrapMode> = match p.get("mode") {
        Some(v) => Some(serde_json::from_value(v.clone()).map_err(|_| bad("object.textWrap", "unknown mode"))?),
        None if side.is_some() || contour.is_some() => None,
        None => Some(WrapMode::BoundingBox),
    };
    let off = match p.get("offset") {
        Some(Value::Number(n)) => Some([n.as_f64().unwrap_or(0.0); 4]),
        Some(v) => serde_json::from_value(v.clone()).ok(),
        None => None,
    };
    let invert = p.get("invert").and_then(Value::as_bool);
    let content = match p.get("content").and_then(Value::as_bool) {
        Some(c) => c,
        None => p.get("ids").is_none() && p.get("id").is_none() && s.doc()?.selection.content,
    };
    set_flag(
        s,
        p,
        move |i| {
            let w = match &mut i.content {
                Content::Graphic(g) if content => &mut g.wrap,
                _ => &mut i.wrap,
            };
            if let Some(m) = mode {
                w.mode = m;
            }
            if let Some(o) = off {
                w.offsets = o;
            }
            if let Some(v) = invert {
                w.invert = v;
            }
            if let Some(v) = side {
                w.side = v;
            }
            if let Some(c) = contour {
                w.contour = c;
            }
            // A custom boundary has the offsets built in and replaces the contour, so setting
            // either one rebuilds the wrap from the frame.
            if off.is_some() || contour.is_some() {
                w.path = None;
            }
        },
        false,
    )
}

fn corner_options(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_geom::corners::{Corner, CornerOptions, CornerShape};
    let opts = match p.get("corners") {
        Some(v) => {
            let corners: [Corner; 4] =
                serde_json::from_value(v.clone()).map_err(|e| bad("object.cornerOptions", format!("corners: four {{shape, size}} entries ({e})")))?;
            if corners.iter().any(|c| !c.size.is_finite() || c.size < 0.0) {
                return Err(bad("object.cornerOptions", "corner sizes must be finite and not negative"));
            }
            CornerOptions { corners }
        }
        None => {
            let shape = p.get("shape").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(CornerShape::Rounded);
            CornerOptions::uniform(shape, f64_or(p, "size", 12.0))
        }
    };
    set_flag(s, p, move |i| i.corners = opts, false)
}

/// `object.matchAttributes`: the selected objects (or `ids`) take `from`'s appearance.
pub(super) fn match_attributes(s: &mut Session, p: &Value) -> Result<Value> {
    let from = super::id_param(p, "from").ok_or_else(|| bad("object.matchAttributes", "missing from"))?;
    let src = s.doc()?.doc.item(from).cloned().ok_or(designcraft_doc::DocError::NoItem(from))?;
    set_flag(
        s,
        p,
        move |i| {
            if i.id == src.id {
                return;
            }
            i.fill = src.fill.clone();
            i.stroke = src.stroke.clone();
            i.corners = src.corners;
            i.opacity = src.opacity;
            i.blend = src.blend;
            i.effects = src.effects.clone();
        },
        false,
    )
}

/// `patch` merged into `base`: objects field by field (recursively), anything else replaced.
fn merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                merge_json(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// Keep effect values in their ranges: opacities 0–1, sizes and distances ≥ 0, percentages 0–100.
fn sanitize_effects(e: &mut designcraft_doc::Effects) -> Result<()> {
    const ID: &str = "object.effects";
    let len = |v: &mut f64| -> Result<()> {
        if !v.is_finite() {
            return Err(bad(ID, "values must be finite"));
        }
        *v = v.clamp(0.0, 10_000.0);
        Ok(())
    };
    let pct = |v: &mut f64| -> Result<()> {
        if !v.is_finite() {
            return Err(bad(ID, "values must be finite"));
        }
        *v = v.clamp(0.0, 100.0);
        Ok(())
    };
    let ang = |v: &mut f64| -> Result<()> {
        if !v.is_finite() {
            return Err(bad(ID, "values must be finite"));
        }
        *v = (*v + 180.0).rem_euclid(360.0) - 180.0;
        Ok(())
    };
    let op = |v: &mut f32| -> Result<()> {
        if !v.is_finite() {
            return Err(bad(ID, "values must be finite"));
        }
        *v = v.clamp(0.0, 1.0);
        Ok(())
    };
    let ds = &mut e.drop_shadow;
    op(&mut ds.opacity)?;
    ang(&mut ds.angle)?;
    len(&mut ds.distance)?;
    len(&mut ds.size)?;
    pct(&mut ds.spread)?;
    let is = &mut e.inner_shadow;
    op(&mut is.opacity)?;
    ang(&mut is.angle)?;
    len(&mut is.distance)?;
    len(&mut is.size)?;
    pct(&mut is.choke)?;
    let og = &mut e.outer_glow;
    op(&mut og.opacity)?;
    len(&mut og.size)?;
    pct(&mut og.spread)?;
    let ig = &mut e.inner_glow;
    op(&mut ig.opacity)?;
    len(&mut ig.size)?;
    pct(&mut ig.choke)?;
    let bv = &mut e.bevel;
    len(&mut bv.size)?;
    len(&mut bv.depth)?;
    ang(&mut bv.angle)?;
    op(&mut bv.highlight_opacity)?;
    op(&mut bv.shadow_opacity)?;
    let sa = &mut e.satin;
    op(&mut sa.opacity)?;
    ang(&mut sa.angle)?;
    len(&mut sa.distance)?;
    len(&mut sa.size)?;
    len(&mut e.feather)?;
    pct(&mut e.feather_options.choke)?;
    for w in &mut e.directional_feather.widths {
        len(w)?;
    }
    let gf = &mut e.gradient_feather;
    ang(&mut gf.angle)?;
    op(&mut gf.start)?;
    op(&mut gf.end)?;
    Ok(())
}

fn effects_all(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.effects";
    let patch = match p.get("effects") {
        None | Some(Value::Null) => None,
        Some(v @ Value::Object(_)) => Some(v.clone()),
        Some(_) => return Err(bad(ID, "effects must be an object")),
    };
    let opacity = match p.get("opacity") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().filter(|o| o.is_finite()).ok_or_else(|| bad(ID, "opacity must be a number"))?.clamp(0.0, 1.0) as f32),
    };
    let blend = match p.get("blend") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let raw = v.as_str().ok_or_else(|| bad(ID, "blend must be a string"))?;
            Some(BlendMode::parse(raw).ok_or_else(|| bad(ID, format!("unknown blend mode `{raw}`")))?)
        }
    };
    let ids = targets(s, p)?;
    s.edit(|d, _| {
        let mut changed = 0;
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            if it.locked {
                continue;
            }
            if let Some(patch) = &patch {
                let mut cur = serde_json::to_value(&it.effects).map_err(|e| bad(ID, e.to_string()))?;
                merge_json(&mut cur, patch);
                let mut fx: designcraft_doc::Effects = serde_json::from_value(cur).map_err(|e| bad(ID, format!("effects: {e}")))?;
                sanitize_effects(&mut fx)?;
                it.effects = fx;
            }
            if let Some(o) = opacity {
                it.opacity = o;
            }
            if let Some(b) = blend {
                it.blend = b;
            }
            changed += 1;
        }
        Ok(json!({"changed": changed}))
    })
}

fn set_flag(s: &mut Session, p: &Value, f: impl Fn(&mut Item), deselect: bool) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, sel| {
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                f(it);
            }
        }
        if deselect {
            *sel = Selection::default();
        }
        Ok(json!({"changed": ids.len()}))
    })
}

/// An object a colour command may change: neither it nor its layer is locked.
fn unlocked(d: &Document, it: &Item) -> bool {
    !it.locked && d.layer(it.layer).is_none_or(|l| !l.locked)
}

/// Refuses a path edit on `id` when it, a group it sits in, or its layer is locked.
pub(super) fn path_editable(d: &Document, id: ItemId, cmd: &str) -> Result<()> {
    let loc = d.find(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
    let mut items = d.spread(loc.spread).map(|sp| &sp.items[..]).unwrap_or_default();
    for &i in &loc.path {
        let Some(it) = items.get(i) else { break };
        if !unlocked(d, it) {
            return Err(bad(cmd, "the path is locked"));
        }
        items = it.children();
    }
    Ok(())
}

fn has_editable_selection(s: &Session) -> std::result::Result<(), String> {
    has_selection(s)?;
    let st = s.active().ok_or_else(|| super::NOTHING_SELECTED.to_string())?;
    if st.selection.items.iter().any(|id| st.doc.item(*id).is_some_and(|it| unlocked(&st.doc, it))) {
        Ok(())
    } else {
        Err("the selected objects are locked".into())
    }
}

/// Runs `f` on each unlocked target in one edit (one undo step).
fn edit_unlocked(s: &mut Session, p: &Value, f: impl Fn(&mut Item)) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, _| {
        let mut changed = 0;
        for id in &ids {
            if d.item(*id).is_some_and(|it| unlocked(d, it))
                && let Some(it) = d.item_mut(*id)
            {
                f(it);
                changed += 1;
            }
        }
        Ok(json!({"changed": changed}))
    })
}

/// A stroke that gets a colour needs a weight to show it.
fn visible_weight(st: &mut Stroke) {
    if st.swatch != designcraft_color::swatch::NONE && st.weight <= 0.0 {
        st.weight = Stroke::default().weight;
    }
}

fn swap_fill_stroke(it: &mut Item) {
    let fill = std::mem::take(&mut it.fill);
    it.fill = Fill { swatch: std::mem::take(&mut it.stroke.swatch), tint: it.stroke.tint, ..fill.clone() };
    it.stroke.swatch = fill.swatch;
    it.stroke.tint = fill.tint;
    visible_weight(&mut it.stroke);
}

fn default_fill_stroke(it: &mut Item) {
    it.fill = Fill { swatch: designcraft_color::swatch::NONE.into(), tint: 1.0, ..it.fill.clone() };
    it.stroke.swatch = designcraft_color::swatch::BLACK.into();
    it.stroke.tint = 1.0;
    visible_weight(&mut it.stroke);
}

/// Edits the targets' effects. A value that isn't a finite number is refused; the others are
/// clamped to their ranges.
fn set_effects(s: &mut Session, p: &Value, cmd: &'static str, f: impl Fn(&mut Effects)) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, _| {
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                let mut fx = it.effects.clone();
                f(&mut fx);
                it.effects = fx.sanitized().map_err(|m| bad(cmd, m))?;
            }
        }
        Ok(json!({"changed": ids.len()}))
    })
}

fn all_flag(s: &mut Session, f: impl Fn(&mut Item)) -> Result<Value> {
    s.edit(|d, _| {
        for i in 0..d.spreads.len() {
            let sp = Arc::make_mut(&mut d.spreads[i]);
            for it in &mut sp.items {
                f(Arc::make_mut(it));
            }
        }
        ok()
    })
}

fn content_type(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let t = str_param(p, "type").unwrap_or("unassigned").to_string();
    s.edit(|d, _| {
        for id in &ids {
            let cur = d.item(*id).map(|i| i.content.clone());
            match (t.as_str(), cur) {
                ("text", Some(Content::Unassigned | Content::Graphic(_))) => {
                    let sid = StoryId(d.alloc());
                    let mut st = Story::new(sid);
                    st.direction = d.new_story_direction();
                    st.frames.push(*id);
                    d.stories.insert(sid, Arc::new(st));
                    if let Some(it) = d.item_mut(*id) {
                        it.content = Content::Text(TextFrame { story: sid, options: TextFrameOptions::default() });
                    }
                }
                ("graphic" | "unassigned", Some(Content::Text(tf))) => {
                    let empty = d.story(tf.story).map(|s| s.frames.len() == 1).unwrap_or(true);
                    if let Some(st) = d.story_mut(tf.story) {
                        st.frames.retain(|f| f != id);
                    }
                    if empty {
                        d.stories.remove(&tf.story);
                    }
                    if let Some(it) = d.item_mut(*id) {
                        it.content = Content::Unassigned;
                    }
                }
                ("graphic" | "unassigned", Some(Content::Graphic(_))) if t == "unassigned" => {
                    if let Some(it) = d.item_mut(*id) {
                        it.content = Content::Unassigned;
                    }
                }
                _ => {}
            }
        }
        ok()
    })
}

fn text_frame_options(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.textFrameOptions";
    let st = s.doc()?;
    let mut ids = targets(s, p)?;
    if ids.is_empty()
        && let Some(t) = st.selection.text
    {
        ids = st.doc.story(t.story).map(|s| s.frames.clone()).unwrap_or_default();
    }
    let ranged = |key: &str, lo: f64, hi: f64, what: &str| match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64() {
            Some(x) if x.is_finite() && (lo..=hi).contains(&x) => Ok(Some(x)),
            _ => Err(bad(ID, format!("{key} must be {what} from {lo} to {hi}, not {v}"))),
        },
    };
    let rule_weight = ranged("columnRuleWeight", 0.0, 1000.0, "a number of points")?;
    let rule_tint = ranged("columnRuleTint", 0.0, 1.0, "a tint")?;
    let rule_offset = ranged("columnRuleOffset", -1440.0, 1440.0, "a number of points")?;
    let rule_top = ranged("columnRuleTopInset", -1440.0, 1440.0, "a number of points")?;
    let rule_bottom = ranged("columnRuleBottomInset", -1440.0, 1440.0, "a number of points")?;
    let min_height = ranged("minimumHeight", 0.0, crate::autosize::MAX_SIDE, "a number of points")?;
    let min_width = ranged("minimumWidth", 0.0, crate::autosize::MAX_SIDE, "a number of points")?;
    let column_width = ranged("columnWidth", 0.0, 15552.0, "a number of points")?;
    let first_baseline_min = ranged("firstBaselineMin", 0.0, 15552.0, "a number of points")?;
    let reference = match p.get("autoSizeReference") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .and_then(|n| crate::autosize::REFERENCES.iter().position(|r| *r == n))
                .ok_or_else(|| bad(ID, format!("autoSizeReference must be one of {}, not {v}", crate::autosize::REFERENCES.join("|"))))?
                as u8,
        ),
    };
    let rule_color = match p.get("columnRuleColor") {
        None | Some(Value::Null) => None,
        Some(Value::String(c)) if st.doc.swatch(c).is_some() => Some(c.clone()),
        Some(v) => return Err(bad(ID, format!("no swatch {v}"))),
    };
    let p = p.clone();
    s.edit(|d, _| {
        for id in &ids {
            let Some(tf) = d.item_mut(*id).and_then(Item::text_frame_mut) else { continue };
            let o = &mut tf.options;
            if let Some(v) = p.get("columns").and_then(Value::as_u64) {
                o.columns = (v as u32).clamp(1, 40);
            }
            if let Some(v) = p.get("gutter").and_then(Value::as_f64) {
                o.gutter = v.max(0.0);
            }
            match p.get("inset") {
                Some(Value::Number(n)) => o.inset = [n.as_f64().unwrap_or(0.0); 4],
                Some(v @ Value::Array(_)) => {
                    if let Ok(a) = serde_json::from_value::<[Option<f64>; 4]>(v.clone()) {
                        for (side, v) in o.inset.iter_mut().zip(a) {
                            if let Some(v) = v {
                                *side = v;
                            }
                        }
                    }
                }
                _ => {}
            }
            for (k, which) in [("verticalJustification", 0), ("firstBaseline", 1), ("autoSize", 2), ("columnsKind", 3)] {
                if let Some(v) = p.get(k).cloned() {
                    let _ = match which {
                        0 => serde_json::from_value(v).map(|x| o.vertical_justification = x),
                        1 => serde_json::from_value(v).map(|x| o.first_baseline = x),
                        2 => serde_json::from_value(v).map(|x| o.auto_size = x),
                        _ => serde_json::from_value(v).map(|x| o.columns_kind = x),
                    };
                }
            }
            if let Some(v) = reference {
                o.auto_size_ref = v;
            }
            if let Some(v) = p.get("useMinimumHeight").and_then(Value::as_bool) {
                o.auto_size_use_min_height = v;
            }
            if let Some(v) = min_height {
                o.auto_size_min_height = v;
            }
            if let Some(v) = p.get("useMinimumWidth").and_then(Value::as_bool) {
                o.auto_size_use_min_width = v;
            }
            if let Some(v) = min_width {
                o.auto_size_min_width = v;
            }
            if let Some(v) = p.get("noLineBreaks").and_then(Value::as_bool) {
                o.auto_size_no_line_breaks = v;
            }
            if let Some(v) = p.get("ignoreWrap").and_then(Value::as_bool) {
                o.ignore_wrap = v;
            }
            if let Some(v) = column_width {
                o.column_width = v;
            }
            if let Some(v) = first_baseline_min {
                o.first_baseline_min = v;
            }
            if let Some(v) = p.get("balanceColumns").and_then(Value::as_bool) {
                o.balance_columns = v;
            }
            if let Some(v) = p.get("columnRule").and_then(Value::as_bool) {
                o.column_rule = v;
            }
            if let Some(w) = rule_weight {
                o.column_rule_weight = w;
            }
            if let Some(c) = &rule_color {
                o.column_rule_color = c.clone();
            }
            if let Some(t) = rule_tint {
                o.column_rule_tint = t as f32;
            }
            if let Some(v) = rule_offset {
                o.column_rule_offset = v;
            }
            if let Some(v) = rule_top {
                o.column_rule_top_inset = v;
            }
            if let Some(v) = rule_bottom {
                o.column_rule_bottom_inset = v;
            }
            // Fixed Width columns set the frame's width: n columns, the gutters between them and
            // the left and right insets.
            let fixed_width = o.columns_kind == designcraft_doc::ColumnsKind::FixedWidth && o.column_width > 0.0;
            let width = f64::from(o.columns.max(1)) * o.column_width + f64::from(o.columns.max(1) - 1) * o.gutter + o.inset[1] + o.inset[3];
            if fixed_width
                && ["columnsKind", "columnWidth", "columns", "gutter", "inset"].iter().any(|k| p.get(*k).is_some())
                && let Some(r) = d.item(*id).map(Item::inner_bounds)
            {
                let r = super::finite_rect("object.textFrameOptions", Rect::new(r.x0, r.y0, r.x0 + width, r.y1))?;
                crate::autosize::resize(d, *id, r);
            }
        }
        // Story direction belongs to the story: every frame of the thread turns.
        if let Some(v) = p.get("vertical").and_then(Value::as_bool) {
            let stories: Vec<StoryId> = ids.iter().filter_map(|id| d.item(*id).and_then(Item::text_frame).map(|t| t.story)).collect();
            super::text::set_story_direction(d, &stories, v);
        }
        Ok(json!({"changed": ids.len()}))
    })
}

/// The gap at `p` on spread `sr`: (vertical gap?, edge a, edge b, ids ending at a, ids starting at b).
pub(crate) fn find_gap(d: &Document, sr: designcraft_doc::SpreadRef, p: Point) -> Option<(bool, f64, f64, Vec<ItemId>, Vec<ItemId>)> {
    let sp = d.spread(sr)?;
    let page = sp.page_at_x(p.x).and_then(|i| sp.pages.get(i)).map(|pg| pg.bounds())?;
    let items: Vec<(ItemId, Rect)> = sp
        .items
        .iter()
        .filter(|it| !it.hidden && !it.locked && d.layer(it.layer).is_none_or(|l| !l.locked && l.visible))
        .map(|it| (it.id, it.bounds()))
        .collect();
    if items.iter().any(|(_, b)| b.contains(p)) {
        return None;
    }
    // Between left and right neighbours (a vertical gap), or above and below (horizontal).
    let side = |vertical: bool| {
        let (pos, lo, hi) = if vertical { (p.x, page.x0, page.x1) } else { (p.y, page.y0, page.y1) };
        let across = |b: &Rect| if vertical { b.y0 <= p.y && p.y <= b.y1 } else { b.x0 <= p.x && p.x <= b.x1 };
        let end = |b: &Rect| if vertical { b.x1 } else { b.y1 };
        let start = |b: &Rect| if vertical { b.x0 } else { b.y0 };
        let a = items.iter().filter(|(_, b)| across(b) && end(b) <= pos).map(|(_, b)| end(b)).fold(lo, f64::max);
        let z = items.iter().filter(|(_, b)| across(b) && start(b) >= pos).map(|(_, b)| start(b)).fold(hi, f64::min);
        let before: Vec<ItemId> = items.iter().filter(|(_, b)| across(b) && (end(b) - a).abs() < 0.5).map(|(i, _)| *i).collect();
        let after: Vec<ItemId> = items.iter().filter(|(_, b)| across(b) && (start(b) - z).abs() < 0.5).map(|(i, _)| *i).collect();
        (!before.is_empty() || !after.is_empty()).then_some((vertical, a, z, before, after))
    };
    match (side(true), side(false)) {
        (Some(v), Some(h)) => Some(if v.2 - v.1 <= h.2 - h.1 { v } else { h }),
        (v, h) => v.or(h),
    }
}

fn gap_move(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "gap.move";
    let at = p
        .get("at")
        .and_then(Value::as_array)
        .and_then(|a| Some(Point::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?)))
        .ok_or_else(|| bad(ID, "missing at"))?;
    let sr = spread_param(p, ID)?;
    let (vertical, a, z, before, after) = find_gap(&s.doc()?.doc, sr, at).ok_or_else(|| bad(ID, "no gap there"))?;
    // `delta` along the gap's axis, or a drag (dx, dy) of which the axis part counts.
    let delta =
        p.get("delta").and_then(Value::as_f64).unwrap_or_else(|| p.get(if vertical { "dx" } else { "dy" }).and_then(Value::as_f64).unwrap_or(0.0));
    let _ = (a, z);
    s.edit(|d, _| {
        let mut n = 0;
        for (ids, is_before) in [(&before, true), (&after, false)] {
            for id in ids {
                let Some(it) = d.item_mut(*id) else { continue };
                let b = it.bounds();
                let mut to = b;
                match (vertical, is_before) {
                    (true, true) => to.x1 = (b.x1 + delta).max(b.x0 + 1.0),
                    (true, false) => to.x0 = (b.x0 + delta).min(b.x1 - 1.0),
                    (false, true) => to.y1 = (b.y1 + delta).max(b.y0 + 1.0),
                    (false, false) => to.y0 = (b.y0 + delta).min(b.y1 - 1.0),
                }
                let m = Affine::translate((to.x0, to.y0))
                    * Affine::scale_non_uniform(to.width() / b.width().max(1e-9), to.height() / b.height().max(1e-9))
                    * Affine::translate((-b.x0, -b.y0));
                if matches!(it.content, Content::Group { .. }) {
                    it.xf = m * it.xf;
                } else {
                    let inner = it.xf.inverse() * m * it.xf;
                    bake(it, inner);
                }
                n += 1;
            }
        }
        Ok(json!({"axis": if vertical { "x" } else { "y" }, "changed": n}))
    })
}

/// Clipping Path: trace the graphic (its alpha channel, or edges against a white background) and
/// give the frame that outline.
fn clipping_path(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.clippingPath";
    let ids = targets(s, p)?;
    let edges = match str_param(p, "type").unwrap_or("alpha") {
        "alpha" => false,
        "edges" => true,
        t => return Err(bad(ID, format!("unknown type `{t}` (alpha, edges)"))),
    };
    let threshold = p.get("threshold").and_then(Value::as_f64).unwrap_or(25.0).clamp(0.0, 255.0);
    let tol = p.get("tolerance").and_then(Value::as_f64).unwrap_or(2.0).max(0.1);
    // Trace each graphic (outside the edit: decoding is the slow part).
    let mut paths: Vec<(ItemId, designcraft_geom::PathData)> = Vec::new();
    {
        let d = &s.doc()?.doc;
        for id in &ids {
            let Some(it) = d.item(*id) else { continue };
            let Some(g) = it.graphic() else { continue };
            let Some(asset) = d.assets.get(&g.asset) else { continue };
            let Some(px) = designcraft_render::decode_pixmap_page(&asset.data, asset.page) else { continue };
            // A placed PDF cropped to a box: trace that box, which graphic space spans.
            let px = match designcraft_render::shown_box(asset) {
                Some(frac) => designcraft_render::crop_pixmap(&px, frac).unwrap_or(px),
                None => px,
            };
            let (pw, ph) = (px.width() as usize, px.height() as usize);
            // A grid of at most 400 cells on the long side.
            let step = (pw.max(ph) as f64 / 400.0).max(1.0);
            let (w, h) = (((pw as f64) / step).ceil() as usize, ((ph as f64) / step).ceil() as usize);
            let mask: Vec<bool> = (0..w * h)
                .map(|i| {
                    let (x, y) = (((i % w) as f64 * step) as u16, ((i / w) as f64 * step) as u16);
                    let c = px.sample(x.min(pw as u16 - 1), y.min(ph as u16 - 1));
                    if edges {
                        // Un-premultiply over white: anything darker than the threshold is inside.
                        let a = c.a as f64 / 255.0;
                        let lum = (0.299 * c.r as f64 + 0.587 * c.g as f64 + 0.114 * c.b as f64) + 255.0 * (1.0 - a);
                        lum < 255.0 - threshold
                    } else {
                        c.a as f64 > threshold
                    }
                })
                .collect();
            // The tolerance is in image pixels; the grid cells are `step` pixels.
            let rings = designcraft_geom::trace::trace(&mask, w, h, tol / step, 4.0);
            if rings.is_empty() {
                return Err(bad(ID, "nothing to trace (the image is empty at this threshold)"));
            }
            let (sx, sy) = (g.size.0 / w as f64, g.size.1 / h as f64);
            let subpaths = rings
                .into_iter()
                .map(|r| {
                    let pts: Vec<Point> = r.into_iter().map(|q| g.xf * Point::new(q.x * sx, q.y * sy)).collect();
                    designcraft_geom::path::SubPath::polyline(&pts, true)
                })
                .collect();
            paths.push((*id, designcraft_geom::PathData::new(subpaths)));
        }
    }
    if paths.is_empty() {
        return Err(bad(ID, "select a frame with a placed graphic"));
    }
    s.edit(|d, _| {
        let n = paths.len();
        for (id, path) in paths {
            if let Some(it) = d.item_mut(id) {
                it.path = path;
                it.shape = Shape::Path;
                it.corners = Default::default();
                if let Content::Graphic(g) = &mut it.content {
                    g.auto_fit = designcraft_doc::Fitting::None;
                }
            }
        }
        Ok(json!({"changed": n}))
    })
}

/// Clipping Path Options: report a graphic's clipping path, remove it, or change its options.
fn clipping_path_options(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.clippingPathOptions";
    let ids = targets(s, p)?;
    let remove = match str_param(p, "type") {
        None => false,
        Some("none") => true,
        Some(t) => return Err(bad(ID, format!("type `{t}` can't be set here (only `none`); object.clippingPath traces an outline"))),
    };
    let flag = |key: &str| p.get(key).map(|v| v.as_bool().ok_or_else(|| bad(ID, format!("{key} must be true or false")))).transpose();
    let (invert, inside) = (flag("invert")?, flag("includeInsideEdges")?);
    let graphics: Vec<ItemId> = {
        let d = &s.doc()?.doc;
        ids.into_iter().filter(|id| d.item(*id).and_then(Item::graphic).is_some()).collect()
    };
    if graphics.is_empty() {
        return Err(bad(ID, "select a frame with a placed graphic"));
    }
    if !remove && invert.is_none() && inside.is_none() {
        let d = &s.doc()?.doc;
        let items: Vec<Value> = graphics
            .iter()
            .filter_map(|id| d.item(*id).and_then(Item::graphic).map(|g| (id, g)))
            .map(|(id, g)| match &g.clip {
                None => json!({"id": id.0, "type": "none"}),
                Some(c) => json!({
                    "id": id.0,
                    "type": serde_json::to_value(c.kind).ok(),
                    "anchors": c.path.anchor_count(),
                    "invert": c.invert,
                    "includeInsideEdges": c.include_inside_edges,
                    "threshold": c.threshold,
                    "tolerance": c.tolerance,
                    "inset": c.inset,
                    "sourceName": c.source_name,
                }),
            })
            .collect();
        return Ok(json!({"items": items}));
    }
    s.edit(|d, _| {
        let mut n = 0;
        for id in &graphics {
            let Some(Content::Graphic(g)) = d.item_mut(*id).map(|it| &mut it.content) else { continue };
            if remove {
                n += usize::from(g.clip.take().is_some());
            } else if let Some(c) = &mut g.clip {
                let c = Arc::make_mut(c);
                c.invert = invert.unwrap_or(c.invert);
                c.include_inside_edges = inside.unwrap_or(c.include_inside_edges);
                n += 1;
            }
        }
        Ok(json!({"changed": n}))
    })
}

fn fitting_mode(s: &str) -> Option<designcraft_doc::Fitting> {
    use designcraft_doc::Fitting as F;
    Some(match s {
        "none" => F::None,
        "fillProportionally" => F::FillProportionally,
        "fitProportionally" => F::FitProportionally,
        "fitContentToFrame" => F::FitContentToFrame,
        "centerContent" => F::CenterContent,
        _ => return None,
    })
}

/// Frame Fitting Options: auto-fit, fitting, align from, crop amounts.
fn fitting_options(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.fittingOptions";
    let ids = targets(s, p)?;
    let fitting = match str_param(p, "fitting") {
        Some(f) => Some(fitting_mode(f).ok_or_else(|| bad(ID, format!("unknown fitting `{f}`")))?),
        None => None,
    };
    let auto = p.get("autoFit").and_then(Value::as_bool);
    let align = p.get("align").and_then(Value::as_u64).map(|v| v.min(8) as u8);
    let crop = match p.get("crop") {
        Some(Value::Array(a)) if a.len() == 4 => Some([0, 1, 2, 3].map(|i| a[i].as_f64().unwrap_or(0.0))),
        Some(Value::Number(n)) => Some([n.as_f64().unwrap_or(0.0); 4]),
        Some(v) => return Err(bad(ID, format!("bad `crop`: {v} (n or [t, l, b, r])"))),
        None => None,
    };
    s.edit(|d, _| {
        let mut n = 0;
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            let r = it.inner_bounds();
            let Content::Graphic(g) = &mut it.content else { continue };
            if let Some(a) = align {
                g.fit_align = a;
            }
            if let Some(c) = crop {
                g.crop = c;
            }
            let mode = fitting.unwrap_or(g.auto_fit);
            if let Some(a) = auto {
                g.auto_fit = if a {
                    if mode == designcraft_doc::Fitting::None { designcraft_doc::Fitting::FillProportionally } else { mode }
                } else {
                    designcraft_doc::Fitting::None
                };
            } else if fitting.is_some() && g.auto_fit != designcraft_doc::Fitting::None {
                g.auto_fit = mode;
            }
            if let Some(xf) = g.fitted(r, mode) {
                g.xf = xf;
            }
            n += 1;
        }
        Ok(json!({"changed": n}))
    })
}

fn fit(s: &mut Session, p: &Value) -> Result<Value> {
    let doc = &s.doc()?.doc;
    let ids: Vec<_> =
        targets(s, p)?.into_iter().filter(|id| doc.item(*id).is_some_and(|it| !it.locked && doc.layer(it.layer).is_none_or(|l| !l.locked))).collect();
    let mode = str_param(p, "mode").unwrap_or("fitProportionally").to_string();
    let (height, width) = match str_param(p, "axis").unwrap_or("both") {
        "height" => (true, false),
        "width" => (false, true),
        "both" => (true, true),
        other => return Err(bad("object.fit", format!("unknown axis `{other}`"))),
    };
    // Text frames: Fit Frame to Content fits the frame to its composed text.
    let mut text_fits = Vec::new();
    if mode == "fitFrameToContent" {
        let d = s.doc()?.doc.clone();
        for id in &ids {
            if let Some(r) = fitted_text_frame(&d, *id, height, width)? {
                text_fits.push((*id, r));
            }
        }
    }
    s.edit(|d, _| {
        for (id, r) in &text_fits {
            if let Some(it) = d.item_mut(*id) {
                it.path = shapes::rectangle(*r);
            }
        }
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            let r = it.inner_bounds();
            let Content::Graphic(g) = &mut it.content else { continue };
            let (nw, nh) = g.size;
            let cur = g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, nw, nh));
            if mode == "fitFrameToContent" {
                it.path = shapes::rectangle(Rect::new(
                    if width { cur.x0 } else { r.x0 },
                    if height { cur.y0 } else { r.y0 },
                    if width { cur.x1 } else { r.x1 },
                    if height { cur.y1 } else { r.y1 },
                ));
                continue;
            }
            let m = fitting_mode(&mode)
                .filter(|m| *m != designcraft_doc::Fitting::None)
                .ok_or_else(|| bad("object.fit", format!("unknown mode `{mode}`")))?;
            if let Some(xf) = g.fitted(r, m) {
                g.xf = xf;
            }
        }
        ok()
    })
}

/// The inner rectangle that fits text frame `id` to its text (Fit Frame to Content): the smallest
/// fitting height and the right side at the longest line's end (`width`, one column only),
/// including insets; the top-left stays. Overset text grows the frame first. None for
/// frames that aren't horizontal text frames, have no text, or are threaded on to another frame,
/// and for auto-size frames, which already fit their text around their reference point.
fn fitted_text_frame(d: &Document, id: ItemId, height: bool, width: bool) -> Result<Option<Rect>> {
    let Some(it) = d.item(id) else { return Ok(None) };
    let Some(tf) = it.text_frame() else { return Ok(None) };
    if tf.options.auto_size != designcraft_doc::AutoSize::Off {
        return Ok(None);
    }
    if d.frame_vertical(it) || tf.options.path.is_some() {
        return Ok(None);
    }
    let Some(story) = d.story(tf.story) else { return Ok(None) };
    // The last frame of its thread only: an earlier frame's text runs on.
    if story.frames.last() != Some(&id) {
        return Ok(None);
    }
    let r = it.inner_bounds();
    if !(r.width().is_finite() && r.height().is_finite() && r.width() > 0.0 && r.height() > 0.0) {
        return Err(bad("object.fit", "text frame bounds must be finite and nonempty"));
    }
    // Keep trial layouts out of the session cache. Search with the actual columns and frame
    // options: measuring a very tall frame collapses columns and includes vertical alignment's
    // extra space. A fitting trial must compose all the text, regardless of its justification.
    let cache = designcraft_compose::Cache::new();
    let mut trial = d.clone();
    let mut compose = |rect| {
        if let Some(it) = trial.item_mut(id) {
            it.path = shapes::rectangle(rect);
        }
        cache.get(&trial, tf.story, None)
    };
    let initial = compose(r);
    if initial.frame(id).is_none_or(|ft| ft.lines.is_empty()) && !initial.is_overset() {
        return Ok(None);
    }
    let mut out = r;
    // First fit the height at the existing width, then measure the complete text's width.
    // Recheck height after fitting width, since a small width change can change a line break.
    if height {
        out = fit_text_height(out, id, tf.options.inset[2], &mut compose)?;
    }
    if width && tf.options.columns <= 1 {
        let cs = compose(out);
        if let Some(ft) = cs.frame(id) {
            let right = ft.lines.iter().map(|l| l.end_x).fold(f64::NEG_INFINITY, f64::max);
            if right.is_finite() {
                out.x1 = (right + tf.options.inset[3] + 0.001).max(r.x0 + 1.0);
            }
        }
        if height {
            out = fit_text_height(out, id, tf.options.inset[2], &mut compose)?;
        }
    }
    Ok(Some(out))
}

fn fit_text_height(
    mut rect: Rect,
    id: ItemId,
    inset_bottom: f64,
    compose: &mut impl FnMut(Rect) -> Arc<designcraft_compose::ComposedStory>,
) -> Result<Rect> {
    let mut low = 1.0;
    let limit = 20_000.0;
    let mut high = rect.height().clamp(1.0, limit);
    rect.y1 = rect.y0 + high;
    while compose(rect).is_overset() {
        if high >= limit {
            return Err(bad("object.fit", "text does not fit within the 20,000 pt fitting limit"));
        }
        low = high;
        high = (high * 2.0).min(limit);
        rect.y1 = rect.y0 + high;
    }
    // Bounded search, always retaining a verified fitting upper bound.
    for _ in 0..32 {
        if high - low < 0.001 {
            break;
        }
        let mid = low + (high - low) / 2.0;
        rect.y1 = rect.y0 + mid;
        if compose(rect).is_overset() {
            low = mid;
        } else {
            high = mid;
        }
    }
    rect.y1 = rect.y0 + high;
    // A line is set when its baseline is within 0.01 pt of the bottom, so the search stops just
    // short of the lowest baseline. The bottom goes onto it (plus the inset): the frame then fits
    // its text exactly and still does when it is scaled up.
    let lowest = compose(rect).frame(id).and_then(|ft| ft.lines.iter().map(|l| l.baseline).reduce(f64::max));
    if let Some(b) = lowest.map(|b| b + inset_bottom).filter(|b| b.is_finite() && *b > rect.y1 && *b - rect.y1 <= 0.02) {
        rect.y1 = b;
    }
    Ok(rect)
}

pub(crate) trait RemoveKeep {
    fn remove_item_keep_story(&mut self, id: ItemId) -> Result<Item>;
}

/// Detach an item without touching story threads (moving between spreads, grouping).
impl RemoveKeep for Document {
    fn remove_item_keep_story(&mut self, id: ItemId) -> Result<Item> {
        let loc = self.find(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        if loc.path.len() != 1 {
            return Err(bad("object", "nested items can't be detached"));
        }
        let sp = self.spread_mut(loc.spread).ok_or(designcraft_doc::DocError::NoItem(id))?;
        Ok(Arc::unwrap_or_clone(sp.items.remove(loc.path[0])))
    }
}

/// Bounds of each target item (spread space) and the alignment reference rect.
fn align_reference(d: &Document, ids: &[ItemId], to: &str, key: Option<ItemId>) -> Option<Rect> {
    let bounds: Vec<Rect> = ids.iter().filter_map(|i| d.item(*i).map(Item::bounds)).collect();
    let union = bounds.iter().copied().reduce(|a, b| a.union(b))?;
    let first = ids.first().and_then(|i| d.find(*i))?;
    let sp = d.spread(first.spread)?;
    let page = sp.page_at_x(union.center().x).and_then(|pi| sp.pages.get(pi));
    Some(match to {
        "keyObject" => key.and_then(|k| d.item(k)).map(Item::bounds).unwrap_or(union),
        "margins" => page.map(|p| p.margin_rect()).unwrap_or(union),
        "page" => page.map(|p| p.bounds()).unwrap_or(union),
        "spread" => sp.bounds(),
        _ => union,
    })
}

fn align(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let edge = str_param(p, "edge").unwrap_or("left").to_string();
    let to = str_param(p, "to").map(str::to_string).unwrap_or_else(|| if ids.len() > 1 { "selection".into() } else { "page".into() });
    let key = s.doc()?.selection.key;
    s.edit(|d, _| {
        let r = align_reference(d, &ids, &to, key).ok_or_else(|| bad("object.align", "nothing to align"))?;
        for id in &ids {
            if Some(*id) == key && to == "keyObject" {
                continue;
            }
            let Some(it) = d.item_mut(*id) else { continue };
            if it.locked {
                continue;
            }
            let b = it.bounds();
            let (dx, dy) = match edge.as_str() {
                "left" => (r.x0 - b.x0, 0.0),
                "hcenter" => (r.center().x - b.center().x, 0.0),
                "right" => (r.x1 - b.x1, 0.0),
                "top" => (0.0, r.y0 - b.y0),
                "vcenter" => (0.0, r.center().y - b.center().y),
                "bottom" => (0.0, r.y1 - b.y1),
                other => return Err(bad("object.align", format!("unknown edge `{other}`"))),
            };
            it.xf = Affine::translate((dx, dy)) * it.xf;
        }
        ok()
    })
}

fn distribute(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    if ids.len() < 3 && p.get("spacing").is_none() {
        return Err(bad("object.distribute", "select at least three objects"));
    }
    let horizontal = str_param(p, "axis") != Some("vertical");
    let by_spacing = str_param(p, "by") == Some("spacing") || p.get("spacing").is_some();
    let fixed = p.get("spacing").and_then(Value::as_f64);
    s.edit(|d, _| {
        let mut items: Vec<(ItemId, Rect)> = ids.iter().filter_map(|i| d.item(*i).map(|it| (*i, it.bounds()))).collect();
        let key = |r: &Rect| if horizontal { r.x0 } else { r.y0 };
        items.sort_by(|a, b| key(&a.1).total_cmp(&key(&b.1)));
        let n = items.len();
        if n < 2 {
            return ok();
        }
        let (first, last) = (items[0].1, items[n - 1].1);
        let lo = |r: &Rect| if horizontal { r.x0 } else { r.y0 };
        let hi = |r: &Rect| if horizontal { r.x1 } else { r.y1 };
        let size = |r: &Rect| hi(r) - lo(r);
        let mut targets: Vec<f64> = Vec::with_capacity(n);
        if by_spacing {
            let total: f64 = items.iter().map(|(_, r)| size(r)).sum();
            let gap = fixed.unwrap_or_else(|| (hi(&last) - lo(&first) - total) / (n - 1) as f64);
            let mut pos = lo(&first);
            for (_, r) in &items {
                targets.push(pos);
                pos += size(r) + gap;
            }
        } else {
            let c0 = (lo(&first) + hi(&first)) / 2.0;
            let c1 = (lo(&last) + hi(&last)) / 2.0;
            for (k, (_, r)) in items.iter().enumerate() {
                targets.push(c0 + (c1 - c0) * k as f64 / (n - 1) as f64 - size(r) / 2.0);
            }
        }
        for ((id, r), t) in items.iter().zip(targets) {
            let delta = t - lo(r);
            if let Some(it) = d.item_mut(*id) {
                // Keep locked items in the spacing calculation, as align does for its reference.
                if it.locked {
                    continue;
                }
                let v = if horizontal { Vec2::new(delta, 0.0) } else { Vec2::new(0.0, delta) };
                it.xf = Affine::translate(v) * it.xf;
            }
        }
        ok()
    })
}

fn anchor_from(v: &Value) -> Option<designcraft_geom::Anchor> {
    let pt = |k: &str| -> Option<Point> {
        let a = v.get(k)?.as_array()?;
        Some(Point::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?))
    };
    let p = pt("p")?;
    let h_in = pt("in").unwrap_or(p);
    let h_out = pt("out").unwrap_or(p);
    let smooth = (h_in - p).hypot() > 1e-9 || (h_out - p).hypot() > 1e-9;
    Some(designcraft_geom::Anchor {
        p,
        h_in,
        h_out,
        kind: if smooth { designcraft_geom::AnchorKind::Smooth } else { designcraft_geom::AnchorKind::Corner },
    })
}

fn path_create(s: &mut Session, p: &Value) -> Result<Value> {
    let anchors: Vec<designcraft_geom::Anchor> =
        p.get("anchors").and_then(Value::as_array).map(|a| a.iter().filter_map(anchor_from).collect()).unwrap_or_default();
    if anchors.len() < 2 {
        return Err(bad("path.create", "a path needs at least two anchors"));
    }
    let closed = bool_or(p, "closed", false);
    let sr = spread_param(p, "path.create")?;
    let lid = s.doc()?.active_layer;
    s.edit(|d, sel| {
        let id = ItemId(d.alloc());
        let shape = if anchors.len() == 2 && !closed { Shape::GraphicLine } else { Shape::Path };
        let mut it = Item::new(id, lid, shape, designcraft_geom::PathData::single(designcraft_geom::SubPath::new(anchors, closed)));
        it.stroke = Stroke::default();
        new_object_colors(d, &mut it);
        d.insert_item(sr, it, None)?;
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })
}

fn path_append(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::id_param(p, "id").ok_or_else(|| bad("path.appendAnchor", "missing id"))?;
    let a = p.get("anchor").and_then(anchor_from).ok_or_else(|| bad("path.appendAnchor", "missing anchor"))?;
    let subpath = p.get("subpath").and_then(Value::as_u64);
    let at_start = match p.get("at") {
        None => false,
        Some(v) if v.as_str() == Some("end") => false,
        Some(v) if v.as_str() == Some("start") => true,
        Some(_) => return Err(bad("path.appendAnchor", "`at` must be `start` or `end`")),
    };
    s.edit(|d, _| {
        path_editable(d, id, "path.appendAnchor")?;
        let loc = d.find(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let inv = (d.parent_xf(&loc) * d.item_at(&loc).map(|i| i.xf).unwrap_or_default()).inverse();
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let sp = match subpath {
            Some(i) => {
                usize::try_from(i).ok().and_then(|i| it.path.subpaths.get_mut(i)).ok_or_else(|| bad("path.appendAnchor", "no such subpath"))?
            }
            None => it.path.subpaths.last_mut().ok_or_else(|| bad("path.appendAnchor", "empty path"))?,
        };
        if sp.closed {
            return Err(bad("path.appendAnchor", "the subpath is closed"));
        }
        if at_start {
            sp.anchors.insert(0, a.transform(inv));
        } else {
            sp.anchors.push(a.transform(inv));
        }
        if it.shape == Shape::GraphicLine {
            it.shape = Shape::Path;
        }
        ok()
    })
}

fn path_set_anchor(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "path.setAnchor";
    let id = super::id_param(p, "id").ok_or_else(|| bad(ID, "missing id"))?;
    let pt = |k: &str| -> Option<Point> {
        let a = p.get(k)?.as_array()?;
        let q = Point::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?);
        (q.x.is_finite() && q.y.is_finite()).then_some(q)
    };
    let (new_p, new_in, new_out) = (pt("p"), pt("in"), pt("out"));
    if new_p.is_none() && new_in.is_none() && new_out.is_none() {
        return Err(bad(ID, "give p, in or out"));
    }
    let subpath = p.get("subpath").and_then(Value::as_u64);
    let which = p.get("anchor").cloned().unwrap_or(Value::Null);
    s.edit(|d, _| {
        path_editable(d, id, ID)?;
        let loc = d.find(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let inv = (d.parent_xf(&loc) * d.item_at(&loc).map(|i| i.xf).unwrap_or_default()).inverse();
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let sp = match subpath {
            Some(i) => usize::try_from(i).ok().and_then(|i| it.path.subpaths.get_mut(i)),
            None => it.path.subpaths.last_mut(),
        }
        .ok_or_else(|| bad(ID, "no such subpath"))?;
        let n = sp.anchors.len();
        let ai = match &which {
            Value::String(w) if w == "first" => 0,
            Value::String(w) if w == "last" => n.saturating_sub(1),
            v => v.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or_else(|| bad(ID, "anchor: an index, \"first\" or \"last\""))?,
        };
        let a = sp.anchors.get_mut(ai).ok_or_else(|| bad(ID, "no such anchor"))?;
        if let Some(q) = new_p {
            let v = inv * q - a.p;
            a.p += v;
            a.h_in += v;
            a.h_out += v;
        }
        if let Some(q) = new_in {
            a.h_in = inv * q;
        }
        if let Some(q) = new_out {
            a.h_out = inv * q;
        }
        if new_in.is_some() && new_out.is_some() {
            let (i, o) = (a.h_in - a.p, a.h_out - a.p);
            let collinear = (i.x * o.y - i.y * o.x).abs() <= 1e-6 * i.hypot().max(1.0) * o.hypot().max(1.0) && i.dot(o) <= 0.0;
            a.kind = if collinear { designcraft_geom::AnchorKind::Smooth } else { designcraft_geom::AnchorKind::Corner };
        }
        ok()
    })
}

fn path_reverse(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let only = p.get("subpath").and_then(Value::as_u64).map(|i| usize::try_from(i).unwrap_or(usize::MAX));
    if let (Some(i), Some(id)) = (only, ids.first())
        && s.doc()?.doc.item(*id).is_none_or(|it| it.path.subpaths.get(i).is_none())
    {
        return Err(bad("path.reverse", "no such subpath"));
    }
    let ids: Vec<ItemId> = if only.is_some() { ids.into_iter().take(1).collect() } else { ids };
    let doc = &s.doc()?.doc;
    let editable: Vec<ItemId> = ids.iter().copied().filter(|id| path_editable(doc, *id, "path.reverse").is_ok()).collect();
    if editable.is_empty()
        && let Some(id) = ids.first()
    {
        path_editable(doc, *id, "path.reverse")?;
    }
    s.edit(|d, _| {
        let mut reversed = 0;
        for id in &editable {
            let Some(it) = d.item_mut(*id) else { continue };
            if it.is_group() {
                continue;
            }
            for (i, sp) in it.path.subpaths.iter_mut().enumerate() {
                if only.is_some_and(|o| o != i) {
                    continue;
                }
                sp.anchors.reverse();
                for a in &mut sp.anchors {
                    std::mem::swap(&mut a.h_in, &mut a.h_out);
                }
            }
            reversed += 1;
        }
        Ok(json!({"reversed": reversed}))
    })
}

fn path_close(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::id_param(p, "id").ok_or_else(|| bad("path.close", "missing id"))?;
    let only = p.get("subpath").and_then(Value::as_u64).map(|i| usize::try_from(i).unwrap_or(usize::MAX));
    s.edit(|d, _| {
        path_editable(d, id, "path.close")?;
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        if only.is_some_and(|i| i >= it.path.subpaths.len()) {
            return Err(bad("path.close", "no such subpath"));
        }
        for (i, sp) in it.path.subpaths.iter_mut().enumerate() {
            if only.is_none_or(|o| o == i) {
                sp.closed = true;
            }
        }
        if it.shape == Shape::GraphicLine {
            it.shape = Shape::Path;
        }
        ok()
    })
}

fn path_move_anchors(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::id_param(p, "id").ok_or_else(|| bad("path.moveAnchors", "missing id"))?;
    let which: Vec<(usize, usize)> = p.get("anchors").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let d0 = Vec2::new(f64_or(p, "dx", 0.0), f64_or(p, "dy", 0.0));
    let handle = str_param(p, "handle").map(str::to_string);
    s.edit(|d, _| {
        path_editable(d, id, "path.moveAnchors")?;
        let loc = d.find(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let m = d.parent_xf(&loc) * d.item_at(&loc).map(|i| i.xf).unwrap_or_default();
        // Spread-space delta → inner-space delta (linear part only).
        let inv = m.inverse();
        let dv = (inv * Point::new(d0.x, d0.y)) - (inv * Point::ZERO);
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        for (si, ai) in &which {
            let Some(a) = it.path.anchor_mut(*si, *ai) else { continue };
            match handle.as_deref() {
                Some("in") => {
                    a.h_in += dv;
                    if a.kind == designcraft_geom::AnchorKind::Smooth {
                        a.h_out = a.p - (a.h_in - a.p);
                    }
                }
                Some("out") => {
                    a.h_out += dv;
                    if a.kind == designcraft_geom::AnchorKind::Smooth {
                        a.h_in = a.p - (a.h_out - a.p);
                    }
                }
                _ => a.translate(dv),
            }
        }
        if it.shape == Shape::Rectangle || it.shape == Shape::Oval || it.shape == Shape::Polygon {
            it.shape = Shape::Path;
        }
        ok()
    })
}

const MAX_STEP_REPEAT_COPIES: u64 = 1000;
const MAX_STEP_REPEAT_WORK: usize = 1000;
const MAX_STEP_REPEAT_OFFSET: f64 = 1_000_000_000.0;

fn step_repeat_count(p: &Value, key: &str, default: u64) -> Result<u64> {
    let Some(value) = p.get(key) else { return Ok(default) };
    let count = value.as_u64().ok_or_else(|| bad("edit.stepAndRepeat", format!("`{key}` must be an integer")))?;
    if !(1..=MAX_STEP_REPEAT_COPIES).contains(&count) {
        return Err(bad("edit.stepAndRepeat", format!("`{key}` must be between 1 and {MAX_STEP_REPEAT_COPIES}")));
    }
    Ok(count)
}

fn step_repeat_delta(p: &Value, key: &str, default: f64) -> Result<f64> {
    let Some(value) = p.get(key) else { return Ok(default) };
    let delta = value.as_f64().ok_or_else(|| bad("edit.stepAndRepeat", format!("`{key}` must be a finite number")))?;
    if !delta.is_finite() {
        return Err(bad("edit.stepAndRepeat", format!("`{key}` must be a finite number")));
    }
    Ok(delta)
}

fn checked_step_offset(dx: f64, dy: f64, x: u64, y: u64) -> Result<Vec2> {
    let (x, y) = (x as f64 * dx, y as f64 * dy);
    if !x.is_finite() || !y.is_finite() || x.abs() > MAX_STEP_REPEAT_OFFSET || y.abs() > MAX_STEP_REPEAT_OFFSET {
        return Err(bad("edit.stepAndRepeat", format!("generated offsets must stay between -{MAX_STEP_REPEAT_OFFSET} and {MAX_STEP_REPEAT_OFFSET}")));
    }
    Ok(Vec2::new(x, y))
}

fn step_repeat_offsets(p: &Value) -> Result<Vec<Vec2>> {
    let (dx, dy) = (step_repeat_delta(p, "dx", 12.0)?, step_repeat_delta(p, "dy", 12.0)?);
    match (p.get("rows"), p.get("columns")) {
        (None, None) => {
            let count = step_repeat_count(p, "count", 1)?;
            (1..=count).map(|k| checked_step_offset(dx, dy, k, k)).collect()
        }
        (Some(_), Some(_)) => {
            let rows = step_repeat_count(p, "rows", 1)?;
            let columns = step_repeat_count(p, "columns", 1)?;
            let cells = rows
                .checked_mul(columns)
                .filter(|count| *count <= MAX_STEP_REPEAT_COPIES)
                .ok_or_else(|| bad("edit.stepAndRepeat", format!("`rows` × `columns` must not exceed {MAX_STEP_REPEAT_COPIES}")))?;
            let capacity = usize::try_from(cells.saturating_sub(1))
                .map_err(|_| bad("edit.stepAndRepeat", "the requested grid is too large for this platform"))?;
            let mut offsets = Vec::with_capacity(capacity);
            for row in 0..rows {
                for column in 0..columns {
                    if row != 0 || column != 0 {
                        offsets.push(checked_step_offset(dx, dy, column, row)?);
                    }
                }
            }
            Ok(offsets)
        }
        _ => Err(bad("edit.stepAndRepeat", "`rows` and `columns` must be provided together")),
    }
}

/// Existing roots to duplicate, with repeated/ancestor-overlapping targets collapsed. Returns an
/// error before cloning the document when the copies (including group descendants) exceed the
/// command's work budget.
fn checked_step_repeat_targets(d: &Document, ids: &[ItemId], copies: usize, max_work: usize) -> Result<Vec<ItemId>> {
    let mut seen = HashSet::new();
    let mut located = Vec::new();
    for id in ids {
        if seen.insert(*id)
            && let Some(loc) = d.find(*id)
        {
            located.push((*id, loc));
        }
    }
    if located.is_empty() {
        return Err(bad("edit.stepAndRepeat", "nothing to repeat"));
    }

    // A selected ancestor already deep-copies its descendants. Keeping a selected child as a
    // second root would duplicate it twice and make the checked work disagree with the real work.
    let roots: Vec<ItemId> = located
        .iter()
        .filter(|(_, loc)| {
            !located.iter().any(|(_, parent)| parent.spread == loc.spread && parent.path.len() < loc.path.len() && loc.path.starts_with(&parent.path))
        })
        .map(|(id, _)| *id)
        .collect();

    let mut expanded = HashSet::new();
    for id in &roots {
        if let Some(item) = d.item(*id) {
            item.walk(&mut |child| {
                expanded.insert(child.id);
            });
        }
    }
    expanded
        .len()
        .checked_mul(copies)
        .filter(|work| *work <= max_work)
        .ok_or_else(|| bad("edit.stepAndRepeat", format!("a single command may create at most {max_work} items, including group contents")))?;
    Ok(roots)
}

fn step_and_repeat(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let offsets = step_repeat_offsets(p)?;
    let ids = checked_step_repeat_targets(&s.doc()?.doc, &ids, offsets.len(), MAX_STEP_REPEAT_WORK)?;
    s.edit(|d, sel| {
        let src = d.clone();
        let mut all = ids.clone();
        for off in offsets {
            for id in &ids {
                let sr = src.find(*id).map(|l| l.spread).unwrap_or(SpreadRef::Doc(0));
                all.extend(duplicate_from(d, &src, &[*id], sr, off)?);
            }
        }
        *sel = Selection::items(all.clone());
        let copies: Vec<u64> = all.iter().skip(ids.len()).map(|i| i.0).collect();
        Ok(json!({"created": copies.len(), "ids": copies}))
    })
}

#[cfg(test)]
mod arrange_tests {
    use super::*;

    fn fixture<const N: usize>(layers: [usize; N]) -> (Session, [ItemId; N]) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        let first = s.doc().unwrap().doc.default_layer().0;
        let second = s.execute("layer.new", &json!({"name": "Other layer"})).unwrap()["id"].as_u64().unwrap();
        let layer_ids = [first, second];
        let mut ids = [ItemId(0); N];
        for (id, layer) in ids.iter_mut().zip(layers) {
            // Overlap objects within each layer; separate layers spatially for hit testing.
            let x = if layer == 0 { 0 } else { 200 };
            *id = ItemId(s.execute("frame.create", &json!({"rect": [x, 0, x + 100, 100]})).unwrap()["id"].as_u64().unwrap());
            s.execute("object.setLayer", &json!({"ids": [id.0], "layer": layer_ids[layer]})).unwrap();
        }
        (s, ids)
    }

    fn stack(s: &Session) -> Vec<ItemId> {
        s.doc().unwrap().doc.spreads[0].items.iter().map(|it| it.id).collect()
    }

    fn snapshot(s: &Session) -> Value {
        let st = s.doc().unwrap();
        json!({"document": &*st.doc, "selection": &st.selection})
    }

    fn assert_items_unchanged(before: &Document, after: &Document) {
        assert_eq!(serde_json::to_value(&before.layers).unwrap(), serde_json::to_value(&after.layers).unwrap());
        for it in &before.spreads[0].items {
            assert_eq!(serde_json::to_value(it).unwrap(), serde_json::to_value(after.item(it.id).unwrap()).unwrap());
        }
    }

    #[test]
    fn arrange_steps_past_other_layers_change_hit_order_and_undo() {
        for direction in ["forward", "backward"] {
            let (mut s, [a, x, y, b]) = fixture([0, 1, 1, 0]);
            let selected = if direction == "forward" { a } else { b };
            s.execute("selection.set", &json!({"ids": [selected.0]})).unwrap();
            let before = snapshot(&s);
            let doc = s.doc().unwrap().doc.clone();
            let undo_len = s.doc().unwrap().history.undo.len();
            let hit = |s: &Session| s.doc().unwrap().doc.hit_item(0, Point::new(50.0, 50.0), 0.0);
            assert_eq!(hit(&s), Some(b));

            s.execute("object.arrange", &json!({"to": direction})).unwrap();
            assert_eq!(stack(&s), [b, x, y, a], "{direction}");
            assert_eq!(hit(&s), Some(a), "the visible topmost object must change");
            assert_items_unchanged(&doc, &s.doc().unwrap().doc);
            assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1);
            assert_eq!(s.doc().unwrap().selection.items, [selected]);
            let after = snapshot(&s);

            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(snapshot(&s), before);
            assert_eq!(hit(&s), Some(b));
            s.execute("edit.redo", &json!({})).unwrap();
            assert_eq!(snapshot(&s), after);
            assert_eq!(hit(&s), Some(a));
        }
    }

    #[test]
    fn arrange_multiple_selection_preserves_order_independent_of_input_order() {
        // Layer 0: a, b, c, d. Layer 1: x, y. Arrays name the original stack indices.
        let cases = [
            ("forward", [0, 2], [4, 1, 0, 3, 2, 5]),  // a,b cross c as a block
            ("backward", [2, 4], [2, 1, 4, 3, 0, 5]), // b,c cross a as a block
            ("forward", [0, 4], [2, 1, 0, 3, 5, 4]),  // separate selected blocks
            ("backward", [2, 5], [2, 1, 0, 3, 5, 4]),
            ("forward", [4, 5], [0, 1, 2, 3, 4, 5]),  // front block is already at its endpoint
            ("backward", [0, 2], [0, 1, 2, 3, 4, 5]), // back block is already at its endpoint
        ];
        for (direction, selected, expected) in cases {
            for reverse in [false, true] {
                let (mut s, ids) = fixture([0, 1, 0, 1, 0, 0]);
                let doc = s.doc().unwrap().doc.clone();
                let mut selected = selected.map(|i| ids[i].0).to_vec();
                if reverse {
                    selected.reverse();
                }
                // Explicit command targets may contain duplicates: an item still moves once.
                selected.push(selected[0]);
                s.execute("object.arrange", &json!({"to": direction, "ids": selected})).unwrap();
                assert_eq!(stack(&s), expected.map(|i| ids[i]), "{direction}, reverse={reverse}");
                assert_items_unchanged(&doc, &s.doc().unwrap().doc);
            }
        }
    }

    #[test]
    fn arrange_same_layer_steps_stop_at_layer_endpoints() {
        let cases = [
            ("forward", vec![1], [0, 2, 1, 3, 4]),
            ("backward", vec![3], [0, 1, 3, 2, 4]),
            ("forward", vec![3], [0, 1, 2, 3, 4]),
            ("backward", vec![1], [0, 1, 2, 3, 4]),
            ("forward", vec![3, 1, 2], [0, 1, 2, 3, 4]),
            ("backward", vec![3, 1, 2], [0, 1, 2, 3, 4]),
        ];
        for (direction, selected, expected) in cases {
            let (mut s, ids) = fixture([1, 0, 0, 0, 1]);
            let selected: Vec<_> = selected.into_iter().map(|i| ids[i].0).collect();
            s.execute("object.arrange", &json!({"to": direction, "ids": selected})).unwrap();
            assert_eq!(stack(&s), expected.map(|i| ids[i]));
        }
        for direction in ["forward", "backward"] {
            let (mut s, [x, a, y]) = fixture([1, 0, 1]);
            s.execute("object.arrange", &json!({"to": direction, "ids": [a.0]})).unwrap();
            assert_eq!(stack(&s), [x, a, y], "only object on its layer");
        }
    }

    #[test]
    fn arrange_selection_on_two_layers_moves_each_layer_independently() {
        for direction in ["forward", "backward"] {
            let (mut s, [a, x, b, y]) = fixture([0, 1, 0, 1]);
            let selected = if direction == "forward" { [x.0, a.0] } else { [b.0, y.0] };
            s.execute("object.arrange", &json!({"to": direction, "ids": selected})).unwrap();
            assert_eq!(stack(&s), [b, y, a, x]);
            let doc = &s.doc().unwrap().doc;
            assert_eq!(doc.hit_item(0, Point::new(50.0, 50.0), 0.0), Some(a));
            assert_eq!(doc.hit_item(0, Point::new(250.0, 50.0), 0.0), Some(x));
        }
    }

    #[test]
    fn arrange_menu_steps_use_same_layer_order() {
        for command in ["object.bringForward", "object.sendBackward"] {
            let (mut s, [a, x, b]) = fixture([0, 1, 0]);
            let selected = if command == "object.bringForward" { a } else { b };
            s.execute("selection.set", &json!({"ids": [selected.0]})).unwrap();
            s.execute(command, &json!({})).unwrap();
            assert_eq!(stack(&s), [b, x, a]);
            assert_eq!(s.doc().unwrap().doc.hit_item(0, Point::new(50.0, 50.0), 0.0), Some(a));
        }
    }
}

#[cfg(test)]
mod step_repeat_tests {
    use super::*;

    /// With nothing selected, fill and stroke set the colours new shapes are drawn with, as
    /// InDesign's document defaults; frames for graphics and text keep theirs.
    #[test]
    fn with_nothing_selected_fill_and_stroke_are_for_new_shapes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let colors = |s: &Session, id: u64| {
            let it = s.doc().unwrap().doc.item(ItemId(id)).unwrap();
            (it.fill.swatch.clone(), it.stroke.swatch.clone(), it.stroke.weight)
        };
        let rect = |s: &mut Session| {
            let id = s.execute("frame.create", &json!({"rect": [72, 72, 144, 144], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
            s.execute("selection.set", &json!({"ids": []})).unwrap();
            id
        };
        // New documents: no fill and a 1 pt black stroke.
        let a = rect(&mut s);
        assert_eq!(colors(&s, a), ("[None]".into(), "[Black]".into(), 1.0));
        // A picked colour and Apply None, with nothing selected.
        let r = s.execute("object.color", &json!({"color": "#7c1315"})).unwrap();
        let red = r["swatch"].as_str().unwrap().to_string();
        s.execute("object.stroke", &json!({"swatch": "[None]"})).unwrap();
        let b = rect(&mut s);
        assert_eq!(colors(&s, b), (red.clone(), "[None]".into(), 1.0), "the next shape has them");
        assert_eq!(colors(&s, a).0, "[None]", "shapes drawn before keep theirs");
        let line = s.execute("line.create", &json!({"a": [0, 0], "b": [100, 0]})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(colors(&s, line).1, "[None]");
        // Pen and Pencil paths too.
        let anchors = json!([{"p": [0, 200]}, {"p": [100, 250]}, {"p": [50, 300]}]);
        let path = s.execute("path.create", &json!({"anchors": anchors, "closed": true})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(colors(&s, path), (red.clone(), "[None]".into(), 1.0));
        s.execute("selection.set", &json!({"ids": []})).unwrap();
        // A graphics frame keeps its object style's.
        let g = s.execute("frame.create", &json!({"rect": [200, 72, 300, 144]})).unwrap()["id"].as_u64().unwrap();
        assert_ne!(colors(&s, g).0, red);
        s.execute("selection.set", &json!({"ids": []})).unwrap();
        // Swap and Default.
        s.execute("object.swapFillStroke", &json!({})).unwrap();
        let c = &s.doc().unwrap().doc.settings.new_object;
        assert_eq!((c.fill.as_str(), c.stroke.as_str()), ("[None]", red.as_str()));
        s.execute("object.fill", &json!({"swatch": "[Paper]"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.settings.new_object.fill, "[Paper]");
        s.execute("object.defaultFillStroke", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.settings.new_object, designcraft_doc::NewObjectColors::default());
        // One undo step each.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.settings.new_object.fill, "[Paper]");
        // With a selection, they still colour it and leave the defaults.
        s.execute("selection.set", &json!({"ids": [a]})).unwrap();
        s.execute("object.fill", &json!({"swatch": "[Black]"})).unwrap();
        assert_eq!(colors(&s, a).0, "[Black]");
        assert_eq!(s.doc().unwrap().doc.settings.new_object.fill, "[Paper]");
    }

    #[test]
    fn swap_and_default_fill_stroke_are_one_undo_step_per_object() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = |v: Value| ItemId(v["id"].as_u64().unwrap());
        let a = id(s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).unwrap());
        let b = id(s.execute("frame.create", &json!({"rect": [20, 0, 30, 10]})).unwrap());
        let locked = id(s.execute("frame.create", &json!({"rect": [40, 0, 50, 10]})).unwrap());
        s.execute("object.fill", &json!({"ids": [a.0], "swatch": "[Paper]", "tint": 0.5})).unwrap();
        s.execute("object.stroke", &json!({"ids": [a.0], "swatch": "[Black]", "weight": 2})).unwrap();
        s.execute("object.fill", &json!({"ids": [b.0, locked.0], "swatch": "[Black]"})).unwrap();
        s.execute("object.stroke", &json!({"ids": [b.0, locked.0], "swatch": "[None]"})).unwrap();
        s.execute("object.lock", &json!({"ids": [locked.0]})).unwrap();
        s.execute("selection.set", &json!({"ids": [a.0, b.0, locked.0]})).unwrap();
        let colors = |s: &Session, i: ItemId| {
            let it = s.doc().unwrap().doc.item(i).unwrap().clone();
            (it.fill.swatch, it.fill.tint, it.stroke.swatch, it.stroke.tint)
        };
        let before = [colors(&s, a), colors(&s, b), colors(&s, locked)];

        assert_eq!(s.execute("object.swapFillStroke", &json!({})).unwrap()["changed"], 2);
        assert_eq!(colors(&s, a), ("[Black]".into(), 1.0, "[Paper]".into(), 0.5));
        assert_eq!(colors(&s, b), ("[None]".into(), 1.0, "[Black]".into(), 1.0));
        assert_eq!(colors(&s, locked), before[2], "a locked object keeps its colours");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!([colors(&s, a), colors(&s, b), colors(&s, locked)], before, "one undo restores both");

        s.execute("object.defaultFillStroke", &json!({})).unwrap();
        for i in [a, b] {
            assert_eq!(colors(&s, i), ("[None]".into(), 1.0, "[Black]".into(), 1.0));
        }
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!([colors(&s, a), colors(&s, b), colors(&s, locked)], before);

        s.execute("selection.set", &json!({"ids": [locked.0]})).unwrap();
        assert!(s.execute("object.swapFillStroke", &json!({})).is_err(), "nothing unlocked to change");
    }

    #[test]
    fn step_repeat_validates_counts_and_offsets() {
        assert_eq!(step_repeat_offsets(&json!({"count": MAX_STEP_REPEAT_COPIES})).unwrap().len(), MAX_STEP_REPEAT_COPIES as usize);
        assert_eq!(step_repeat_offsets(&json!({"rows": 25, "columns": 40})).unwrap().len(), 999);
        assert!(step_repeat_offsets(&json!({"count": MAX_STEP_REPEAT_COPIES + 1})).is_err());
        assert!(step_repeat_offsets(&json!({"rows": 500, "columns": 3})).is_err());
        assert!(step_repeat_offsets(&json!({"rows": 3})).is_err());
        assert!(step_repeat_offsets(&json!({"count": 2, "dx": f64::MAX})).is_err());
    }

    #[test]
    fn step_repeat_counts_unique_items_and_group_contents() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = ItemId(s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap());
        let b = ItemId(s.execute("frame.create", &json!({"rect": [20, 0, 30, 10]})).unwrap()["id"].as_u64().unwrap());
        let c = ItemId(s.execute("frame.create", &json!({"rect": [40, 0, 50, 10]})).unwrap()["id"].as_u64().unwrap());
        let d = &s.doc().unwrap().doc;
        assert_eq!(checked_step_repeat_targets(d, &[a, a], 2, 2).unwrap(), vec![a]);
        assert!(checked_step_repeat_targets(d, &[a, b, c], 1, 2).is_err());
        assert_eq!(s.execute("edit.stepAndRepeat", &json!({"ids": [a.0, a.0], "count": 1})).unwrap()["created"], 1);

        let group = ItemId(s.execute("object.group", &json!({"ids": [a.0, b.0]})).unwrap()["id"].as_u64().unwrap());
        let d = &s.doc().unwrap().doc;
        assert_eq!(checked_step_repeat_targets(d, &[group, group, a], 1, 3).unwrap(), vec![group]);
        assert!(checked_step_repeat_targets(d, &[group, a], 1, 2).is_err());
        assert_eq!(s.execute("edit.stepAndRepeat", &json!({"ids": [group.0, a.0], "count": 1})).unwrap()["created"], 1);
    }
}

#[cfg(test)]
mod polygon_tests {
    use super::*;

    #[test]
    fn polygon_settings_make_stars() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("tool.polygonSettings", &json!({"sides": 5, "starInset": 50})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200], "shape": "polygon"})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        let it = d.item(ItemId(id)).unwrap();
        // A 5-point star has 10 vertices, fitted to the rect.
        assert_eq!(it.path.subpaths[0].anchors.len(), 10);
        let b = it.bounds();
        assert!((b.x0 - 100.0).abs() < 1e-6 && (b.y1 - 200.0).abs() < 1e-6, "{b:?}");
        // Explicit parameters win over the settings.
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50], "shape": "polygon", "sides": 3, "starInset": 0})).unwrap()["id"]
            .as_u64()
            .unwrap();
        assert_eq!(s.doc().unwrap().doc.item(ItemId(id)).unwrap().path.subpaths[0].anchors.len(), 3);
    }
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    #[test]
    fn scale_strokes_and_dimensions_with_stroke() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [100, 100, 200, 150]})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        s.execute("object.stroke", &json!({"weight": 4, "swatch": "[Black]"})).unwrap();
        let weight = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().stroke.weight;
        let bounds = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().bounds();
        s.execute("transform.scale", &json!({"sx": 2.0})).unwrap();
        assert_eq!(weight(&s), 8.0);
        assert_eq!(bounds(&s), Rect::new(50.0, 75.0, 250.0, 175.0));
        s.execute("prefs.set", &json!({"scaleStrokes": false})).unwrap();
        s.execute("transform.scale", &json!({"sx": 0.5})).unwrap();
        assert_eq!(weight(&s), 8.0);
        assert!(s.execute("prefs.set", &json!({"nope": 1})).is_err());
        // Width 108 with an 8 pt centred stroke: the path is 100 wide.
        s.execute("transform.set", &json!({"width": 108})).unwrap();
        assert!((bounds(&s).width() - 100.0).abs() < 1e-9, "{:?}", bounds(&s));
        s.execute("prefs.set", &json!({"dimensionsIncludeStroke": false})).unwrap();
        s.execute("transform.set", &json!({"width": 120})).unwrap();
        assert!((bounds(&s).width() - 120.0).abs() < 1e-9);
    }

    fn text_frame(s: &mut Session, rect: [f64; 4], text: &str) -> (ItemId, StoryId) {
        let r = s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": text})).unwrap();
        (ItemId(r["id"].as_u64().unwrap()), StoryId(r["story"].as_u64().unwrap()))
    }

    fn props_at(s: &Session, story: StoryId, pos: usize) -> (designcraft_doc::ParaProps, designcraft_doc::CharProps) {
        let d = &s.doc().unwrap().doc;
        let st = d.story(story).unwrap();
        let (pp, base) = d.styles.resolve_para(&st.paras[st.para_at(pos)]);
        (pp, d.styles.resolve_char(&base, st.char_format_at(pos)))
    }

    /// InDesign's default When Scaling: Apply to Content: Cmd+Shift-dragging a text frame's handle
    /// sets the text larger and leaves the frame at 100% (Dansky's Exclude Overlap tutorial, 2:16).
    #[test]
    fn scaling_a_text_frame_scales_its_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let (f, story) = text_frame(&mut s, [100.0, 100.0, 300.0, 200.0], "Learn\nmore");
        s.execute("text.select", &json!({"story": story.0, "anchor": 0, "focus": 5})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 20, "leading": {"kind": "points", "value": 24}, "baselineShift": 2, "tracking": -50}}))
            .unwrap();
        s.execute("type.para", &json!({"attrs": {"leftIndent": 4, "spaceAfter": 6}})).unwrap();
        s.execute("edit.deselectAll", &json!({})).unwrap();
        // The handle drag: from the top-left, twice as wide and high.
        s.execute("transform.resize", &json!({"ids": [f.0], "from": [100, 100, 300, 200], "to": [100, 100, 500, 300], "content": true})).unwrap();
        let (pp, cp) = props_at(&s, story, 0);
        assert_eq!(
            (cp.size, cp.leading, cp.baseline_shift, cp.tracking, cp.h_scale),
            (40.0, designcraft_doc::Leading::Points(48.0), 4.0, -50.0, 1.0)
        );
        assert_eq!((pp.left_indent, pp.space_after), (8.0, 12.0));
        // The second paragraph's size came from the paragraph style: it is set too.
        assert_eq!(props_at(&s, story, 7).1.size, 24.0);
        let it = s.doc().unwrap().doc.item(f).unwrap().clone();
        assert_eq!(it.bounds(), Rect::new(100.0, 100.0, 500.0, 300.0));
        let scale = s.execute("transform.info", &json!({"ids": [f.0]})).unwrap();
        assert_eq!((scale["scaleX"].as_f64(), scale["scaleY"].as_f64()), (Some(100.0), Some(100.0)), "{scale}");
        // Wider than high: the horizontal scale takes the difference.
        s.execute("transform.scale", &json!({"ids": [f.0], "sx": 1.0, "sy": 0.5})).unwrap();
        let (_, cp) = props_at(&s, story, 0);
        assert_eq!((cp.size, cp.h_scale), (20.0, 2.0));
        // The Transform panel's scale fields do the same, and show 100% afterwards.
        s.execute("transform.set", &json!({"ids": [f.0], "scaleX": 50, "scaleY": 50})).unwrap();
        assert_eq!(props_at(&s, story, 0).1.size, 10.0);
        // One undo step.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(props_at(&s, story, 0).1.size, 20.0);
    }

    /// A threaded story, a rotated scale and a flip keep the old behaviour: the frame is
    /// transformed as a whole and the text keeps its size.
    #[test]
    fn threaded_and_flipped_text_frames_scale_as_a_whole() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let (a, story) = text_frame(&mut s, [100.0, 100.0, 200.0, 140.0], "Some threaded text");
        let (b, _) = text_frame(&mut s, [100.0, 200.0, 200.0, 240.0], "");
        s.execute("text.thread", &json!({"from": a.0, "to": b.0})).unwrap();
        s.execute("transform.scale", &json!({"ids": [a.0], "sx": 2.0})).unwrap();
        assert_eq!(props_at(&s, story, 0).1.size, 12.0);
        let (c, story) = text_frame(&mut s, [300.0, 100.0, 400.0, 140.0], "Flip");
        s.execute("transform.scale", &json!({"ids": [c.0], "sx": -1.0, "sy": 1.0})).unwrap();
        assert_eq!(props_at(&s, story, 0).1.size, 12.0);
        assert!(s.doc().unwrap().doc.item(c).unwrap().xf.as_coeffs()[0] < 0.0);
        // A frame holding a table: the table would keep its size while the text grew.
        let (t, story) = text_frame(&mut s, [300.0, 300.0, 500.0, 500.0], "Grid");
        s.execute("text.select", &json!({"story": story.0, "anchor": 4, "focus": 4})).unwrap();
        s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
        s.execute("transform.scale", &json!({"ids": [t.0], "sx": 2.0})).unwrap();
        assert_eq!(props_at(&s, story, 0).1.size, 12.0);
        assert!((s.doc().unwrap().doc.item(t).unwrap().xf.as_coeffs()[0] - 2.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod gradient_tests {
    use super::*;

    #[test]
    fn gradient_tool_vector_and_unnamed_gradients() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [100, 100, 300, 200]})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        let swatches = |s: &Session| s.doc().unwrap().doc.swatches.len();
        let n = swatches(&s);
        // The default White→Black gradient, dragged from the left edge to the middle.
        let r = s.execute("object.gradient", &json!({"from": [100, 150], "to": [200, 150]})).unwrap();
        assert_eq!(r["kind"], "linear");
        assert_eq!(r["stops"][1]["color"], "#000000");
        assert_eq!(swatches(&s), n + 1, "an unnamed gradient");
        let fill = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().fill.clone();
        assert_eq!(fill(&s).gradient_vector, Some([100.0, 150.0, 200.0, 150.0]));
        // Same gradient again: no new swatch. Reverse / radial: a new unnamed gradient.
        s.execute("object.gradient", &json!({})).unwrap();
        assert_eq!(swatches(&s), n + 1);
        let r = s.execute("object.gradient", &json!({"kind": "radial", "reverse": true})).unwrap();
        assert_eq!(r["kind"], "radial");
        assert_eq!(r["stops"][0]["color"], "#000000");
        let r = s
            .execute(
                "object.gradient",
                &json!({"stops": [{"location": 0, "color": "#ff0000"}, {"location": 60, "color": "#0000ff", "midpoint": 30}]}),
            )
            .unwrap();
        assert_eq!(r["stops"][1]["location"], 60.0);
        assert_eq!(r["stops"][1]["midpoint"], 30.0);
        assert!(s.execute("object.gradient", &json!({"stops": [{"location": 0, "color": "#ff0000"}]})).is_err());
        // Scaling moves the vector with the shape.
        s.execute("transform.scale", &json!({"sx": 2.0})).unwrap();
        assert_eq!(fill(&s).gradient_vector, Some([0.0, 150.0, 200.0, 150.0]));
        // An angle replaces the vector.
        s.execute("object.gradient", &json!({"angle": 90})).unwrap();
        assert_eq!(fill(&s).gradient_vector, None);
        assert_eq!(fill(&s).gradient_angle, Some(90.0));
    }

    #[test]
    fn swatches_dropped_on_stops_keep_their_cmyk() {
        use designcraft_color::Color;
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [100, 100, 300, 200]})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        // LYH Studio episode 6: magenta on the left stop, cyan dropped at 68.636 %, a grey third.
        let r = s
            .execute(
                "object.gradient",
                &json!({"stops": [
                    {"location": 0, "swatch": "C=0 M=100 Y=0 K=0"},
                    {"location": 68.636, "swatch": "C=100 M=0 Y=0 K=0"},
                    {"location": 100, "color": {"gray": 25}}
                ]}),
            )
            .unwrap();
        let d = &s.doc().unwrap().doc;
        let g = designcraft_color::swatch::resolve_gradient(&d.swatches, r["swatch"].as_str().unwrap()).unwrap();
        assert_eq!(d.item(id).unwrap().fill.swatch, r["swatch"].as_str().unwrap());
        assert_eq!(g.stops[0].color, Color::cmyk(0.0, 1.0, 0.0, 0.0));
        assert_eq!(g.stops[1].color, Color::cmyk(1.0, 0.0, 0.0, 0.0));
        assert!((g.stops[1].offset - 0.68636).abs() < 1e-6);
        assert_eq!(g.stops[2].color, Color::gray(0.25));
        // Not a colour: refused, nothing changes.
        for bad in ["[None]", "Missing", r["swatch"].as_str().unwrap()] {
            let p = json!({"stops": [{"location": 0, "swatch": bad}, {"location": 100, "color": "#000000"}]});
            assert!(s.execute("object.gradient", &p).is_err(), "{bad}");
        }
    }
}

#[cfg(test)]
mod reference_point_tests {
    use super::*;

    #[test]
    fn rotate_and_flip_turn_about_the_reference_point() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = ItemId(s.execute("frame.create", &json!({"rect": [100, 100, 200, 150]})).unwrap()["id"].as_u64().unwrap());
        let item = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().clone();
        // 90° counterclockwise about the top-left: that corner stays, the frame swings up.
        s.execute("transform.rotate", &json!({"angle": 90, "ref": 0, "ids": [id.0]})).unwrap();
        let it = item(&s);
        assert!((it.xf * Point::new(100.0, 100.0) - Point::new(100.0, 100.0)).hypot() < 1e-9, "{:?}", it.xf);
        let b = it.bounds();
        assert!((b.x0 - 100.0).abs() < 1e-9 && (b.y0 - 0.0).abs() < 1e-9 && (b.x1 - 150.0).abs() < 1e-9 && (b.y1 - 100.0).abs() < 1e-9, "{b:?}");
        // Flip horizontally about the right edge's middle: the frame lands to the right of it.
        s.execute("transform.flip", &json!({"axis": "horizontal", "ref": 5, "ids": [id.0]})).unwrap();
        let b = item(&s).bounds();
        assert!((b.x0 - 150.0).abs() < 1e-9 && (b.x1 - 200.0).abs() < 1e-9 && (b.y0 - 0.0).abs() < 1e-9, "{b:?}");
    }
}

#[cfg(test)]
mod again_tests {
    use super::*;

    #[test]
    fn transform_again_sequence_and_clear() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = ItemId(s.execute("frame.create", &json!({"rect": [100, 100, 200, 150]})).unwrap()["id"].as_u64().unwrap());
        let b = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().bounds();
        assert!(s.execute("transform.again", &json!({})).is_err());
        s.execute("transform.move", &json!({"dx": 10, "dy": 0})).unwrap();
        s.execute("transform.again", &json!({})).unwrap();
        assert_eq!(b(&s).x0, 120.0);
        s.execute("transform.rotate", &json!({"angle": 90})).unwrap();
        // Sequence: move 10 then rotate 90 again.
        s.execute("transform.again", &json!({"sequence": true})).unwrap();
        // The 100×50 frame is now 180° round from where it started: still 100 wide.
        assert!((b(&s).width() - 100.0).abs() < 1e-6, "{:?}", b(&s));
        // Clear Transformations: 180° in total → upright again, same centre.
        let c = b(&s).center();
        s.execute("transform.clear", &json!({})).unwrap();
        let it = s.doc().unwrap().doc.item(id).unwrap().clone();
        assert_eq!(it.xf.as_coeffs()[..4], [1.0, 0.0, 0.0, 1.0]);
        assert!((it.bounds().center() - c).hypot() < 1e-6);
        assert!((it.bounds().width() - 100.0).abs() < 1e-6);
        // Step and repeat via Transform Again on a moved copy.
        let n = s.doc().unwrap().doc.spreads[0].items.len();
        s.execute("transform.move", &json!({"dx": 0, "dy": 60, "copy": true})).unwrap();
        s.execute("transform.again", &json!({})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.spreads[0].items.len(), n + 2);
        let ys: Vec<f64> = d.spreads[0].items[n..].iter().map(|i| i.bounds().y0.round()).collect();
        assert_eq!(ys[1] - ys[0], 60.0, "{ys:?}");
    }
}

#[cfg(test)]
mod label_tests {
    use super::*;

    #[test]
    fn script_label_alt_text_and_group_transparency() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [10, 10, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [60, 10, 90, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.label", &json!({"label": "price", "ids": [a, b]})).unwrap();
        s.execute("object.altText", &json!({"text": "A red square", "ids": [a]})).unwrap();
        assert_eq!(s.execute("object.findByLabel", &json!({"label": "price"})).unwrap()["ids"], json!([a, b]));
        assert_eq!(s.execute("object.findByLabel", &json!({"label": "nope"})).unwrap()["ids"], json!([]));
        assert_eq!(s.doc().unwrap().doc.item(ItemId(a)).unwrap().alt_text, "A red square");
        s.execute("object.transparencyGroup", &json!({"knockout": true, "ids": [b]})).unwrap();
        let it = s.doc().unwrap().doc.item(ItemId(b)).unwrap().clone();
        assert!(it.knockout && !it.isolate);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!s.doc().unwrap().doc.item(ItemId(b)).unwrap().knockout);
    }
}

#[cfg(test)]
mod light_tests {
    use super::*;

    #[test]
    fn global_light_moves_shadows_that_use_it() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [10, 10, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.dropShadow", &json!({"on": true, "angle": 45, "ids": [a]})).unwrap();
        let angle = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            let ds = &d.item(ItemId(a)).unwrap().effects.drop_shadow;
            d.light_angle(ds.angle, ds.global_light)
        };
        s.execute("object.globalLight", &json!({"angle": 200})).unwrap();
        assert_eq!(angle(&s), 45.0, "own angle");
        s.execute("object.dropShadow", &json!({"globalLight": true, "on": true, "ids": [a]})).unwrap();
        assert_eq!(angle(&s), -160.0, "normalised to -180..180");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(angle(&s), 45.0);
    }
}

#[cfg(test)]
mod transform_values_tests {
    use super::*;

    #[test]
    fn absolute_rotation_shear_and_totals() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 150]})).unwrap()["id"].as_u64().unwrap();
        let info = |s: &mut Session, id: u64| s.execute("transform.info", &json!({"ids": [id]})).unwrap();
        let centre = |s: &Session| s.doc().unwrap().doc.item(ItemId(a)).unwrap().bounds().center();
        let c0 = centre(&s);
        s.execute("transform.set", &json!({"rotation": 30, "ids": [a]})).unwrap();
        s.execute("transform.set", &json!({"rotation": 30, "ids": [a]})).unwrap();
        assert_eq!(info(&mut s, a)["rotation"], 30.0, "absolute, not cumulative");
        assert!((centre(&s) - c0).hypot() < 1e-9, "about the centre reference point");
        s.execute("transform.set", &json!({"shear": 20, "ids": [a]})).unwrap();
        let i = info(&mut s, a);
        assert_eq!((i["rotation"].as_f64(), i["shear"].as_f64()), (Some(30.0), Some(20.0)));
        assert_eq!(i["scaleX"], 100.0);
        // Nested: a group rotated 10° around the rotated frame.
        let b = s.execute("frame.create", &json!({"rect": [300, 100, 350, 150]})).unwrap()["id"].as_u64().unwrap();
        let g = s.execute("object.group", &json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
        s.execute("transform.rotate", &json!({"angle": 10, "ids": [g]})).unwrap();
        assert_eq!(info(&mut s, a)["rotation"], 40.0, "totals");
        s.execute("prefs.set", &json!({"transformationsAreTotals": false})).unwrap();
        assert_eq!(info(&mut s, a)["rotation"], 30.0, "relative to the group");
        s.execute("transform.set", &json!({"rotation": 0, "shear": 0, "ids": [a]})).unwrap();
        s.execute("prefs.set", &json!({"transformationsAreTotals": true})).unwrap();
        assert_eq!(info(&mut s, a)["rotation"], 10.0);
        // Scale is relative to what's shown: a frame always shows 100%.
        s.execute("transform.set", &json!({"scaleX": 200, "ids": [b]})).unwrap();
        assert_eq!(info(&mut s, b)["scaleX"], 100.0);
    }
}

#[cfg(test)]
mod attributes_tests {
    use super::*;

    #[test]
    fn attributes_panel_flags() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [10, 10, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.attributes", &json!({"overprintFill": true, "overprintGap": true, "nonprinting": true})).unwrap();
        let it = s.doc().unwrap().doc.item(ItemId(a)).unwrap().clone();
        assert!(it.fill.overprint && it.stroke.gap_overprint && it.nonprinting && !it.stroke.overprint);
        // IDML keeps them.
        let bytes = designcraft_idml::export_idml(&s.doc().unwrap().doc);
        let back = designcraft_idml::import_idml(&bytes).unwrap();
        let it = back.spreads.iter().flat_map(|sp| sp.items.iter()).find(|i| i.fill.overprint).expect("overprint fill survives");
        assert!(it.stroke.gap_overprint);
    }
}

#[cfg(test)]
mod fitting_tests {
    use super::*;

    #[test]
    fn inner_glow_bevel_and_satin_change_the_inside() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "C=100 M=0 Y=0 K=0", "ids": [id]})).unwrap();
        let shot = |s: &Session| {
            let d = s.doc().unwrap().doc.clone();
            let mut rr = designcraft_render::Renderer::new();
            rr.threads = 0;
            rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap()
        };
        let plain = shot(&s);
        // Inner glow (paper colour, screened): the edge lightens, the centre doesn't.
        s.execute("object.innerGlow", &json!({"ids": [id], "on": true, "size": 20})).unwrap();
        let g = shot(&s);
        assert!(g.pixel(103, 200)[0] > plain.pixel(103, 200)[0] + 30, "edge lighter");
        assert_eq!(g.pixel(200, 200), plain.pixel(200, 200));
        s.execute("object.innerGlow", &json!({"ids": [id], "on": false})).unwrap();
        // Bevel: lit and shaded sides differ.
        s.execute("object.bevel", &json!({"ids": [id], "on": true, "size": 20, "angle": 120, "globalLight": false})).unwrap();
        let b = shot(&s);
        let (top_left, bottom_right) = (b.pixel(104, 104), b.pixel(296, 296));
        assert!(top_left[1] as i32 > bottom_right[1] as i32 + 30, "{top_left:?} {bottom_right:?}");
        s.execute("object.bevel", &json!({"ids": [id], "on": false})).unwrap();
        // Satin darkens somewhere inside.
        s.execute("object.satin", &json!({"ids": [id], "on": true, "distance": 40, "size": 10})).unwrap();
        let st = shot(&s);
        let darker = (110..290)
            .step_by(10)
            .flat_map(|x| (110..290).step_by(10).map(move |y| (x, y)))
            .any(|(x, y)| (st.pixel(x, y)[1] as i32) < plain.pixel(x, y)[1] as i32 - 30);
        assert!(darker);
        s.execute("object.satin", &json!({"ids": [id], "on": false})).unwrap();
        // Directional feather: only the left edge fades.
        s.execute("object.directionalFeather", &json!({"ids": [id], "on": true, "widths": [0, 40, 0, 0]})).unwrap();
        let df = shot(&s);
        assert!(df.pixel(102, 200)[0] > 200, "left edge faded to paper: {:?}", df.pixel(102, 200));
        assert_eq!(df.pixel(298, 200), plain.pixel(298, 200), "right edge untouched");
        // A knockout group stays vector, as a knockout transparency group.
        let other = s.execute("frame.create", &json!({"rect": [400, 100, 450, 150]})).unwrap()["id"].clone();
        let other2 = s.execute("frame.create", &json!({"rect": [420, 120, 470, 170]})).unwrap()["id"].clone();
        let g = s.execute("object.group", &json!({"ids": [other, other2]})).unwrap()["id"].clone();
        s.execute("object.transparencyGroup", &json!({"ids": [g], "knockout": true})).unwrap();
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("1 object(s)")), "{r}");
        assert!(String::from_utf8_lossy(&super::super::file::base64_decode(r["base64"].as_str().unwrap())).contains("/K true"));
        // PDF export carries effects (as an image of the object).
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("soft effects")), "{r}");
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        assert!(String::from_utf8_lossy(&bytes).contains("/Subtype/Image"));
    }

    #[test]
    fn a_flat_line_resizes_along_its_length() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("line.create", &json!({"a": [100, 100], "b": [200, 100]})).unwrap()["id"].as_u64().unwrap();
        s.execute("transform.resize", &json!({"ids": [id], "from": [100, 100, 200, 100], "to": [100, 100, 300, 100]})).unwrap();
        let r = s.doc().unwrap().doc.item(ItemId(id)).unwrap().bounds();
        assert!((r.x0 - 100.0).abs() < 1e-9 && (r.x1 - 300.0).abs() < 1e-9 && (r.y0 - 100.0).abs() < 1e-9 && r.height().abs() < 1e-9, "{r:?}");
    }

    #[test]
    fn live_distribute_spreads_objects_without_resizing() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 120, 120]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [180, 100, 200, 120]})).unwrap()["id"].as_u64().unwrap();
        s.execute("transform.resize", &json!({"ids": [a, b], "from": [100, 100, 200, 120], "to": [100, 100, 300, 120], "distribute": true})).unwrap();
        let d = &s.doc().unwrap().doc;
        let (ra, rb) = (d.item(ItemId(a)).unwrap().bounds(), d.item(ItemId(b)).unwrap().bounds());
        assert!((ra.width() - 20.0).abs() < 1e-9 && (rb.width() - 20.0).abs() < 1e-9, "sizes kept");
        assert!((ra.center().x - 120.0).abs() < 1e-9, "{ra:?}");
        assert!((rb.center().x - 280.0).abs() < 1e-9, "{rb:?}");
    }

    #[test]
    fn frame_fitting_options_align_crop_and_auto_fit() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // A 40×20 pt image placed into a 200×200 frame.
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
        let f = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
        s.execute("file.place", &json!({"base64": super::super::file::base64_encode(&png), "name": "a.png", "frame": f})).unwrap();
        let content = |s: &Session| {
            let it = s.doc().unwrap().doc.item(ItemId(f)).unwrap().clone();
            let Content::Graphic(g) = &it.content else { panic!("graphic") };
            it.xf.transform_rect_bbox(g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1)))
        };
        // Fit proportionally from the top-left: 200 wide, 100 high, against the top.
        s.execute("object.fittingOptions", &json!({"ids": [f], "fitting": "fitProportionally", "align": 0})).unwrap();
        let c = content(&s);
        assert!((c.x0 - 100.0).abs() < 1e-6 && (c.y0 - 100.0).abs() < 1e-6 && (c.width() - 200.0).abs() < 1e-6, "{c:?}");
        // Crop 10 pt on every side and fit again: the content spreads past the frame.
        s.execute("object.fittingOptions", &json!({"ids": [f], "fitting": "fitProportionally", "crop": 10, "align": 4})).unwrap();
        let c = content(&s);
        assert!((c.width() - 220.0).abs() < 1e-6, "{c:?}");
        // Auto-fit: resizing the frame refits the content.
        s.execute("object.fittingOptions", &json!({"ids": [f], "autoFit": true, "fitting": "fillProportionally", "crop": 0})).unwrap();
        s.execute("transform.resize", &json!({"from": [100, 100, 300, 300], "to": [100, 100, 500, 300], "ids": [f]})).unwrap();
        let c = content(&s);
        assert!((c.width() - 400.0).abs() < 1e-6, "fills the wider frame: {c:?}");
        // IDML keeps the options.
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        let g = back.spreads.iter().flat_map(|sp| sp.items.iter()).find_map(|i| i.graphic().cloned()).unwrap();
        assert_eq!(g.auto_fit, designcraft_doc::Fitting::FillProportionally);
    }
}

#[cfg(test)]
mod primary_frame_tests {
    use super::*;

    #[test]
    fn toggle_primary_text_frame() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 300], "content": "text", "text": "x", "caret": false})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("object.primaryTextFrame", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.settings.primary_story.map(|x| x.0), Some(sid));
        s.execute("object.primaryTextFrame", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.settings.primary_story, None, "toggles off");
        let g = s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].clone();
        assert!(s.execute("object.primaryTextFrame", &json!({"ids": [g]})).is_err());
    }
}

#[cfg(test)]
mod clipping_tests {
    use super::*;

    #[test]
    fn clipping_path_from_alpha_traces_the_shape() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // 40×40 image: an opaque disc on transparency.
        let mut px = vec![0u8; 40 * 40 * 4];
        for y in 0..40 {
            for x in 0..40 {
                let (dx, dy) = (x as f64 + 0.5 - 20.0, y as f64 + 0.5 - 20.0);
                if (dx * dx + dy * dy).sqrt() < 15.0 {
                    px[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4].copy_from_slice(&[200, 0, 0, 255]);
                }
            }
        }
        let png = designcraft_render::Rendered { width: 40, height: 40, pixels: px }.to_png();
        let f = s.execute("frame.create", &json!({"rect": [100, 100, 140, 140]})).unwrap()["id"].as_u64().unwrap();
        s.execute("file.place", &json!({"base64": super::super::file::base64_encode(&png), "name": "disc.png", "frame": f})).unwrap();
        s.execute("object.clippingPath", &json!({"ids": [f], "tolerance": 0.5})).unwrap();
        let it = s.doc().unwrap().doc.item(ItemId(f)).unwrap().clone();
        let b = it.bounds();
        // The disc spans 5..35 of the 40 px image placed at 100..140.
        assert!((b.x0 - 105.0).abs() < 1.5 && (b.x1 - 135.0).abs() < 1.5, "{b:?}");
        assert_eq!(it.shape, Shape::Path);
        assert!(s.execute("object.clippingPath", &json!({"ids": [f], "threshold": 255})).is_err());
    }

    #[test]
    fn clipping_path_options_report_invert_and_remove_the_clip() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let png = designcraft_render::Rendered { width: 4, height: 4, pixels: vec![255; 64] }.to_png();
        let f = s.execute("frame.create", &json!({"rect": [100, 100, 140, 140]})).unwrap()["id"].as_u64().unwrap();
        s.execute("file.place", &json!({"base64": super::super::file::base64_encode(&png), "name": "white.png", "frame": f})).unwrap();
        let get = |s: &mut Session| s.execute("object.clippingPathOptions", &json!({"ids": [f]})).unwrap()["items"][0].clone();
        assert_eq!(get(&mut s)["type"], "none");
        // As an imported file would carry it.
        let triangle = designcraft_geom::PathData::single(designcraft_geom::path::SubPath::polyline(
            &[Point::new(0.0, 4.0), Point::new(2.0, 0.0), Point::new(4.0, 4.0)],
            true,
        ));
        s.edit(|d, _| {
            if let Some(Content::Graphic(g)) = d.item_mut(ItemId(f)).map(|it| &mut it.content) {
                g.clip = Some(Arc::new(designcraft_doc::ClippingPath::new(designcraft_doc::ClippingKind::PhotoshopPath, triangle)));
            }
            Ok(json!({}))
        })
        .unwrap();
        let c = get(&mut s);
        assert_eq!((c["type"].as_str(), c["anchors"].as_u64(), c["invert"].as_bool()), (Some("photoshopPath"), Some(3), Some(false)));
        s.execute("object.clippingPathOptions", &json!({"ids": [f], "invert": true})).unwrap();
        assert_eq!(get(&mut s)["invert"], true);
        assert_eq!(s.execute("object.clippingPathOptions", &json!({"ids": [f], "type": "none"})).unwrap()["changed"], 1);
        assert_eq!(get(&mut s)["type"], "none");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(get(&mut s)["type"], "photoshopPath");
        assert!(s.execute("object.clippingPathOptions", &json!({"ids": [f], "type": "alphaChannel"})).is_err());
        assert!(s.execute("object.clippingPathOptions", &json!({"ids": [f], "invert": "yes"})).is_err());
    }
}

#[cfg(test)]
mod gap_tests {
    use super::*;

    #[test]
    fn moving_a_gap_resizes_both_sides() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 300]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [220, 100, 320, 300]})).unwrap()["id"].as_u64().unwrap();
        let r = s.execute("gap.move", &json!({"at": [210, 200], "delta": 30})).unwrap();
        assert_eq!((r["axis"].as_str(), r["changed"].as_u64()), (Some("x"), Some(2)));
        let bb = |s: &Session, id: u64| s.doc().unwrap().doc.item(ItemId(id)).unwrap().bounds();
        assert_eq!((bb(&s, a).x1, bb(&s, b).x0), (230.0, 250.0), "the gap moved, its width kept");
        assert_eq!(bb(&s, b).x1, 320.0);
        assert!(s.execute("gap.move", &json!({"at": [150, 200], "delta": 5})).is_err(), "inside an object");
    }
}

#[cfg(test)]
mod line_end_tests {
    use super::*;

    /// "One\rTwo" made one paragraph (the CR stayed in the text) instead of two.
    #[test]
    fn carriage_returns_separate_paragraphs() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "One\rTwo\r\nThree"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let story = |s: &Session| s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(story(&s).text, "One\nTwo\nThree");
        assert_eq!(story(&s).paras.len(), 3);
        // The caret is at the end: typed text gets the same treatment.
        s.execute("text.insert", &json!({"text": "\rFour", "raw": true})).unwrap();
        assert_eq!(story(&s).text, "One\nTwo\nThree\nFour");
        assert_eq!(story(&s).paras.len(), 4);
        // Editing a range or the whole story (scripts written for InDesign end paragraphs with \r).
        s.execute("story.replaceRange", &json!({"story": sid.0, "start": 0, "end": 3, "text": "Zero\rOne"})).unwrap();
        assert_eq!((story(&s).text.as_str(), story(&s).paras.len()), ("Zero\nOne\nTwo\nThree\nFour", 5));
        s.execute("story.setText", &json!({"story": sid.0, "text": "A\r\nB\rC"})).unwrap();
        assert_eq!((story(&s).text.as_str(), story(&s).paras.len()), ("A\nB\nC", 3));
        // Notes, made and edited.
        s.execute("text.select", &json!({"story": sid.0, "anchor": 1})).unwrap();
        let n = s.execute("note.new", &json!({"text": "One\rTwo"})).unwrap();
        let note = |s: &mut Session| s.execute("note.list", &json!({})).unwrap()[0]["text"].clone();
        assert_eq!(note(&mut s), "One\nTwo");
        s.execute("note.edit", &json!({"story": n["story"], "id": n["id"], "text": "Three\r\nFour"})).unwrap();
        assert_eq!(note(&mut s), "Three\nFour");
    }

    /// "One\u{B}Two" kept the vertical tab, drawn as a missing-glyph box, where Word and
    /// PowerPoint text (Chr(11)) means a forced line break.
    #[test]
    fn vertical_tabs_are_forced_line_breaks() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "One\u{B}Two"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let story = |s: &Session| s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!((story(&s).text.as_str(), story(&s).paras.len()), ("One\u{2028}Two", 1));
        s.execute("text.insert", &json!({"text": "\u{B}Three", "raw": true})).unwrap();
        assert_eq!((story(&s).text.as_str(), story(&s).paras.len()), ("One\u{2028}Two\u{2028}Three", 1));
        s.execute("story.setText", &json!({"story": sid.0, "text": "A\u{B}B\rC"})).unwrap();
        assert_eq!((story(&s).text.as_str(), story(&s).paras.len()), ("A\u{2028}B\nC", 2));
    }
}

#[cfg(test)]
mod named_target_tests {
    use super::*;

    /// With a text caret (or nothing) selected, a command given `ids` failed
    /// with "command `object.textFrameOptions` is not available right now: nothing selected".
    #[test]
    fn ids_act_without_a_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Hello"})).unwrap();
        assert!(s.doc().unwrap().selection.items.is_empty(), "frame.create leaves a text caret");
        let e = s.execute("object.textFrameOptions", &json!({"columns": 2})).unwrap_err().to_string();
        assert!(e.contains("nothing selected"), "{e}");
        s.execute("object.textFrameOptions", &json!({"ids": [r["id"]], "columns": 2})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        let columns = |s: &Session| s.doc().unwrap().doc.item(id).and_then(|i| i.text_frame().map(|t| t.options.columns));
        assert_eq!(columns(&s), Some(2));
        s.execute("transform.set", &json!({"id": r["id"], "x": 100, "ref": 0})).unwrap();
        let e = s.execute("object.textFrameOptions", &json!({"ids": [99999], "columns": 1})).unwrap_err().to_string();
        assert!(e.contains("no object with id 99999"), "{e}");
        assert_eq!(columns(&s), Some(2));
        // An empty `ids` names nothing (as `targets` reads it), even beside an `id`.
        let e = s.execute("object.textFrameOptions", &json!({"ids": [], "id": r["id"], "columns": 3})).unwrap_err().to_string();
        assert!(e.contains("nothing selected"), "{e}");
        assert_eq!(columns(&s), Some(2));
        // Commands documented with `ids` honour `id` too.
        assert_eq!(s.execute("conveyor.collect", &json!({"id": r["id"]})).unwrap()["count"], 1);
    }
}

#[cfg(test)]
mod column_rule_tests {
    use super::*;

    #[test]
    fn column_rules_set_validate_undo_and_inspect() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Hello"})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        let opts = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone();
        s.execute("object.textFrameOptions", &json!({"ids": [id.0], "columns": 3, "inset": [1, 2, 3, 4]})).unwrap();
        let before = opts(&s);
        // An unknown swatch or a bad weight is an error and changes nothing.
        let e = s.execute("object.textFrameOptions", &json!({"ids": [id.0], "columnRule": true, "columnRuleColor": "Mauve"})).unwrap_err();
        assert!(e.to_string().contains("Mauve"), "{e}");
        assert!(s.execute("object.textFrameOptions", &json!({"ids": [id.0], "columnRuleWeight": -1})).is_err());
        assert!(s.execute("object.textFrameOptions", &json!({"ids": [id.0], "columnRuleTint": 2})).is_err());
        assert!(s.execute("object.textFrameOptions", &json!({"ids": [id.0], "columnRuleTopInset": "x"})).is_err());
        assert_eq!(opts(&s), before);
        s.execute(
            "object.textFrameOptions",
            &json!({"ids": [id.0], "columnRule": true, "columnRuleWeight": 0.5, "columnRuleColor": "C=100 M=0 Y=0 K=0", "columnRuleTint": 0.5,
                "columnRuleOffset": -2, "columnRuleTopInset": 4, "columnRuleBottomInset": 6}),
        )
        .unwrap();
        let o = opts(&s);
        assert!(o.column_rule && o.column_rule_weight == 0.5 && o.column_rule_color == "C=100 M=0 Y=0 K=0");
        assert_eq!(o.inset, [1.0, 2.0, 3.0, 4.0], "untouched options stay");
        assert_eq!((o.column_rule_tint, o.column_rule_offset, o.column_rule_top_inset, o.column_rule_bottom_inset), (0.5, -2.0, 4.0, 6.0));
        let item = s.execute("document.inspect", &json!({})).unwrap()["spreads"][0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == json!(id.0))
            .cloned()
            .unwrap();
        assert_eq!(
            (&item["columnRule"], &item["columnRuleWeight"], &item["columnRuleColor"]),
            (&json!(true), &json!(0.5), &json!("C=100 M=0 Y=0 K=0"))
        );
        assert_eq!(
            (&item["columnRuleTint"], &item["columnRuleOffset"], &item["columnRuleTopInset"], &item["columnRuleBottomInset"]),
            (&json!(0.5), &json!(-2.0), &json!(4.0), &json!(6.0))
        );
        // One undo step.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(opts(&s), before);
    }
}

#[cfg(test)]
mod blend_mode_tests {
    use designcraft_color::BlendMode;

    use super::*;

    /// `object.opacity` documents `blend?: normal|multiply|…`, but deserializing the value
    /// against the enum only accepted the PascalCase variant names, so the documented
    /// lowercase forms were silently dropped: the command reported `changed: 1` while the
    /// stored blend stayed as it was (#260).
    #[test]
    fn lowercase_documented_blend_modes_apply() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "unassigned"})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        let blend = |s: &Session| s.doc().unwrap().doc.item(id).map(|i| i.blend);

        for (given, want) in [
            ("multiply", BlendMode::Multiply),
            ("screen", BlendMode::Screen),
            ("MULTIPLY", BlendMode::Multiply),
            ("Color Burn", BlendMode::ColorBurn),
            ("color-burn", BlendMode::ColorBurn),
            ("soft_light", BlendMode::SoftLight),
            ("normal", BlendMode::Normal),
        ] {
            let out = s.execute("object.opacity", &json!({"opacity": 1, "blend": given})).unwrap();
            assert_eq!(out["changed"], 1, "{given}");
            assert_eq!(blend(&s), Some(want), "blend mode {given:?} must be applied");
        }
    }

    /// The PascalCase names keep working, so the Properties panel's own submissions are
    /// unaffected.
    #[test]
    fn pascal_case_blend_modes_still_apply() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "unassigned"})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        s.execute("object.opacity", &json!({"opacity": 0.5, "blend": "Multiply"})).unwrap();
        let it = s.doc().unwrap().doc.item(id).cloned().unwrap();
        assert_eq!(it.blend, BlendMode::Multiply);
        assert!((it.opacity - 0.5).abs() < 1e-6, "opacity must still be set");
    }

    /// A blend mode the command doesn't know is an error, not a silent success that leaves
    /// the previous appearance in place.
    #[test]
    fn an_unknown_blend_mode_is_an_error() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "unassigned"})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        s.execute("object.opacity", &json!({"opacity": 1, "blend": "Multiply"})).unwrap();
        let e = s.execute("object.opacity", &json!({"opacity": 1, "blend": "sparkle"})).unwrap_err().to_string();
        assert!(e.contains("sparkle") && e.contains("blend"), "{e}");
        let it = s.doc().unwrap().doc.item(id).cloned().unwrap();
        assert_eq!(it.blend, BlendMode::Multiply, "a rejected value changes nothing");
        assert_eq!(it.opacity, 1.0);
    }
}

#[cfg(test)]
mod reorder_tests {
    use super::*;

    fn frame(s: &mut Session) -> u64 {
        s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap()
    }

    /// Top-level objects of the first spread on `layer`, topmost first.
    fn stack(s: &Session, layer: u64) -> Vec<u64> {
        let d = &s.doc().unwrap().doc;
        d.spreads[0].items.iter().rev().filter(|i| i.layer.0 == layer).map(|i| i.id.0).collect()
    }

    #[test]
    fn reorder_within_and_across_layers_validates_and_undoes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let l1 = s.doc().unwrap().doc.layers[0].id.0;
        let l2 = s.execute("layer.new", &json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.activate", &json!({"id": l1})).unwrap();
        let (a, b, c) = (frame(&mut s), frame(&mut s), frame(&mut s));
        s.execute("layer.activate", &json!({"id": l2})).unwrap();
        let (x, y) = (frame(&mut s), frame(&mut s));
        assert_eq!(stack(&s, l1), [c, b, a]);
        s.execute("object.reorder", &json!({"ids": [a], "above": c})).unwrap();
        assert_eq!(stack(&s, l1), [a, c, b]);
        s.execute("object.reorder", &json!({"ids": [b], "index": 0})).unwrap();
        assert_eq!(stack(&s, l1), [b, a, c]);
        // To another layer at a position.
        s.execute("object.reorder", &json!({"ids": [a], "layer": l2, "index": 1})).unwrap();
        assert_eq!((stack(&s, l1), stack(&s, l2)), (vec![b, c], vec![y, a, x]));
        s.execute("object.reorder", &json!({"ids": [x, y], "below": c})).unwrap();
        assert_eq!((stack(&s, l1), stack(&s, l2)), (vec![b, c, y, x], vec![a]));
        // Bad input is an error and changes nothing.
        let before = s.doc().unwrap().doc.clone();
        for bad in [
            json!({"ids": [999], "index": 0}),
            json!({"ids": [b], "index": 4}),
            json!({"ids": [b], "above": b}),
            json!({"ids": [b], "above": 999}),
            json!({"ids": [b], "layer": 999}),
            json!({"ids": [b], "layer": l2, "above": c}),
            json!({"ids": [b], "above": c, "index": 0}),
            json!({"ids": [b], "index": "top"}),
            json!({"ids": []}),
        ] {
            assert!(s.execute("object.reorder", &bad).is_err(), "{bad}");
            assert_eq!(*s.doc().unwrap().doc, *before, "{bad}");
        }
        // The bottom of a layer is index = its count.
        s.execute("object.reorder", &json!({"ids": [b], "index": 3})).unwrap();
        assert_eq!(stack(&s, l1), [c, y, x, b]);
        // A reorder that changes nothing adds no undo step.
        let undo = |s: &Session| s.doc().unwrap().history.undo.len();
        let steps = undo(&s);
        s.execute("object.reorder", &json!({"ids": [x], "index": 2})).unwrap();
        s.execute("object.reorder", &json!({"ids": [y], "above": x})).unwrap();
        s.execute("object.reorder", &json!({"ids": [c], "layer": l1, "index": 0})).unwrap();
        assert_eq!(stack(&s, l1), [c, y, x, b]);
        assert_eq!(undo(&s), steps);
        // One undo step per reorder.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(stack(&s, l1), [b, c, y, x]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!((stack(&s, l1), stack(&s, l2)), (vec![b, c], vec![y, a, x]));
    }

    #[test]
    fn group_members_reorder_only_within_their_group() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let (a, b, c) = (frame(&mut s), frame(&mut s), frame(&mut s));
        let g = s.execute("object.group", &json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
        let kids = |s: &Session| s.doc().unwrap().doc.item(ItemId(g)).unwrap().children().iter().map(|i| i.id.0).collect::<Vec<_>>();
        assert_eq!(kids(&s), [a, b]);
        s.execute("object.reorder", &json!({"ids": [a], "above": b})).unwrap();
        assert_eq!(kids(&s), [b, a]);
        s.execute("object.reorder", &json!({"ids": [a], "index": 1})).unwrap();
        assert_eq!(kids(&s), [a, b]);
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("object.reorder", &json!({"ids": [a], "above": c})).is_err(), "out of the group");
        assert!(s.execute("object.reorder", &json!({"ids": [c], "above": a})).is_err(), "into the group");
        assert!(s.execute("object.reorder", &json!({"ids": [a, c], "index": 0})).is_err(), "mixed parents");
        assert_eq!(*s.doc().unwrap().doc, *before);
    }

    #[test]
    fn moving_a_group_member_to_a_layer_moves_its_group() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let (a, b) = (frame(&mut s), frame(&mut s));
        let g = s.execute("object.group", &json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
        let art = s.execute("layer.new", &json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap();
        let r = s.execute("object.setLayer", &json!({"ids": [a, b], "layer": art})).unwrap();
        assert_eq!(r["changed"], 1, "the group, once");
        let d = &s.doc().unwrap().doc;
        assert!([g, a, b].iter().all(|i| d.item(ItemId(*i)).unwrap().layer.0 == art));
        assert!(s.execute("object.setLayer", &json!({"ids": [a], "layer": 999})).is_err());
    }
}

#[cfg(test)]
mod content_move_tests {
    use super::*;

    /// A session with a 40×20 image placed in a frame at (100, 100)–(300, 300).
    fn placed() -> (Session, ItemId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
        let f = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        s.execute("place.load", &json!({"base64": super::super::base64_encode(&png)})).unwrap();
        s.execute("place.drop", &json!({"frame": f})).unwrap();
        (s, ItemId(f))
    }

    /// Spread bounds of the frame and of its graphic.
    fn boxes(s: &Session, id: ItemId) -> (Rect, Rect) {
        let it = s.doc().unwrap().doc.item(id).unwrap().clone();
        let g = it.graphic().unwrap();
        (it.bounds(), (it.xf * g.xf).transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1)))
    }

    fn near(a: Rect, b: Rect) -> bool {
        [a.x0 - b.x0, a.y0 - b.y0, a.x1 - b.x1, a.y1 - b.y1].iter().all(|d| d.abs() < 1e-6)
    }

    #[test]
    fn content_moves_inside_the_frame_in_one_undo_step() {
        let (mut s, f) = placed();
        let (frame0, g0) = boxes(&s, f);
        s.execute("selection.set", &json!({"ids": [f.0], "content": true})).unwrap();
        s.execute("transform.move", &json!({"dx": 10, "dy": -5})).unwrap();
        let (frame1, g1) = boxes(&s, f);
        assert!(near(frame1, frame0), "the frame stays");
        assert!(near(g1, g0 + Vec2::new(10.0, -5.0)), "{g1:?}");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(near(boxes(&s, f).1, g0));
        // Explicit ids with `content` act on the content whatever the selection is.
        s.execute("selection.set", &json!({"ids": [f.0]})).unwrap();
        s.execute("transform.move", &json!({"ids": [f.0], "content": true, "dx": 3})).unwrap();
        assert!(near(boxes(&s, f).0, frame0) && near(boxes(&s, f).1, g0 + Vec2::new(3.0, 0.0)));
        // A frame selection moves frame and content together.
        s.execute("transform.move", &json!({"dx": 4})).unwrap();
        assert!(near(boxes(&s, f).0, frame0 + Vec2::new(4.0, 0.0)) && near(boxes(&s, f).1, g0 + Vec2::new(7.0, 0.0)));
    }

    #[test]
    fn content_moves_on_the_page_axes_in_a_rotated_frame_and_pasted_items_move_too() {
        let (mut s, f) = placed();
        s.execute("transform.rotate", &json!({"ids": [f.0], "angle": 90})).unwrap();
        let (frame0, g0) = boxes(&s, f);
        s.execute("transform.move", &json!({"ids": [f.0], "content": true, "dx": 10})).unwrap();
        let (frame1, g1) = boxes(&s, f);
        assert!(near(frame1, frame0) && near(g1, g0 + Vec2::new(10.0, 0.0)), "{g0:?} → {g1:?}");
        // Items pasted into a frame are its content.
        let a = s.execute("frame.create", &json!({"rect": [400, 100, 420, 120], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [a]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        let b = s.execute("frame.create", &json!({"rect": [400, 300, 500, 400], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [b]})).unwrap();
        s.execute("edit.pasteInto", &json!({})).unwrap();
        let kids = |s: &Session| {
            let it = s.doc().unwrap().doc.item(ItemId(b)).unwrap().clone();
            (it.bounds(), it.children().iter().map(|c| it.xf.transform_rect_bbox(c.bounds())).collect::<Vec<_>>())
        };
        let (fb0, k0) = kids(&s);
        s.execute("transform.move", &json!({"ids": [b], "content": true, "dx": 5, "dy": 6})).unwrap();
        let (fb1, k1) = kids(&s);
        assert!(near(fb1, fb0) && near(k1[0], k0[0] + Vec2::new(5.0, 6.0)), "{k0:?} → {k1:?}");
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;

    fn frame(s: &mut Session, rect: [f64; 4], spread: usize) -> ItemId {
        ItemId(s.execute("frame.create", &json!({"rect": rect, "spread": spread})).unwrap()["id"].as_u64().unwrap())
    }

    fn group_ids(s: &mut Session, ids: &[ItemId]) -> Result<ItemId> {
        let r = s.execute("object.group", &json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}))?;
        Ok(ItemId(r["id"].as_u64().unwrap()))
    }

    fn children(s: &Session, id: ItemId) -> Vec<ItemId> {
        s.doc().unwrap().doc.item(id).unwrap().children().iter().map(|c| c.id).collect()
    }

    fn world(s: &Session, id: ItemId) -> Affine {
        let d = &s.doc().unwrap().doc;
        let loc = d.find(id).unwrap();
        d.parent_xf(&loc) * d.item(id).unwrap().xf
    }

    fn group_enabled(s: &Session) -> bool {
        super::super::find_command("object.group").unwrap().info(s).enabled
    }

    #[test]
    fn grouping_objects_already_in_a_group_does_not_panic() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let a = frame(&mut s, [36.0, 36.0, 136.0, 136.0], 0);
        let b = frame(&mut s, [200.0, 36.0, 300.0, 136.0], 0);
        let g = group_ids(&mut s, &[a, b]).unwrap();
        let inner = group_ids(&mut s, &[a, b]).unwrap();
        assert_eq!(children(&s, g), vec![inner]);
        assert_eq!(children(&s, inner), vec![a, b]);

        // The same through the selection, and through Convert to Multi-State Object.
        s.execute("selection.set", &json!({"ids": [a.0, b.0], "content": true})).unwrap();
        assert!(group_enabled(&s));
        let inner2 = ItemId(s.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap());
        assert_eq!(children(&s, inner), vec![inner2]);
        let st = ItemId(s.execute("states.create", &json!({"ids": [a.0, b.0]})).unwrap()["id"].as_u64().unwrap());
        assert_eq!(children(&s, inner2), vec![st]);
        assert_eq!(s.doc().unwrap().doc.item(st).unwrap().states.len(), 2);
    }

    #[test]
    fn grouping_siblings_inside_a_group_nests_them_and_undo_restores() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let a = frame(&mut s, [0.0, 0.0, 10.0, 10.0], 0);
        let b = frame(&mut s, [20.0, 0.0, 30.0, 10.0], 0);
        let c = frame(&mut s, [40.0, 0.0, 50.0, 10.0], 0);
        let g = group_ids(&mut s, &[a, b, c]).unwrap();
        s.execute("transform.rotate", &json!({"ids": [g.0], "angle": 30})).unwrap();
        let before = (world(&s, a), world(&s, c));

        let inner = group_ids(&mut s, &[c, a]).unwrap();
        assert_eq!(children(&s, g), vec![b, inner], "the new group takes the frontmost object's place");
        assert_eq!(children(&s, inner), vec![a, c], "stacking order kept");
        assert_eq!((world(&s, a), world(&s, c)), before, "nothing moves");
        assert_eq!(s.doc().unwrap().selection.items, vec![inner]);

        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(children(&s, g), vec![a, b, c]);
        assert!(s.doc().unwrap().doc.find(inner).is_none());
    }

    #[test]
    fn grouping_across_levels_or_spreads_is_refused_and_changes_nothing() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        let a = frame(&mut s, [0.0, 0.0, 10.0, 10.0], 0);
        let b = frame(&mut s, [20.0, 0.0, 30.0, 10.0], 0);
        let top = frame(&mut s, [40.0, 0.0, 50.0, 10.0], 0);
        let other = frame(&mut s, [0.0, 0.0, 10.0, 10.0], 1);
        let other2 = frame(&mut s, [20.0, 0.0, 30.0, 10.0], 1);
        let g = group_ids(&mut s, &[a, b]).unwrap();
        let doc = s.doc().unwrap().doc.clone();

        for ids in [[a, top], [top, a], [a, other], [top, other], [g, a]] {
            let e = group_ids(&mut s, &ids).unwrap_err().to_string();
            assert!(e.contains("same spread and at the same level"), "{ids:?}: {e}");
            let e = s.execute("states.create", &json!({"ids": [ids[0].0, ids[1].0]})).unwrap_err().to_string();
            assert!(e.contains("same spread and at the same level"), "states.create {ids:?}: {e}");
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &doc), "{ids:?} changed the document");
        }
        assert!(group_ids(&mut s, &[a, ItemId(999_999)]).is_err());

        s.execute("selection.set", &json!({"ids": [a.0, other.0], "content": true})).unwrap();
        assert!(!group_enabled(&s), "Group is disabled for objects on different spreads");
        s.execute("selection.set", &json!({"ids": [other.0, other2.0]})).unwrap();
        assert!(group_enabled(&s));
        // Named objects win over a selection that can't be grouped.
        s.execute("selection.set", &json!({"ids": [a.0, other.0], "content": true})).unwrap();
        assert!(group_ids(&mut s, &[other, other2]).is_ok());

        // A multi-state object keeps one state per object.
        let ms = ItemId(s.execute("states.create", &json!({"ids": [g.0]})).unwrap()["id"].as_u64().unwrap());
        assert_eq!(ms, g);
        let e = group_ids(&mut s, &[a, b]).unwrap_err().to_string();
        assert!(e.contains("multi-state"), "{e}");
        assert_eq!(children(&s, g), vec![a, b]);
    }
}

#[cfg(test)]
mod stroke_type_tests {
    use serde_json::json;

    #[test]
    fn malformed_stroke_types_return_bad_params_without_editing() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 100, 100]})).unwrap()["id"].clone();
        for (kind, hint) in [
            (json!({"kind": "dashed", "dash": [8, 2.5]}), "dash"),
            (json!({"kind": "zigzag"}), "zigzag"),
            (json!({"kind": "solid", "dash": [8, 2.5]}), "dash"),
            (json!({"kind": "dashed", "pattern": [8, 2.5], "dash": [1, 2]}), "dash"),
            (json!({"kind": "dashed"}), "pattern"),
            (json!({"kind": "dashed", "pattern": "bad"}), "type"),
            (json!({"kind": "stripes", "bands": [[0, "bad"]]}), "type"),
            (json!({"kind": "style"}), "name"),
            (json!({}), "kind"),
            (json!("solid"), "type"),
            (json!(null), "type"),
        ] {
            let before = s.doc().unwrap().doc.clone();
            let error = s.execute("object.stroke", &json!({"ids": [id], "weight": 5, "type": kind})).unwrap_err();
            assert!(matches!(error, crate::EngineError::BadParams { .. }), "{error}");
            assert!(error.to_string().contains(hint), "{kind}: {error}");
            assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc), "{kind}: nothing changed");
        }
    }

    #[test]
    fn stroke_types_apply_all_supported_variants() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 100, 100]})).unwrap()["id"].as_u64().unwrap();
        for kind in [
            json!({"kind": "solid"}),
            json!({"kind": "dashed", "pattern": [8, 2.5]}),
            json!({"kind": "dotted"}),
            json!({"kind": "thickThin"}),
            json!({"kind": "thinThick"}),
            json!({"kind": "thinThin"}),
            json!({"kind": "thickThick"}),
            json!({"kind": "thinThickThin"}),
            json!({"kind": "thickThinThick"}),
            json!({"kind": "wavy"}),
            json!({"kind": "hashed"}),
            json!({"kind": "stripes", "bands": [[0, 0.3], [0.7, 0.3]]}),
            json!({"kind": "style", "name": "Rails"}),
        ] {
            let result = s.execute("object.stroke", &json!({"ids": [id], "type": kind})).unwrap();
            assert_eq!(result["changed"], 1);
            let item = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap();
            assert_eq!(item.stroke.kind, serde_json::from_value::<designcraft_doc::StrokeType>(kind).unwrap());
        }
    }
}

#[cfg(test)]
mod swatch_name_tests {
    use serde_json::json;

    #[test]
    fn fills_and_strokes_must_name_a_swatch_and_change_nothing_otherwise() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 100, 100]})).unwrap()["id"].clone();
        for (cmd, params, hint) in [
            ("object.fill", json!({"swatch": "Paper", "ids": [id]}), Some("[Paper]")),
            ("object.fill", json!({"swatch": "[black]", "ids": [id]}), Some("[Black]")),
            ("object.fill", json!({"swatch": "Missing Ink", "ids": [id]}), None),
            ("object.stroke", json!({"swatch": "#ff0000", "weight": 4, "ids": [id]}), None),
            ("object.stroke", json!({"gapSwatch": "paper", "ids": [id]}), Some("[Paper]")),
        ] {
            let before = s.doc().unwrap().doc.clone();
            let e = s.execute(cmd, &params).unwrap_err().to_string();
            assert!(e.contains("no swatch named") && e.contains("swatch.create"), "{cmd}: {e}");
            if let Some(h) = hint {
                assert!(e.contains(&format!("did you mean `{h}`")), "{cmd}: {e}");
            }
            assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc), "{cmd}: nothing changed");
        }
        // Real names, [None] included, still work.
        for sw in ["[Paper]", "[None]", "C=100 M=0 Y=0 K=0"] {
            s.execute("object.fill", &json!({"swatch": sw, "ids": [id]})).unwrap();
            s.execute("object.stroke", &json!({"swatch": sw, "ids": [id]})).unwrap();
        }
    }
}

#[cfg(test)]
mod copy_ids_tests {
    use serde_json::json;

    #[test]
    fn copies_report_their_ids() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        let ids = |r: &serde_json::Value| r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect::<Vec<_>>();
        let selected = |s: &crate::Session| s.doc().unwrap().selection.items.iter().map(|i| i.0).collect::<Vec<_>>();
        // Moving reports what moved; copying reports the copies, which are selected.
        assert_eq!(ids(&s.execute("transform.move", &json!({"dx": 5, "dy": 0, "ids": [a]})).unwrap()), [a]);
        assert_eq!(ids(&s.execute("transform.move", &json!({"dx": 5, "dy": 0, "ids": [a], "content": true})).unwrap()), [a]);
        let r = s.execute("transform.move", &json!({"dx": 100, "dy": 0, "copy": true, "ids": [a]})).unwrap();
        let copy = ids(&r);
        assert!(copy.len() == 1 && copy[0] != a && s.doc().unwrap().doc.item(designcraft_doc::ItemId(copy[0])).is_some(), "{r}");
        assert_eq!(selected(&s), copy);
        // Edit › Duplicate.
        let r = s.execute("edit.duplicate", &json!({})).unwrap();
        let dup = ids(&r);
        assert!(dup.len() == 1 && dup[0] != copy[0], "{r}");
        assert_eq!(selected(&s), dup);
        // Step and Repeat: every copy.
        let r = s.execute("edit.stepAndRepeat", &json!({"count": 3, "dx": 0, "dy": 60, "ids": [a]})).unwrap();
        assert_eq!((r["created"].as_u64(), ids(&r).len()), (Some(3), 3), "{r}");
        assert!(ids(&r).iter().all(|i| s.doc().unwrap().doc.item(designcraft_doc::ItemId(*i)).is_some()));
    }
}

#[cfg(test)]
mod tint_alone_tests {
    use serde_json::json;

    #[test]
    fn a_tint_alone_keeps_each_swatch_and_leaves_no_stroke_alone() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [60, 0, 110, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [a]})).unwrap();
        s.execute("object.fill", &json!({"swatch": "C=100 M=0 Y=0 K=0", "ids": [b]})).unwrap();
        s.execute("object.stroke", &json!({"swatch": "[None]", "ids": [a, b]})).unwrap();
        // Fill tint on both: each keeps its swatch.
        s.execute("object.fill", &json!({"tint": 0.25, "ids": [a, b]})).unwrap();
        let item = |s: &crate::Session, id: u64| s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().clone();
        assert_eq!((item(&s, a).fill.swatch.as_str(), item(&s, a).fill.tint), ("[Black]", 0.25));
        assert_eq!((item(&s, b).fill.swatch.as_str(), item(&s, b).fill.tint), ("C=100 M=0 Y=0 K=0", 0.25));
        // A swatch alone is 100%.
        s.execute("object.fill", &json!({"swatch": "[Paper]", "ids": [a]})).unwrap();
        assert_eq!(item(&s, a).fill.tint, 1.0);
        // A stroke tint on no stroke: still no stroke.
        s.execute("object.stroke", &json!({"tint": 0.5, "ids": [a]})).unwrap();
        assert!(item(&s, a).stroke.is_none(), "{:?}", item(&s, a).stroke);
        assert!(s.execute("object.fill", &json!({"ids": [a]})).is_err(), "neither swatch nor tint");
    }
}

#[cfg(test)]
mod content_transform_tests {
    use serde_json::json;

    use crate::Session;

    /// A rotated graphic frame holding a 200 x 100 image, its content selected.
    fn rotated_frame_with_image() -> (Session, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        let png = designcraft_render::Rendered { width: 200, height: 100, pixels: vec![200u8; 200 * 100 * 4] }.to_png();
        s.execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&png), "name": "a.png", "frame": f})).unwrap();
        s.execute("transform.rotate", &json!({"ids": [f], "angle": -45})).unwrap();
        s.execute("selection.set", &json!({"ids": [f], "content": true})).unwrap();
        (s, f)
    }

    #[test]
    fn content_bounds_resize_the_graphic_and_leave_the_frame() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        let png = designcraft_render::Rendered { width: 200, height: 100, pixels: vec![200u8; 200 * 100 * 4] }.to_png();
        s.execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&png), "name": "a.png", "frame": f})).unwrap();
        let bounds = |s: &Session| {
            let it = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().clone();
            let g = it.graphic().unwrap().clone();
            (it.bounds(), (it.xf * g.xf).transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1)))
        };
        let (frame0, _) = bounds(&s);
        // The handles' drag: the image to (20, 40)–(420, 240), one undo step; the frame stays.
        s.execute("transform.set", &json!({"ids": [f], "content": true, "bounds": [20, 40, 420, 240]})).unwrap();
        let (frame1, g1) = bounds(&s);
        assert_eq!(frame1, frame0);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(near(g1.x0, 20.0) && near(g1.y0, 40.0) && near(g1.x1, 420.0) && near(g1.y1, 240.0), "{g1:?}");
        // Dragged past the opposite edge: normalized, never empty.
        s.execute("transform.set", &json!({"ids": [f], "content": true, "bounds": [300, 300, 300, 100]})).unwrap();
        let (_, g2) = bounds(&s);
        assert!(g2.width() > 0.0 && near(g2.y0, 100.0) && near(g2.y1, 300.0), "{g2:?}");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(near(bounds(&s).1.x1, 420.0));
    }

    #[test]
    fn rotating_the_content_leaves_the_frame_as_it_is() {
        let (mut s, f) = rotated_frame_with_image();
        let before = s.execute("transform.info", &json!({"ids": [f]})).unwrap();
        assert!((before["rotation"].as_f64().unwrap() + 45.0).abs() < 1e-6 && (before["content"]["rotation"].as_f64().unwrap() + 45.0).abs() < 1e-6);
        // The tutorial's step: the image straightened inside its tilted frame.
        s.execute("transform.set", &json!({"rotation": 0})).unwrap();
        let after = s.execute("transform.info", &json!({"ids": [f]})).unwrap();
        assert!((after["rotation"].as_f64().unwrap() + 45.0).abs() < 1e-6, "the frame stays at -45: {after}");
        assert!(after["content"]["rotation"].as_f64().unwrap().abs() < 1e-6, "the image is upright: {after}");
        // Scale and position act on the image too.
        s.execute("transform.set", &json!({"scaleX": 50, "scaleY": 50})).unwrap();
        let info = s.execute("transform.info", &json!({"ids": [f]})).unwrap();
        assert!((info["content"]["scaleX"].as_f64().unwrap() - 50.0).abs() < 1e-6, "{info}");
        assert!((info["rotation"].as_f64().unwrap() + 45.0).abs() < 1e-6);
        // One undo step each.
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        let undone = s.execute("transform.info", &json!({"ids": [f]})).unwrap();
        assert!((undone["content"]["rotation"].as_f64().unwrap() + 45.0).abs() < 1e-6, "{undone}");
    }

    #[test]
    fn with_the_graphic_selected_transform_info_reports_the_graphic() {
        let (mut s, f) = rotated_frame_with_image();
        s.execute("transform.set", &json!({"rotation": 0})).unwrap();
        // What the Control bar and Properties read: the graphic's 0, not the frame's -45, so a
        // spinner step from it gives 1, not -44.
        let info = s.execute("transform.info", &json!({})).unwrap();
        assert!(info["rotation"].as_f64().unwrap().abs() < 1e-6, "{info}");
        let rot = info["rotation"].as_f64().unwrap() + 1.0;
        s.execute("transform.set", &json!({"rotation": rot})).unwrap();
        let after = s.execute("transform.info", &json!({"ids": [f]})).unwrap();
        assert!((after["content"]["rotation"].as_f64().unwrap() - 1.0).abs() < 1e-6, "{after}");
        assert!((after["rotation"].as_f64().unwrap() + 45.0).abs() < 1e-6, "the frame stays: {after}");
        // `id` names the target explicitly, like `ids`: the frame.
        let by_id = s.execute("transform.info", &json!({"id": f})).unwrap();
        assert!((by_id["rotation"].as_f64().unwrap() + 45.0).abs() < 1e-6, "{by_id}");
    }

    #[test]
    fn the_graphic_moves_to_x_and_y_from_the_rulers_zero_point() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        let png = designcraft_render::Rendered { width: 200, height: 100, pixels: vec![200u8; 200 * 100 * 4] }.to_png();
        s.execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&png), "name": "a.png", "frame": f})).unwrap();
        s.execute("view.zeroPoint", &json!({"at": [72, 36]})).unwrap();
        s.execute("selection.set", &json!({"ids": [f], "content": true})).unwrap();
        s.execute("transform.set", &json!({"x": 10, "y": 20, "ref": 0})).unwrap();
        // The graphic's top-left, measured from the zero point (what the Transform fields show).
        let d = &s.doc().unwrap().doc;
        let id = designcraft_doc::ItemId(f);
        let loc = d.find(id).unwrap();
        let it = d.item(id).unwrap();
        let g = it.graphic().unwrap();
        let b = (d.parent_xf(&loc) * it.xf * g.xf).transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1));
        let o = d.ruler_origin(loc.spread, b.center().x).unwrap();
        assert!(o.x > 0.0 && o.y > 0.0, "the zero point moved: {o:?}");
        assert!((b.x0 - o.x - 10.0).abs() < 1e-6 && (b.y0 - o.y - 20.0).abs() < 1e-6, "{b:?} from {o:?}");
        // The frame stays.
        assert!((it.bounds().x0 - 100.0).abs() < 1e-6, "{:?}", it.bounds());
    }

    #[test]
    fn a_locked_frame_records_no_undo_step() {
        let (mut s, f) = rotated_frame_with_image();
        s.execute("object.lock", &json!({"ids": [f]})).unwrap();
        s.execute("selection.set", &json!({"ids": [f], "content": true})).unwrap();
        let undo = s.doc().unwrap().history.undo.len();
        assert_eq!(s.execute("transform.set", &json!({"rotation": 0})).unwrap()["changed"], 0);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo, "no empty undo step");
    }

    #[test]
    fn a_frame_selection_still_transforms_the_frame_and_an_empty_frame_has_no_content() {
        let (mut s, f) = rotated_frame_with_image();
        // `content: false` (or a frame selection) turns frame and image together.
        s.execute("transform.set", &json!({"rotation": 0, "content": false})).unwrap();
        let info = s.execute("transform.info", &json!({"ids": [f]})).unwrap();
        assert!(info["rotation"].as_f64().unwrap().abs() < 1e-6, "{info}");
        let e = s.execute("frame.create", &json!({"rect": [400, 100, 500, 200], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        assert!(s.execute("transform.set", &json!({"ids": [e], "content": true, "rotation": 10})).is_err());
    }
}

#[cfg(test)]
mod fit_text_frame_tests {
    use super::*;

    fn frame(s: &mut Session, rect: [f64; 4], text: &str) -> ItemId {
        let id = ItemId(s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": text})).unwrap()["id"].as_u64().unwrap());
        s.execute("selection.set", &json!({"ids": [id.0]})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 30}})).unwrap();
        id
    }

    fn bounds(s: &Session, id: ItemId) -> Rect {
        s.doc().unwrap().doc.item(id).unwrap().bounds()
    }

    #[test]
    fn fit_frame_to_content_fits_a_text_frame_to_its_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = frame(&mut s, [100.0, 100.0, 500.0, 300.0], "INDESIGN");
        // The height only (a top or bottom handle): the width stays.
        s.execute("object.fit", &json!({"mode": "fitFrameToContent", "axis": "height", "ids": [id.0]})).unwrap();
        let h = bounds(&s, id);
        assert!(h.x1 == 500.0 && h.y0 == 100.0 && h.height() > 30.0 && h.height() < 45.0, "{h:?}");
        // Both (a corner handle, or the menu): around the line.
        s.execute("object.fit", &json!({"mode": "fitFrameToContent", "ids": [id.0]})).unwrap();
        let b = bounds(&s, id);
        assert!(b.x0 == 100.0 && b.width() > 100.0 && b.width() < 250.0, "{b:?}");
        let d = s.doc().unwrap().doc.clone();
        let sid = d.item(id).unwrap().text_frame().unwrap().story;
        let cs = s.cache.get(&d, sid, None);
        assert!(!cs.is_overset(), "still fits");
        // The bottom is on the baseline, so a frame scaled up with its text still fits.
        let baseline = cs.frame(id).unwrap().lines[0].baseline;
        assert!((b.y1 - baseline).abs() < 1e-9, "{b:?} {baseline}");
    }

    #[test]
    fn fit_frame_to_content_grows_a_frame_with_overset_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = frame(&mut s, [100.0, 100.0, 300.0, 140.0], "one two three four five six seven eight nine ten eleven twelve");
        let d = s.doc().unwrap().doc.clone();
        let sid = d.item(id).unwrap().text_frame().unwrap().story;
        assert!(s.cache.get(&d, sid, None).is_overset());
        s.execute("object.fit", &json!({"mode": "fitFrameToContent", "axis": "height", "ids": [id.0]})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(!s.cache.get(&d, sid, None).is_overset(), "{:?}", bounds(&s, id));
        assert!(bounds(&s, id).height() > 100.0);
        assert!(s.execute("object.fit", &json!({"mode": "fitFrameToContent", "axis": "diagonal", "ids": [id.0]})).is_err());
    }

    #[test]
    fn fitting_text_keeps_columns_and_vertical_justification() {
        use designcraft_doc::VerticalJustification::{Bottom, Center, Justify, Top};
        for columns in [1, 2] {
            for justification in [Top, Bottom, Center, Justify] {
                let mut s = Session::new();
                s.execute("file.new", &json!({})).unwrap();
                let id = frame(&mut s, [100.0, 100.0, 500.0, 500.0], "one two three four five six seven eight nine ten eleven twelve");
                let options = &mut Arc::make_mut(&mut s.doc_mut().unwrap().doc).item_mut(id).unwrap().text_frame_mut().unwrap().options;
                options.columns = columns;
                options.vertical_justification = justification;
                s.execute("object.fit", &json!({"mode": "fitFrameToContent", "axis": "height"})).unwrap();
                let d = &s.doc().unwrap().doc;
                let it = d.item(id).unwrap();
                let tf = it.text_frame().unwrap();
                let composed = s.cache.get(d, tf.story, None);
                assert!(!composed.is_overset(), "{columns} {justification:?}");
                assert!(it.bounds().height() < 250.0, "{columns} {justification:?}: {:?}", it.bounds());
                assert_eq!(tf.options.vertical_justification, justification);
                assert_eq!(tf.options.columns, columns);
                if columns == 2 {
                    let ft = composed.frame(id).unwrap();
                    assert!(ft.lines.iter().any(|l| l.column == 1), "both columns stay in use: {ft:?}");
                }
            }
        }
    }

    #[test]
    fn graphic_fitting_obeys_each_axis_and_locked_text_and_graphics_stay_put() {
        for graphic in [false, true] {
            for lock_layer in [false, true] {
                let mut s = Session::new();
                s.execute("file.new", &json!({})).unwrap();
                let id = frame(&mut s, [100.0, 100.0, 500.0, 300.0], "Fit me");
                if graphic {
                    let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
                    s.execute("file.place", &json!({"base64": super::super::file::base64_encode(&png), "name": "test.png", "frame": id.0})).unwrap();
                    let it = Arc::make_mut(&mut s.doc_mut().unwrap().doc).item_mut(id).unwrap();
                    let Content::Graphic(g) = &mut it.content else { panic!("graphic") };
                    g.xf = Affine::translate((80.0, 90.0)) * Affine::scale(3.0);
                    let original = it.inner_bounds();
                    s.execute("object.fit", &json!({"mode": "fitFrameToContent", "axis": "width", "ids": [id.0]})).unwrap();
                    let fitted = s.doc().unwrap().doc.item(id).unwrap().inner_bounds();
                    assert_eq!(fitted, Rect::new(80.0, original.y0, 200.0, original.y1));
                    Arc::make_mut(&mut s.doc_mut().unwrap().doc).item_mut(id).unwrap().path = shapes::rectangle(original);
                    s.execute("object.fit", &json!({"mode": "fitFrameToContent", "axis": "height", "ids": [id.0]})).unwrap();
                    assert_eq!(s.doc().unwrap().doc.item(id).unwrap().inner_bounds(), Rect::new(original.x0, 90.0, original.x1, 150.0));
                    Arc::make_mut(&mut s.doc_mut().unwrap().doc).item_mut(id).unwrap().path = shapes::rectangle(original);
                }
                let before = bounds(&s, id);
                if lock_layer {
                    let layer = s.doc().unwrap().doc.item(id).unwrap().layer;
                    Arc::make_mut(&mut s.doc_mut().unwrap().doc).layer_mut(layer).unwrap().locked = true;
                } else {
                    Arc::make_mut(&mut s.doc_mut().unwrap().doc).item_mut(id).unwrap().locked = true;
                }
                s.execute("object.fit", &json!({"mode": "fitFrameToContent", "ids": [id.0]})).unwrap();
                assert_eq!(bounds(&s, id), before, "graphic={graphic}, layer={lock_layer}");
            }
        }
    }

    #[test]
    fn an_unfittable_frame_returns_an_error_without_changing_any_target() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = frame(&mut s, [100.0, 100.0, 500.0, 300.0], "Fits");
        let b = frame(&mut s, [100.0, 400.0, 500.0, 600.0], "Cannot fit");
        Arc::make_mut(&mut s.doc_mut().unwrap().doc).item_mut(b).unwrap().text_frame_mut().unwrap().options.first_baseline_min = 30_000.0;
        let before = [bounds(&s, a), bounds(&s, b)];
        assert!(s.execute("object.fit", &json!({"mode": "fitFrameToContent", "ids": [a.0, b.0]})).is_err());
        assert_eq!([bounds(&s, a), bounds(&s, b)], before);
    }
}

#[cfg(test)]
mod spread_target_tests {
    use super::*;

    #[test]
    fn creating_on_a_parent_spread_and_rejecting_unknown_spreads() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        let parent = json!({"kind": "parent", "index": 0});
        let r = s.execute("frame.create", &json!({"spread": parent, "rect": [0, 0, 100, 100]})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        assert_eq!(s.doc().unwrap().doc.find(id).unwrap().spread, SpreadRef::Parent(0));
        s.execute("line.create", &json!({"spread": parent, "a": [0, 0], "b": [50, 50]})).unwrap();
        assert_eq!(s.doc().unwrap().doc.parents[0].items.len(), 2);

        // Each of these used to land on the first body page.
        for bad in
            [json!("A-Parent"), json!("0"), json!(-1), json!(1.5), json!({"kind": "master", "index": 0}), json!({"kind": "parent"}), json!([0])]
        {
            assert!(s.execute("frame.create", &json!({"spread": bad, "rect": [0, 0, 100, 100]})).is_err(), "frame.create spread {bad}");
            assert!(s.execute("line.create", &json!({"spread": bad, "a": [0, 0], "b": [50, 50]})).is_err(), "line.create spread {bad}");
        }
        assert!(s.doc().unwrap().doc.spreads[0].items.is_empty());
        // Null is absent: spread 0 (Paste in Place forwards its optional `spread`).
        let r = s.execute("frame.create", &json!({"spread": null, "rect": [0, 0, 100, 100]})).unwrap();
        assert_eq!(s.doc().unwrap().doc.find(ItemId(r["id"].as_u64().unwrap())).unwrap().spread, SpreadRef::Doc(0));
        s.execute("frame.create", &json!({"rect": [0, 0, 100, 100]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        s.execute("edit.pasteInPlace", &json!({})).unwrap();
    }
}

#[cfg(test)]
mod path_edit_tests {
    use designcraft_doc::ItemId;
    use designcraft_geom::Point;
    use serde_json::json;

    fn session_with_path() -> (crate::Session, u64) {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s
            .execute(
                "path.create",
                &json!({"anchors": [{"p": [100, 100]}, {"p": [200, 100], "in": [170, 80], "out": [230, 120]}, {"p": [300, 200]}]}),
            )
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        (s, id)
    }

    fn anchors(s: &crate::Session, id: u64) -> Vec<designcraft_geom::Anchor> {
        let d = &s.doc().unwrap().doc;
        let it = d.item(ItemId(id)).unwrap();
        let xf = d.parent_xf(&d.find(ItemId(id)).unwrap()) * it.xf;
        it.path.subpaths[0].anchors.iter().map(|a| a.transform(xf)).collect()
    }

    #[test]
    fn reverse_path_swaps_ends_and_direction_lines() {
        let (mut s, id) = session_with_path();
        s.execute("path.reverse", &json!({"ids": [id]})).unwrap();
        let a = anchors(&s, id);
        assert_eq!(a.iter().map(|a| a.p).collect::<Vec<_>>(), [Point::new(300.0, 200.0), Point::new(200.0, 100.0), Point::new(100.0, 100.0)]);
        assert_eq!((a[1].h_in, a[1].h_out), (Point::new(230.0, 120.0), Point::new(170.0, 80.0)));
        assert!(s.execute("path.reverse", &json!({"ids": [id], "subpath": 3})).is_err());
    }

    #[test]
    fn anchors_go_on_the_named_open_subpath_and_set_anchor_reshapes_one() {
        let (mut s, id) = session_with_path();
        s.execute("path.appendAnchor", &json!({"id": id, "subpath": 0, "anchor": {"p": [400, 250]}})).unwrap();
        assert_eq!(anchors(&s, id).last().unwrap().p, Point::new(400.0, 250.0));
        assert!(s.execute("path.appendAnchor", &json!({"id": id, "subpath": 1, "anchor": {"p": [0, 0]}})).is_err());
        // `at: start` goes before the first anchor (the Pen continuing from a path's start).
        s.execute("path.appendAnchor", &json!({"id": id, "anchor": {"p": [50, 60]}, "at": "start"})).unwrap();
        assert_eq!((anchors(&s, id)[0].p, anchors(&s, id).len()), (Point::new(50.0, 60.0), 5));
        assert!(s.execute("path.appendAnchor", &json!({"id": id, "anchor": {"p": [0, 0]}, "at": "middle"})).is_err());
        // Direction lines on the last anchor: collinear and opposite make it smooth.
        s.execute("path.setAnchor", &json!({"id": id, "anchor": "last", "in": [380, 230], "out": [420, 270]})).unwrap();
        let last = *anchors(&s, id).last().unwrap();
        assert_eq!((last.h_in, last.h_out, last.kind), (Point::new(380.0, 230.0), Point::new(420.0, 270.0), designcraft_geom::AnchorKind::Smooth));
        s.execute("path.setAnchor", &json!({"id": id, "anchor": 0, "p": [90, 110]})).unwrap();
        assert_eq!(anchors(&s, id)[0].p, Point::new(90.0, 110.0));
        assert!(s.execute("path.setAnchor", &json!({"id": id, "anchor": 9, "p": [0, 0]})).is_err());
        assert!(s.execute("path.setAnchor", &json!({"id": id, "anchor": "last"})).is_err(), "nothing to set");
        // Closing one subpath; then it takes no more anchors.
        s.execute("path.close", &json!({"id": id, "subpath": 0})).unwrap();
        assert!(s.execute("path.appendAnchor", &json!({"id": id, "anchor": {"p": [0, 0]}})).is_err());
        assert!(s.execute("path.close", &json!({"id": id, "subpath": 4})).is_err());
    }

    /// Every edit that `path_editable` guards, aimed at the path `id` of `session_with_path`.
    fn path_edits(id: u64) -> Vec<(&'static str, serde_json::Value)> {
        vec![
            ("path.appendAnchor", json!({"id": id, "anchor": {"p": [400, 250]}})),
            ("path.setAnchor", json!({"id": id, "anchor": 0, "p": [90, 110]})),
            ("path.close", json!({"id": id})),
            ("path.moveAnchors", json!({"id": id, "anchors": [[0, 0]], "dx": 10, "dy": 10})),
            ("path.reverse", json!({"ids": [id]})),
            ("path.addAnchor", json!({"id": id, "at": [150, 100]})),
            ("path.deleteAnchor", json!({"id": id, "subpath": 0, "anchor": 0})),
            ("path.convertAnchor", json!({"id": id, "subpath": 0, "anchor": 0})),
            ("path.erase", json!({"id": id, "points": [[120, 100], [180, 100]]})),
            ("path.smooth", json!({"id": id, "points": [[120, 100], [180, 100]]})),
            ("path.split", json!({"id": id, "at": [150, 100]})),
            ("path.join", json!({"ids": [id]})),
            ("path.convertPoints", json!({"ids": [id], "kind": "corner"})),
        ]
    }

    fn assert_no_path_edit(s: &mut crate::Session, id: u64, why: &str) {
        let before = anchors(s, id);
        for (cmd, p) in path_edits(id) {
            assert!(s.execute(cmd, &p).is_err(), "{cmd} {why}");
        }
        assert_eq!(anchors(s, id), before, "{why}");
        assert!(!s.doc().unwrap().doc.item(ItemId(id)).unwrap().path.subpaths[0].closed, "{why}");
    }

    #[test]
    fn path_edits_leave_a_path_on_a_locked_layer_alone() {
        let (mut s, id) = session_with_path();
        let layer = s.doc().unwrap().doc.item(ItemId(id)).unwrap().layer;
        s.execute("layer.set", &json!({"id": layer.0, "locked": true})).unwrap();
        assert_no_path_edit(&mut s, id, "on a locked layer");
    }

    #[test]
    fn joining_two_paths_with_one_on_a_locked_layer_changes_neither() {
        let (mut s, a) = session_with_path();
        let b = s.execute("path.create", &json!({"anchors": [{"p": [300, 200]}, {"p": [400, 300]}]})).unwrap()["id"].as_u64().unwrap();
        let layer = s.execute("layer.new", &json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.setLayer", &json!({"ids": [b], "layer": layer})).unwrap();
        let a_layer = s.doc().unwrap().doc.item(ItemId(a)).unwrap().layer;
        s.execute("layer.set", &json!({"id": a_layer.0, "locked": true})).unwrap();
        let (before_a, before_b) = (anchors(&s, a), anchors(&s, b));
        for ids in [[a, b], [b, a]] {
            assert!(s.execute("path.join", &json!({"ids": ids})).is_err(), "{ids:?}");
        }
        assert_eq!((anchors(&s, a), anchors(&s, b)), (before_a, before_b));
        for id in [a, b] {
            assert!(!s.doc().unwrap().doc.item(ItemId(id)).unwrap().path.subpaths[0].closed, "{id} stays open");
        }
    }

    #[test]
    fn path_edits_leave_a_path_in_a_locked_group_alone() {
        let (mut s, id) = session_with_path();
        let other = s.execute("frame.create", &json!({"rect": [400, 400, 450, 450]})).unwrap()["id"].as_u64().unwrap();
        let group = s.execute("object.group", &json!({"ids": [id, other]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.lock", &json!({"ids": [group]})).unwrap();
        assert!(!s.doc().unwrap().doc.item(ItemId(id)).unwrap().locked, "only the group is locked");
        assert_no_path_edit(&mut s, id, "in a locked group");
    }
}

#[cfg(test)]
mod effects_command_tests {
    use designcraft_doc::ItemId;
    use serde_json::json;

    #[test]
    fn effects_set_together_in_one_undo_step_and_keep_what_is_not_given() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.dropShadow", &json!({"on": true, "distance": 10, "ids": [id]})).unwrap();
        let undo = s.doc().unwrap().history.undo.len();
        let r = s
            .execute(
                "object.effects",
                &json!({"ids": [id], "opacity": 0.5, "blend": "multiply",
                    "effects": {"innerShadow": {"on": true, "angle": 120, "distance": 7, "size": 7}, "dropShadow": {"size": 9}, "feather": 4}}),
            )
            .unwrap();
        assert_eq!(r["changed"], 1);
        let st = s.doc().unwrap();
        assert_eq!(st.history.undo.len(), undo + 1, "one undo step");
        let it = st.doc.item(ItemId(id)).unwrap();
        let e = &it.effects;
        assert!(e.inner_shadow.on && e.inner_shadow.angle == 120.0 && e.inner_shadow.size == 7.0);
        assert!(e.drop_shadow.on && e.drop_shadow.distance == 10.0 && e.drop_shadow.size == 9.0, "untouched fields stay");
        assert_eq!((e.feather, it.opacity, it.blend), (4.0, 0.5, designcraft_color::BlendMode::Multiply));
        // Out of range values are kept in range; garbage changes nothing.
        s.execute("object.effects", &json!({"ids": [id], "effects": {"outerGlow": {"on": true, "opacity": 3, "spread": 400, "size": -5}}})).unwrap();
        let og = s.doc().unwrap().doc.item(ItemId(id)).unwrap().effects.outer_glow.clone();
        assert_eq!((og.opacity, og.spread, og.size), (1.0, 100.0, 0.0));
        let before = s.doc().unwrap().doc.clone();
        for bad in [json!({"effects": {"dropShadow": {"distance": "far"}}}), json!({"effects": 5}), json!({"blend": "loud"}), json!({"opacity": "x"})]
        {
            let mut p = bad.clone();
            p["ids"] = json!([id]);
            assert!(s.execute("object.effects", &p).is_err(), "{bad}");
        }
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }
}

#[cfg(test)]
mod ungroup_tests {
    use super::*;

    #[test]
    fn ungroup_leaves_frames_with_pasted_in_items_whole() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = |r: Value| ItemId(r["id"].as_u64().unwrap());
        let sq = id(s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap());
        let ell = id(s.execute("frame.create", &json!({"rect": [90, 90, 210, 210], "shape": "ellipse"})).unwrap());
        s.execute("selection.set", &json!({"ids": [sq.0]})).unwrap();
        s.execute("edit.cut", &json!({})).unwrap();
        s.execute("selection.set", &json!({"ids": [ell.0]})).unwrap();
        s.execute("edit.pasteInto", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.item(ell).unwrap().default_label(), "<ellipse>", "named by its shape, not <group>");
        // Ungroup: the frame keeps its content and stays selected.
        s.execute("object.ungroup", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.item(ell).unwrap().has_nested_items());
        assert_eq!(d.spreads[0].items.len(), 1);
        assert_eq!(s.doc().unwrap().selection.items, vec![ell]);
        // A real group with the frame in it still comes apart, the frame whole.
        let other = id(s.execute("frame.create", &json!({"rect": [300, 100, 400, 200]})).unwrap());
        s.execute("selection.set", &json!({"ids": [ell.0, other.0]})).unwrap();
        let g = id(s.execute("object.group", &json!({})).unwrap());
        assert_eq!(s.doc().unwrap().doc.item(g).unwrap().default_label(), "<group>");
        s.execute("object.ungroup", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.item(g).is_none());
        assert!(d.item(ell).unwrap().has_nested_items());
        assert_eq!(s.doc().unwrap().selection.items.len(), 2);
        d.check().unwrap();
    }
}

#[cfg(test)]
mod fixed_width_tests {
    use super::*;

    #[test]
    fn fixed_width_columns_set_the_frame_width() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Hello"})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        let p = json!({"ids": [id.0], "columns": 2, "gutter": 12, "inset": 0, "columnsKind": "fixedWidth", "columnWidth": 120});
        s.execute("object.textFrameOptions", &p).unwrap();
        let b = s.doc().unwrap().doc.item(id).unwrap().inner_bounds();
        assert!((b.width() - (2.0 * 120.0 + 12.0)).abs() < 1e-6, "{b:?}");
        assert!((b.x0 - 36.0).abs() < 1e-6 && (b.height() - 264.0).abs() < 1e-6, "{b:?}");
        // Widths beyond the largest page are refused and change nothing.
        for (k, v) in [("columnWidth", 1e9), ("columnWidth", -1.0), ("firstBaselineMin", 1e9)] {
            assert!(s.execute("object.textFrameOptions", &json!({"ids": [id.0], k: v})).is_err(), "{k} {v}");
        }
        assert!((s.doc().unwrap().doc.item(id).unwrap().inner_bounds().width() - 252.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod glow_feather_options_tests {
    use serde_json::json;

    #[test]
    fn glow_mode_technique_and_feather_options_are_kept() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute(
            "object.effects",
            &json!({"ids": [id], "effects": {"innerGlow": {"on": true, "mode": "Multiply", "technique": "precise"}, "feather": 4.5, "featherOptions": {"choke": 140, "corners": "sharp"}}}),
        )
        .unwrap();
        let e = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().effects.clone();
        assert_eq!((e.inner_glow.mode, e.inner_glow.technique), (designcraft_color::BlendMode::Multiply, designcraft_doc::GlowTechnique::Precise));
        assert_eq!((e.feather_options.choke, e.feather_options.corners), (100.0, designcraft_doc::FeatherCorners::Sharp), "choke is a percentage");
        // InDesign's defaults: Screen and Softer; Diffused corners, no choke.
        let d = designcraft_doc::Effects::default();
        assert_eq!((d.inner_glow.mode, d.inner_glow.technique), (designcraft_color::BlendMode::Screen, designcraft_doc::GlowTechnique::Softer));
        assert_eq!((d.feather_options.choke, d.feather_options.corners), (0.0, designcraft_doc::FeatherCorners::Diffused));
    }
}

#[cfg(test)]
mod fill_tint_tests {
    use super::*;

    #[test]
    fn a_tint_alone_keeps_each_objects_own_swatch() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [36, 36, 100, 100]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [136, 36, 200, 100]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"ids": [a], "swatch": "C=100 M=0 Y=0 K=0"})).unwrap();
        s.execute("object.fill", &json!({"ids": [b], "swatch": "C=0 M=100 Y=0 K=0"})).unwrap();
        s.execute("object.fill", &json!({"ids": [a, b], "tint": 0.4})).unwrap();
        let fill_of = |s: &Session, id: u64| s.doc().unwrap().doc.item(ItemId(id)).unwrap().fill.clone();
        assert_eq!((fill_of(&s, a).swatch.as_str(), fill_of(&s, a).tint), ("C=100 M=0 Y=0 K=0", 0.4));
        assert_eq!((fill_of(&s, b).swatch.as_str(), fill_of(&s, b).tint), ("C=0 M=100 Y=0 K=0", 0.4));
        // A swatch without a tint is at 100 %, as before.
        s.execute("object.fill", &json!({"ids": [a], "swatch": "[Black]"})).unwrap();
        assert_eq!(fill_of(&s, a).tint, 1.0);
        // Neither, or a tint out of range: refused.
        assert!(s.execute("object.fill", &json!({"ids": [a]})).is_err());
        assert!(s.execute("object.fill", &json!({"ids": [a], "tint": 2})).is_err());
        assert!(s.execute("object.fill", &json!({"ids": [a], "tint": "x"})).is_err());
        // A stroke tint alone: stroked objects take it, an object without a stroke keeps none.
        s.execute("object.stroke", &json!({"ids": [a], "swatch": "C=100 M=0 Y=0 K=0", "weight": 2})).unwrap();
        s.execute("object.stroke", &json!({"ids": [b], "swatch": "[None]"})).unwrap();
        s.execute("object.stroke", &json!({"ids": [a, b], "tint": 0.5})).unwrap();
        let stroke_of = |s: &Session, id: u64| s.doc().unwrap().doc.item(ItemId(id)).unwrap().stroke.clone();
        assert_eq!((stroke_of(&s, a).swatch.as_str(), stroke_of(&s, a).tint, stroke_of(&s, a).weight), ("C=100 M=0 Y=0 K=0", 0.5, 2.0));
        assert!(stroke_of(&s, b).is_none());
    }
}
