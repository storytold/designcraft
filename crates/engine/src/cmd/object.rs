//! Object creation and the Object menu: frames, lines, transforms, arrange, group, lock/hide,
//! fill/stroke, content type, Text Frame Options, corners, fitting, text wrap.

use std::collections::HashMap;
use std::sync::Arc;

use designcraft_color::BlendMode;
use designcraft_doc::{
    Content, Document, Fill, Item, ItemId, ParaFormat, Selection, Shape, SpreadRef, Story, StoryId, Stroke, TextFrame, TextFrameOptions, TextSel,
};
use designcraft_geom::{Affine, Point, Rect, Vec2, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, f64_or, has_doc, has_selection, ok, point_param, rect_param, spread_param, str_param, targets};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "frame.create",
            "Create Frame",
            [],
            None,
            "{spread?, rect: [x0,y0,x1,y1] (spread coords), shape?: rectangle|ellipse|polygon, content?: graphic|text|unassigned, sides?: 6, text?: string, caret?: bool, vertical?: bool (text: a new vertical story)}",
            has_doc,
            frame_create
        ),
        cmd!("line.create", "Create Line", [], None, "{spread?, a: [x,y], b: [x,y]}", has_doc, line_create),
        cmd!("transform.move", "Move", ["Object", "Transform"], None, "{dx, dy, copy?: bool, ids?, toSpread?}", has_selection, transform_move),
        cmd!(
            "transform.resize",
            "Resize",
            [],
            None,
            "{from: rect, to: rect, content?: bool (scale content), distribute?: bool (Live Distribute: objects keep their size, their centres spread with the bounds), ids?}",
            has_selection,
            transform_resize
        ),
        cmd!("transform.rotate", "Rotate", ["Object", "Transform"], None, "{angle (degrees, CCW), ids?}", has_selection, transform_rotate),
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
                    if let Some(it) = d.item_mut(*id) {
                        scale_item(it, m, (sx * sy).abs().sqrt(), strokes);
                    }
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
        cmd!("transform.shear", "Shear", ["Object", "Transform"], None, "{angle (degrees), ids?}", has_selection, |s, p| {
            let a = f64_or(p, "angle", 0.0).clamp(-85.0, 85.0).to_radians().tan();
            apply_about_center(s, p, Affine::new([1.0, 0.0, a, 1.0, 0.0, 0.0]))
        }),
        cmd!(
            "object.matchAttributes",
            "Apply Attributes (Eyedropper)",
            [],
            None,
            "{from: id, ids?} — copy fill, stroke, corners, opacity, effects, wrap",
            has_selection,
            |s, p| {
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
        ),
        cmd!("transform.flip", "Flip", ["Object", "Transform"], None, "{axis: horizontal|vertical, ids?}", has_selection, |s, p| {
            let h = str_param(p, "axis") != Some("vertical");
            apply_about_center(s, p, if h { Affine::scale_non_uniform(-1.0, 1.0) } else { Affine::scale_non_uniform(1.0, -1.0) })
        }),
        cmd!(
            "transform.set",
            "Transform Panel",
            [],
            None,
            "{x?, y?, width?, height?, scaleX? (%), scaleY? (%), rotation? (°), shear? (°), ref?: 0..8, ids?} — reference-point based geometry; rotation and shear are absolute (Transformations are Totals decides whether nested objects measure them on the pasteboard)",
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
        cmd!("object.group", "Group", ["Object"], Some("Cmd+G"), "{ids?}", has_selection, group),
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
        cmd!("object.fill", "Fill", [], None, "{swatch, tint?: 0..1, ids?}", has_selection, |s, p| {
            let sw = str_param(p, "swatch").ok_or_else(|| bad("object.fill", "missing swatch"))?.to_string();
            let tint = f64_or(p, "tint", 1.0) as f32;
            set_flag(s, p, move |i| i.fill = Fill { swatch: sw.clone(), tint, ..i.fill.clone() }, false)
        }),
        cmd!(
            "object.stroke",
            "Stroke",
            [],
            None,
            "{swatch?, tint?, weight?, align?: center|inside|outside, type?: {kind:…}, cap?: butt|round|projecting, join?: miter|round|bevel, miterLimit?, start?, end?: none|simple|simpleWide|triangle|triangleWide|barbed|curved|circle|circleSolid|square|squareSolid|bar, gapSwatch?, gapTint?, gapOverprint?: bool, ids?}",
            has_selection,
            |s, p| {
                let p2 = p.clone();
                set_flag(
                    s,
                    p,
                    move |i| {
                        let mut st = if i.stroke.is_none() && p2.get("swatch").is_none() { Stroke::default() } else { i.stroke.clone() };
                        if let Some(v) = p2.get("swatch").and_then(Value::as_str) {
                            st.swatch = v.into();
                        }
                        if let Some(v) = p2.get("tint").and_then(Value::as_f64) {
                            st.tint = v as f32;
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
                            st.gap_tint = v as f32;
                        }
                        if let Some(v) = p2.get("gapOverprint").and_then(Value::as_bool) {
                            st.gap_overprint = v;
                        }
                        for (k, apply) in [("align", 0), ("type", 1), ("cap", 2), ("join", 3), ("start", 4), ("end", 5)] {
                            if let Some(v) = p2.get(k).cloned() {
                                let _ = match apply {
                                    0 => serde_json::from_value(v).map(|x| st.align = x),
                                    1 => serde_json::from_value(v).map(|x| st.kind = x),
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
                set_flag(
                    s,
                    p,
                    move |i| {
                        let ds = &mut i.effects.drop_shadow;
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
                    },
                    false,
                )
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
                set_flag(
                    s,
                    p,
                    move |i| {
                        let e = &mut i.effects.inner_shadow;
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
                    },
                    false,
                )
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
                set_flag(
                    s,
                    p,
                    move |i| {
                        let e = &mut i.effects.outer_glow;
                        e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
                        e.opacity = f64_or(&p2, "opacity", e.opacity as f64) as f32;
                        e.size = f64_or(&p2, "size", e.size);
                        e.spread = f64_or(&p2, "spread", e.spread);
                        if let Some(c) = str_param(&p2, "color") {
                            e.color = c.into();
                        }
                    },
                    false,
                )
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
                set_flag(
                    s,
                    p,
                    move |i| {
                        let e = &mut i.effects.inner_glow;
                        e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
                        e.opacity = f64_or(&p2, "opacity", e.opacity as f64) as f32;
                        e.size = f64_or(&p2, "size", e.size);
                        e.choke = f64_or(&p2, "choke", e.choke);
                        e.center = p2.get("center").and_then(Value::as_bool).unwrap_or(e.center);
                        if let Some(c) = str_param(&p2, "color") {
                            e.color = c.into();
                        }
                    },
                    false,
                )
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
                set_flag(
                    s,
                    p,
                    move |i| {
                        let e = &mut i.effects.bevel;
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
                    },
                    false,
                )
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
                set_flag(
                    s,
                    p,
                    move |i| {
                        let e = &mut i.effects.satin;
                        e.on = p2.get("on").and_then(Value::as_bool).unwrap_or(!e.on);
                        e.opacity = f64_or(&p2, "opacity", e.opacity as f64) as f32;
                        e.angle = f64_or(&p2, "angle", e.angle);
                        e.distance = f64_or(&p2, "distance", e.distance);
                        e.size = f64_or(&p2, "size", e.size);
                        e.invert = p2.get("invert").and_then(Value::as_bool).unwrap_or(e.invert);
                        if let Some(c) = str_param(&p2, "color") {
                            e.color = c.into();
                        }
                    },
                    false,
                )
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
                set_flag(
                    s,
                    p,
                    move |i| {
                        let e = &mut i.effects.directional_feather;
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
                    },
                    false,
                )
            }
        ),
        cmd!("object.feather", "Basic Feather", ["Object", "Effects"], None, "{width (0 = off), ids?}", has_selection, |s, p| {
            let w = f64_or(p, "width", 9.0).max(0.0);
            set_flag(s, p, move |i| i.effects.feather = w, false)
        }),
        cmd!("object.content", "Content", ["Object", "Content"], None, "{type: graphic|text|unassigned, ids?}", has_selection, content_type),
        cmd!(
            "object.textFrameOptions",
            "Text Frame Options…",
            ["Object"],
            Some("Cmd+B"),
            "{columns?, gutter?, inset?: number|[t,l,b,r], verticalJustification?: top|center|bottom|justify, firstBaseline?, autoSize?, ignoreWrap?, balanceColumns?, vertical?: bool (sets the story direction of the frames' stories, as Type ▸ Story Direction), ids?}",
            has_selection,
            text_frame_options
        ),
        cmd!(
            "object.cornerOptions",
            "Corner Options…",
            ["Object"],
            None,
            "{shape: none|rounded|inverseRounded|inset|bevel|fancy, size, ids?}",
            has_selection,
            |s, p| {
                let shape =
                    p.get("shape").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(designcraft_geom::corners::CornerShape::Rounded);
                let size = f64_or(p, "size", 12.0);
                set_flag(s, p, move |i| i.corners = designcraft_geom::corners::CornerOptions::uniform(shape, size), false)
            }
        ),
        cmd!(
            "object.textWrap",
            "Text Wrap",
            ["Window"],
            Some("Cmd+Alt+W"),
            "{mode: none|boundingBox|contour|jumpObject|jumpToNextColumn, offset?: number|[t,l,b,r], invert?: bool, ids?}",
            has_selection,
            |s, p| {
                let mode = p.get("mode").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(designcraft_doc::WrapMode::BoundingBox);
                let off = match p.get("offset") {
                    Some(Value::Number(n)) => Some([n.as_f64().unwrap_or(0.0); 4]),
                    Some(v) => serde_json::from_value(v.clone()).ok(),
                    None => None,
                };
                let invert = p.get("invert").and_then(Value::as_bool);
                set_flag(
                    s,
                    p,
                    move |i| {
                        i.wrap.mode = mode;
                        if let Some(o) = off {
                            i.wrap.offsets = o;
                        }
                        if let Some(v) = invert {
                            i.wrap.invert = v;
                        }
                    },
                    false,
                )
            }
        ),
        cmd!(
            "object.fit",
            "Fitting",
            ["Object", "Fitting"],
            None,
            "{mode: fillProportionally|fitProportionally|fitContentToFrame|centerContent|fitFrameToContent, ids?}",
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
        cmd!("object.rename", "Rename", [], None, "{id, name}", has_doc, |s, p| {
            let name = str_param(p, "name").unwrap_or("").to_string();
            set_flag(s, p, move |i| i.name = name.clone(), false)
        }),
        cmd!(
            "edit.stepAndRepeat",
            "Step and Repeat…",
            ["Edit"],
            Some("Cmd+Alt+U"),
            "{count?: 1, dx?, dy?, rows?, columns?, ids?} — copies of the selection (or ids) offset by (dx, dy); with rows/columns, a grid",
            has_selection,
            step_and_repeat
        ),
        cmd!("path.create", "Create Path", [], None, "{spread?, anchors: [{p:[x,y], in?:[x,y], out?:[x,y]}], closed?: bool}", has_doc, path_create),
        cmd!("path.appendAnchor", "Add Anchor", [], None, "{id, anchor: {p, in?, out?}} (spread coords)", has_doc, path_append),
        cmd!("path.close", "Close Path", ["Object", "Paths"], None, "{id}", has_doc, path_close),
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
            "{at: [x, y] (spread), delta | dx/dy, spread?} — Gap tool: the gap at `at` between objects (or an object and the page edge) moves by `delta`; the objects on both sides resize → {axis, changed}",
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
        cmd!("object.setLayer", "Move to Layer", [], None, "{layer, ids?}", has_selection, |s, p| {
            let l = designcraft_doc::LayerId(p.get("layer").and_then(Value::as_u64).unwrap_or(0));
            set_flag(s, p, move |i| i.layer = l, false)
        }),
    ]
}

fn frame_create(s: &mut Session, p: &Value) -> Result<Value> {
    let rect = rect_param(p, "rect").ok_or_else(|| bad("frame.create", "missing rect"))?;
    let shape = str_param(p, "shape").unwrap_or("rectangle");
    let content = str_param(p, "content").unwrap_or("graphic");
    let sr = spread_param(p, "spread");
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
        } else {
            // Shapes drawn with Rectangle/Ellipse/Polygon get the default 1 pt black stroke.
            it.stroke = Stroke::default();
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
    let sr = spread_param(p, "spread");
    let lid = s.doc()?.active_layer;
    s.edit(|d, sel| {
        let id = ItemId(d.alloc());
        let mut it = Item::new(id, lid, Shape::GraphicLine, shapes::line(a, b));
        it.stroke = Stroke::default();
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

fn renumber(dst: &mut Document, src: &Document, it: &mut Item, stories: &mut HashMap<StoryId, StoryId>) {
    it.id = ItemId(dst.alloc());
    let my_id = it.id;
    match &mut it.content {
        Content::Text(tf) => {
            let new_sid = match stories.get(&tf.story) {
                Some(n) => *n,
                None => {
                    let n = StoryId(dst.alloc());
                    let mut st = src.story(tf.story).cloned().unwrap_or_else(|| Story::new(n));
                    st.id = n;
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
            if let Some(a) = src.assets.get(&g.asset)
                && !dst.assets.contains_key(&g.asset)
            {
                dst.assets.insert(g.asset, a.clone());
            }
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
            let it = d.item_mut(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            if it.locked {
                continue;
            }
            it.xf = Affine::translate((dx, dy)) * it.xf;
        }
        Ok(json!({"moved": ids.len()}))
    })
}

fn transform_resize(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let from = rect_param(p, "from").ok_or_else(|| bad("transform.resize", "missing from"))?;
    let to = rect_param(p, "to").ok_or_else(|| bad("transform.resize", "missing to"))?;
    let scale_content = bool_or(p, "content", false);
    let distribute = bool_or(p, "distribute", false);
    if from.width().abs() < 1e-9 || from.height().abs() < 1e-9 {
        return Err(bad("transform.resize", "degenerate source rect"));
    }
    let sx = to.width() / from.width();
    let sy = to.height() / from.height();
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
                scale_item(it, m, (sx * sy).abs().sqrt(), strokes);
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

fn union_bounds(d: &Document, ids: &[ItemId]) -> Rect {
    ids.iter().filter_map(|i| d.item(*i)).map(Item::bounds).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO)
}

/// Transform an item's geometry in its own space: the path and the gradient vector with it.
pub(crate) fn bake(it: &mut Item, inner: Affine) {
    it.path.transform(inner);
    if let Some([x0, y0, x1, y1]) = it.fill.gradient_vector {
        let (a, b) = (inner * Point::new(x0, y0), inner * Point::new(x1, y1));
        it.fill.gradient_vector = Some([a.x, a.y, b.x, b.y]);
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

fn apply_about_center(s: &mut Session, p: &Value, a: Affine) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, _| {
        let mut b: Option<Rect> = None;
        for id in &ids {
            if let Some(it) = d.item(*id) {
                b = Some(b.map_or(it.bounds(), |r| r.union(it.bounds())));
            }
        }
        let c = b.unwrap_or(Rect::ZERO).center();
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
    apply_about_center(s, p, Affine::rotate(-a.to_radians()))
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

fn transform_info(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let totals = s.prefs.transformations_are_totals;
    let d = &s.doc()?.doc;
    let id = *ids.first().ok_or_else(|| bad("transform.info", "nothing selected"))?;
    let (_, m) = measured_xf(d, id, totals).ok_or_else(|| bad("transform.info", "no such object"))?;
    let mut out = values_json(m);
    if let Some(g) = d.item(id).and_then(Item::graphic) {
        out["content"] = values_json(if totals { m * g.xf } else { g.xf });
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
                scale_item(it, m, (fx * fy).abs().sqrt(), strokes);
            }
        }
        ok()
    })
}

fn transform_set(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
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
        // x/y are the reference point position relative to the page origin of the first page in the spread.
        let page_x = ids
            .first()
            .and_then(|i| d.find(*i))
            .and_then(|l| {
                d.spread(l.spread).map(|sp| {
                    let pi = sp.page_at_x(from.center().x).unwrap_or(0);
                    sp.pages[pi].x
                })
            })
            .unwrap_or(0.0);
        if let Some(x) = p.get("x").and_then(Value::as_f64) {
            to = to + Vec2::new(x + page_x - anchor.x, 0.0);
        }
        if let Some(y) = p.get("y").and_then(Value::as_f64) {
            to = to + Vec2::new(0.0, y - anchor.y);
        }
        let to = Rect::new(to.x0 + pad.0, to.y0 + pad.1, (to.x1 - pad.2).max(to.x0 + pad.0 + 0.01), (to.y1 - pad.3).max(to.y0 + pad.1 + 0.01));
        let m = Affine::translate((to.x0, to.y0))
            * Affine::scale_non_uniform(to.width() / geom.width().max(1e-9), to.height() / geom.height().max(1e-9))
            * Affine::translate((-geom.x0, -geom.y0));
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                if matches!(it.content, Content::Group { .. }) {
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

fn arrange(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let to = str_param(p, "to").unwrap_or("front").to_string();
    s.edit(|d, _| {
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
                "forward" => (i + 1).min(n - 1),
                _ => i.saturating_sub(1),
            };
            sp.items.insert(j, it);
        }
        ok()
    })
}

fn group(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    if ids.len() < 2 {
        return Err(bad("object.group", "select at least two objects"));
    }
    s.edit(|d, sel| {
        let sr = d.find(ids[0]).map(|l| l.spread).ok_or(designcraft_doc::DocError::NoItem(ids[0]))?;
        // Keep stacking order: collect in z-order.
        let order: Vec<ItemId> = d.spread(sr).map(|sp| sp.items.iter().map(|i| i.id).filter(|i| ids.contains(i)).collect()).unwrap_or_default();
        let front = d.spread(sr).and_then(|sp| sp.items.iter().position(|i| Some(&i.id) == order.last())).unwrap_or(0);
        let layer = d.item(order[0]).map(|i| i.layer).unwrap_or_default();
        let mut kids = Vec::new();
        for id in &order {
            kids.push(d.remove_item_keep_story(*id)?);
        }
        let gid = ItemId(d.alloc());
        let mut g = Item::new(gid, layer, Shape::Group, designcraft_geom::PathData::default());
        g.content = Content::Group { items: kids.into_iter().map(Arc::new).collect() };
        let at = front + 1 - order.len();
        d.insert_item(sr, g, Some(at))?;
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
            let Content::Group { items } = &g.content else { continue };
            if loc.path.len() != 1 {
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
    let st = s.doc()?;
    let mut ids = targets(s, p)?;
    if ids.is_empty()
        && let Some(t) = st.selection.text
    {
        ids = st.doc.story(t.story).map(|s| s.frames.clone()).unwrap_or_default();
    }
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
                    if let Ok(a) = serde_json::from_value(v.clone()) {
                        o.inset = a;
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
            if let Some(v) = p.get("ignoreWrap").and_then(Value::as_bool) {
                o.ignore_wrap = v;
            }
            if let Some(v) = p.get("balanceColumns").and_then(Value::as_bool) {
                o.balance_columns = v;
            }
            if let Some(v) = p.get("columnRule").and_then(Value::as_bool) {
                o.column_rule = v;
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
    let sr = spread_param(p, "spread");
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
    let ids = targets(s, p)?;
    let mode = str_param(p, "mode").unwrap_or("fitProportionally").to_string();
    s.edit(|d, _| {
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            let r = it.inner_bounds();
            let Content::Graphic(g) = &mut it.content else { continue };
            let (nw, nh) = g.size;
            let cur = g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, nw, nh));
            if mode == "fitFrameToContent" {
                it.path = shapes::rectangle(cur);
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
    let sr = spread_param(p, "spread");
    let lid = s.doc()?.active_layer;
    s.edit(|d, sel| {
        let id = ItemId(d.alloc());
        let shape = if anchors.len() == 2 && !closed { Shape::GraphicLine } else { Shape::Path };
        let mut it = Item::new(id, lid, shape, designcraft_geom::PathData::single(designcraft_geom::SubPath::new(anchors, closed)));
        it.stroke = Stroke::default();
        d.insert_item(sr, it, None)?;
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })
}

fn path_append(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::id_param(p, "id").ok_or_else(|| bad("path.appendAnchor", "missing id"))?;
    let a = p.get("anchor").and_then(anchor_from).ok_or_else(|| bad("path.appendAnchor", "missing anchor"))?;
    s.edit(|d, _| {
        let loc = d.find(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let inv = (d.parent_xf(&loc) * d.item_at(&loc).map(|i| i.xf).unwrap_or_default()).inverse();
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let sp = it.path.subpaths.last_mut().ok_or_else(|| bad("path.appendAnchor", "empty path"))?;
        sp.anchors.push(a.transform(inv));
        if it.shape == Shape::GraphicLine {
            it.shape = Shape::Path;
        }
        ok()
    })
}

fn path_close(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::id_param(p, "id").ok_or_else(|| bad("path.close", "missing id"))?;
    s.edit(|d, _| {
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        for sp in &mut it.path.subpaths {
            sp.closed = true;
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

fn step_and_repeat(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let (dx, dy) = (f64_or(p, "dx", 12.0), f64_or(p, "dy", 12.0));
    let rows = p.get("rows").and_then(Value::as_u64).map(|v| v as usize);
    let cols = p.get("columns").and_then(Value::as_u64).map(|v| v as usize);
    let count = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 1000) as usize;
    let offsets: Vec<Vec2> = match (rows, cols) {
        (Some(r), Some(c)) if r.checked_mul(c).is_some_and(|n| n <= 1000) => (0..r)
            .flat_map(|i| (0..c).map(move |j| (i, j)))
            .filter(|&(i, j)| i + j > 0)
            .map(|(i, j)| Vec2::new(j as f64 * dx, i as f64 * dy))
            .collect(),
        _ => (1..=count).map(|k| Vec2::new(k as f64 * dx, k as f64 * dy)).collect(),
    };
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
        Ok(json!({"created": all.len() - ids.len()}))
    })
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
        s.execute("object.stroke", &json!({"weight": 4, "swatch": "Black"})).unwrap();
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
    fn separations_come_from_source_colours() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let c = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "C=100 M=0 Y=0 K=0", "ids": [c]})).unwrap();
        let k = s.execute("frame.create", &json!({"rect": [300, 100, 400, 200]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [k]})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let plate = |p: u8| {
            let mut rr = designcraft_render::Renderer::new();
            rr.threads = 0;
            rr.render_page(&d, &s.cache, 0, 1.0, false, &designcraft_render::RenderOptions { plate: Some(p), ..Default::default() }).unwrap()
        };
        let (cyan, black) = (plate(0), plate(3));
        // Cyan ink only on the cyan frame; black ink only on the black one (no rich-black guess).
        assert!(cyan.pixel(150, 150)[0] < 10 && cyan.pixel(350, 150)[0] > 245, "{:?} {:?}", cyan.pixel(150, 150), cyan.pixel(350, 150));
        assert!(black.pixel(350, 150)[0] < 10 && black.pixel(150, 150)[0] > 245);
        assert!(cyan.pixel(20, 20)[0] > 245, "paper has no ink");
    }

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
        // A knockout group exports as an image too.
        let other = s.execute("frame.create", &json!({"rect": [400, 100, 450, 150]})).unwrap()["id"].clone();
        let other2 = s.execute("frame.create", &json!({"rect": [420, 120, 470, 170]})).unwrap()["id"].clone();
        let g = s.execute("object.group", &json!({"ids": [other, other2]})).unwrap()["id"].clone();
        s.execute("object.transparencyGroup", &json!({"ids": [g], "knockout": true})).unwrap();
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("2 object(s)")), "{r}");
        // PDF export carries effects (as an image of the object).
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("soft effects")), "{r}");
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        assert!(String::from_utf8_lossy(&bytes).contains("/Subtype/Image"));
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
        // Crop 10 pt on every side: the content spreads past the frame.
        s.execute("object.fittingOptions", &json!({"ids": [f], "crop": 10, "align": 4})).unwrap();
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
