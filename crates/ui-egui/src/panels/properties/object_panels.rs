use super::object::{anchor_transform_section, appearance_section};
use super::{
    Arrowhead, Cap, DesignApp, Join, SelInfo, Sense, Stroke, StrokeAlign, StrokeType, Tokens, Ui, Value, caption, divider, full_width, icon_toggle,
    icons, json, measure, number, outline_button, pos2, rule, section, section_row, sel_info, text_attrs, units, vec2, widgets,
};

/// Stroke panel: weight, cap, miter limit, join, alignment, type, arrowheads, gap colour.
pub fn stroke_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    if crate::panels::cell_stroke::in_cells(app) {
        crate::panels::cell_stroke::panel(app, ui);
        return;
    }
    if crate::panels::colors_affect_text(app)
        && let Some(a) = text_attrs(app)
    {
        text_stroke_panel(app, ui, &a["chars"]);
        return;
    }
    let Some(i) = sel_info(app) else {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Select an object."));
        return;
    };
    let Some(st) = app.session.active().and_then(|d| d.selection.items.first().and_then(|id| d.doc.item(*id)).map(|it| it.stroke.clone())) else {
        return;
    };
    let t = Tokens::get(ui.ctx());
    egui::Grid::new("stroke_panel").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Weight:"));
        if let Some(v) = number(ui, "sw", Some(i.stroke_weight), " pt", 60.0, 3) {
            let _ = app.run("object.stroke", json!({"weight": v}));
        }
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Cap:"));
        ui.horizontal(|ui| {
            for (label, v, cur) in [("Butt", "butt", Cap::Butt), ("Round", "round", Cap::Round), ("Projecting", "projecting", Cap::Projecting)] {
                if ui.selectable_label(st.cap == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    let _ = app.run("object.stroke", json!({"cap": v}));
                }
            }
        });
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Join:"));
        ui.horizontal(|ui| {
            for (label, v, cur) in [("Miter", "miter", Join::Miter), ("Round", "round", Join::Round), ("Bevel", "bevel", Join::Bevel)] {
                if ui.selectable_label(st.join == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    let _ = app.run("object.stroke", json!({"join": v}));
                }
            }
        });
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Miter Limit:"));
        if let Some(v) = number(ui, "smiter", Some(st.miter_limit), " x", 60.0, 0) {
            let _ = app.run("object.stroke", json!({"miterLimit": v}));
        }
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Align Stroke:"));
        ui.horizontal(|ui| {
            for (label, a, cur) in
                [("Center", "center", StrokeAlign::Center), ("Inside", "inside", StrokeAlign::Inside), ("Outside", "outside", StrokeAlign::Outside)]
            {
                if ui.selectable_label(st.align == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    let _ = app.run("object.stroke", json!({"align": a}));
                }
            }
        });
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Type:"));
        let custom: Vec<String> = app.session.active().map(|d| d.doc.stroke_styles.iter().map(|x| x.name.clone()).collect()).unwrap_or_default();
        let shown = match &st.kind {
            StrokeType::Style { name } => name.clone(),
            k => k.label().to_string(),
        };
        let combo = craft_ui::ComboBox::from_id_salt("stype")
            .selected_text(crate::rtl::widget(
                ui,
                if matches!(st.kind, StrokeType::Style { .. }) { &shown } else { crate::i18n::tr(&app.ui.language, &shown) },
            ))
            .width(140.0)
            .show_ui(ui, |ui| {
                let built_in = [
                    "solid",
                    "dashed",
                    "dotted",
                    "thickThin",
                    "thinThick",
                    "thinThin",
                    "thickThick",
                    "thinThickThin",
                    "thickThinThick",
                    "wavy",
                    "hashed",
                ];
                for k in built_in {
                    let ty = if k == "dashed" { json!({"kind": k, "pattern": [12.0, 4.0]}) } else { json!({"kind": k}) };
                    let label = serde_json::from_value::<StrokeType>(ty.clone()).map(|k| k.label()).unwrap_or("");
                    if ui.selectable_label(shown == label, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                        let _ = app.run("object.stroke", json!({"type": ty}));
                    }
                }
                if !custom.is_empty() {
                    ui.separator();
                }
                for n in &custom {
                    if ui.selectable_label(shown == *n, n).clicked() {
                        let _ = app.run("object.stroke", json!({"type": {"kind": "style", "name": n}}));
                    }
                }
            });
        let _ = crate::widgets::name_combo(combo, crate::i18n::tr(&app.ui.language, "Type"));
        ui.end_row();
        for (key, label, cur, flip) in [("start", "Start/End:", st.start, true), ("end", "", st.end, false)] {
            caption(ui, crate::i18n::tr(&app.ui.language, label));
            let combo = craft_ui::ComboBox::from_id_salt(key)
                .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, cur.label())))
                .width(140.0)
                .show_ui(ui, |ui| {
                    for a in Arrowhead::ALL {
                        ui.horizontal(|ui| {
                            arrow_preview(ui, a, flip, t.text_strong);
                            if ui.selectable_label(a == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, a.label()))).clicked() {
                                let _ = app.run("object.stroke", json!({key: a}));
                            }
                        });
                    }
                });
            // The second arrowhead reuses the shared caption, so it needs its own semantic name.
            let name = crate::i18n::tr(&app.ui.language, if flip { "Start/End" } else { "End" }).to_owned();
            let _ = crate::widgets::name_combo(combo, name);
            ui.end_row();
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "Gap Color:"));
        crate::panels::swatch_picker(app, ui, "sgap", Some(st.gap_swatch.clone()), |app, n| {
            let _ = app.run("object.stroke", json!({"gapSwatch": n}));
        });
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Color:"));
        crate::panels::swatch_picker(app, ui, "spick", Some(i.stroke.clone()), |app, n| {
            let _ = app.run("object.stroke", json!({"swatch": n}));
        });
        ui.end_row();
    });
}

/// Stroke panel while text is selected: the text's outline colour, weight and whether it sits behind the fill.
fn text_stroke_panel(app: &mut DesignApp, ui: &mut egui::Ui, c: &Value) {
    let swatch = c["stroke"].as_str().unwrap_or(designcraft_color::swatch::NONE).to_string();
    egui::Grid::new("text_stroke_panel").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Weight:"));
        if let Some(v) = number(ui, "tsw_panel", c["strokeWeight"].as_f64(), " pt", 60.0, 3) {
            let _ = app.run("type.char", json!({"attrs": {"strokeWeight": v}}));
        }
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Color:"));
        crate::panels::swatch_picker(app, ui, "tspick", Some(swatch), |app, n| {
            let _ = crate::panels::apply_swatch(app, true, &n, None);
        });
        ui.end_row();
    });
    let mut behind = c["strokeBehindFill"].as_bool().unwrap_or(false);
    if ui.checkbox(&mut behind, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Stroke Behind Fill"))).changed() {
        let _ = app.run("type.char", json!({"attrs": {"strokeBehindFill": behind}}));
    }
}

/// A short line ending in arrowhead `a` (pointing left for Start, right for End).
fn arrow_preview(ui: &mut egui::Ui, a: Arrowhead, start: bool, color: egui::Color32) {
    use designcraft_geom::kurbo;
    let (r, _) = ui.allocate_exact_size(vec2(56.0, 14.0), Sense::hover());
    let mut line = kurbo::BezPath::new();
    let (y, x0, x1) = (r.center().y as f64, r.left() as f64 + 6.0, r.right() as f64 - 6.0);
    if start {
        line.move_to((x1, y));
        line.line_to((x0, y));
    } else {
        line.move_to((x0, y));
        line.line_to((x1, y));
    }
    let st = designcraft_doc::Stroke { weight: 1.5, end: a, ..Default::default() };
    let (line, heads) = designcraft_doc::arrow::apply(&line, &st, false).unwrap_or((line, vec![]));
    let pts = |p: &kurbo::BezPath| {
        let mut v = Vec::new();
        kurbo::flatten(p.iter(), 0.1, |el| match el {
            kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => v.push(pos2(p.x as f32, p.y as f32)),
            _ => {}
        });
        v
    };
    let painter = ui.painter();
    painter.add(egui::Shape::line(pts(&line), Stroke::new(1.5, color)));
    for h in heads {
        let p = pts(&h.path);
        match h.outline {
            Some(w) if matches!(h.path.elements().last(), Some(kurbo::PathEl::ClosePath)) => {
                painter.add(egui::Shape::closed_line(p, Stroke::new(w as f32, color)));
            }
            Some(w) => {
                painter.add(egui::Shape::line(p, Stroke::new(w as f32, color)));
            }
            None => {
                painter.add(egui::Shape::convex_polygon(p, color, Stroke::NONE));
            }
        }
    }
}

pub fn wrap_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let info = sel_info(app);
    let mode = info.as_ref().map_or("none", |i| i.wrap);
    // As InDesign's Text Wrap panel: the five wrap buttons and Invert, then the four offsets
    // (top and bottom, left and right) with the link button that makes them all the same.
    ui.horizontal(|ui| {
        for (icon, m, tip) in [
            ("wrap-none", "none", "No Text Wrap"),
            ("wrap-bbox", "boundingBox", "Wrap Around Bounding Box"),
            ("wrap-contour", "contour", "Wrap Around Object Shape"),
            ("wrap-jump", "jumpObject", "Jump Object"),
            ("wrap-next", "jumpToNextColumn", "Jump to Next Column"),
        ] {
            if icons::button(ui, icon, 26.0, mode == m, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.textWrap", json!({"mode": m}));
            }
        }
        ui.add_space(6.0);
        let mut inv = info.as_ref().is_some_and(|i| i.wrap_invert);
        let r = ui.add_enabled_ui(mode != "none", |ui| widgets::checkbox(ui, &mut inv, crate::i18n::tr(&app.ui.language, "Invert"))).inner;
        if r.changed() {
            let _ = app.run("object.textWrap", json!({"mode": mode, "invert": inv}));
        }
    });
    let u = units(app);
    let offsets = info.as_ref().map(|i| i.wrap_offsets);
    // Linked while the offsets are all the same, until the link is clicked.
    let linked_id = egui::Id::unique("wrap_offsets_linked");
    let linked = ui.data(|d| d.get_temp::<bool>(linked_id)).unwrap_or_else(|| offsets.is_none_or(|o| o.iter().all(|v| (v - o[0]).abs() < 1e-9)));
    let mut set: Option<[f64; 4]> = None;
    ui.add_enabled_ui(mode != "none" && info.is_some(), |ui| {
        // Two rows of fixed-width cells: Top | Left, then Bottom | Right; the link between them.
        let cell = |ui: &mut Ui, w: f32, label: &str| {
            let x0 = ui.cursor().min.x;
            caption(ui, crate::i18n::tr(&app.ui.language, label));
            ui.add_space((w - (ui.cursor().min.x - x0)).max(0.0));
        };
        // The second column starts where the first row's does (after the link button).
        let mut col2_x: Option<f32> = None;
        for (r, row) in [[(0, "Top"), (1, "Left")], [(2, "Bottom"), (3, "Right")]].into_iter().enumerate() {
            ui.horizontal(|ui| {
                for (k, (side, label)) in row.into_iter().enumerate() {
                    if k == 1 {
                        match col2_x {
                            Some(x) => ui.add_space((x - ui.cursor().min.x).max(0.0)),
                            None => col2_x = Some(ui.cursor().min.x),
                        }
                    }
                    cell(ui, if k == 0 { 56.0 } else { 40.0 }, label);
                    if let Some(v) = measure(ui, &format!("wo{side}"), offsets.map(|o| o[side]), u, 60.0) {
                        let mut o = offsets.unwrap_or([0.0; 4]);
                        if linked {
                            o = [v; 4];
                        } else {
                            o[side] = v;
                        }
                        set = Some(o);
                    }
                    if k == 0 && r == 0 {
                        let tip = crate::i18n::tr(&app.ui.language, "Make all settings the same");
                        let link = widgets::icon_toggle_sized(ui, if linked { "link" } else { "link-broken" }, linked, tip, vec2(22.0, 21.0));
                        if link.clicked() {
                            ui.data_mut(|d| d.insert_temp(linked_id, !linked));
                        }
                    }
                }
            });
        }
    });
    if let Some(o) = set {
        let _ = app.run("object.textWrap", json!({"mode": mode, "offset": o}));
    }
    let Some(i) = info.as_ref().filter(|i| i.wrap == "boundingBox" || i.wrap == "contour") else { return };
    let sides = [
        ("bothSides", "Both Right & Left Sides"),
        ("rightSide", "Right Side"),
        ("leftSide", "Left Side"),
        ("towardsSpine", "Side Towards Spine"),
        ("awayFromSpine", "Side Away from Spine"),
        ("largestArea", "Largest Area"),
    ];
    wrap_combo(app, ui, "wrap_side", "Wrap To", &sides, &[], &i.wrap_side, "side");
    if i.wrap == "contour" && i.is_graphic {
        // Detected edges, alpha channels and image paths aren't offered: composition doesn't read
        // the image. An imported one shows by name and wraps around the frame.
        let types = [("boundingBox", "Bounding Box"), ("graphicFrame", "Graphic Frame"), ("sameAsClipping", "Same as Clipping")];
        let shown_only = [("detectEdges", "Detect Edges"), ("alphaChannel", "Alpha Channel"), ("photoshopPath", "Photoshop Path")];
        wrap_combo(app, ui, "wrap_contour", "Contour Type", &types, &shown_only, &i.wrap_contour, "contour");
    }
}

/// A Text Wrap panel choice: `opts` are offered, `shown_only` only name the current value.
#[allow(clippy::too_many_arguments)]
fn wrap_combo(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    salt: &str,
    caption_text: &str,
    opts: &[(&str, &str)],
    shown_only: &[(&str, &str)],
    cur: &str,
    key: &str,
) {
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, caption_text));
        let shown = opts.iter().chain(shown_only).find(|o| o.0 == cur).map_or(cur, |o| o.1).to_string();
        let combo = craft_ui::ComboBox::from_id_salt(salt)
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &shown)))
            .width(170.0)
            .show_ui(ui, |ui| {
                for (k, l) in opts {
                    if ui.selectable_label(*k == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        let _ = app.run("object.textWrap", json!({ key: k }));
                    }
                }
            });
        let _ = crate::widgets::name_combo(combo, crate::i18n::tr(&app.ui.language, caption_text));
    });
}

/// The Pathfinder operations, in the order of InDesign's Pathfinder buttons.
const PATHFINDER: [(&str, &str, &str); 5] = [
    ("pf-add", "add", "Add"),
    ("pf-subtract", "subtract", "Subtract"),
    ("pf-intersect", "intersect", "Intersect"),
    ("pf-exclude", "exclude", "Exclude Overlap"),
    ("pf-minus-back", "minusBack", "Minus Back"),
];

/// Properties › Pathfinder (several objects selected): combine them into one shape.
pub(super) fn pathfinder_row(app: &mut DesignApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (icon, op, tip) in PATHFINDER {
            if icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.pathfinder", json!({"op": op}));
            }
        }
    });
}

/// Pathfinder panel › Paths: (icon, command, params, tooltip).
const PATHS: [(&str, &str, &str, &str); 4] = [
    ("path-join", "path.join", "{}", "Join Path"),
    ("path-open", "object.convertShape", r#"{"to": "openPath"}"#, "Open Path"),
    ("path-close", "object.convertShape", r#"{"to": "closedPath"}"#, "Close Path"),
    ("path-reverse", "path.reverse", "{}", "Reverse Path"),
];

/// Pathfinder panel › Convert Shape: (icon, shape, tooltip).
const CONVERT_SHAPE: [(&str, &str, &str); 9] = [
    ("shape-rect", "rectangle", "Rectangle"),
    ("shape-rounded", "roundedRectangle", "Rounded Rectangle"),
    ("shape-beveled", "beveledRectangle", "Beveled Rectangle"),
    ("shape-inverse", "inverseRoundedRectangle", "Inverse Rounded Rectangle"),
    ("shape-ellipse", "ellipse", "Ellipse"),
    ("shape-triangle", "triangle", "Triangle"),
    ("shape-polygon", "polygon", "Polygon"),
    ("shape-line", "line", "Line"),
    ("shape-orthogonal", "orthogonalLine", "Orthogonal Line"),
];

/// Points selected with Direct Selection, as InDesign's Properties shows them: Transform,
/// Appearance, Convert Points (the point type of the selected points) and Delete Point(s).
pub(super) fn anchor_point_sections(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    section(ui, crate::i18n::tr(&app.ui.language, "Transform"), true);
    anchor_transform_section(app, ui, i);
    divider(ui);
    section(ui, crate::i18n::tr(&app.ui.language, "Appearance"), true);
    appearance_section(app, ui, i);
    divider(ui);
    let mut kind = None;
    let lang = app.ui.language.clone();
    section_row(ui, crate::i18n::tr(&lang, "Convert Points"), |ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        // Laid out from the right: last first, so the row reads Plain … Symmetrical.
        for (icon, k, tip) in CONVERT_POINT.iter().rev() {
            if icon_toggle(ui, icon, false, crate::i18n::tr(&lang, tip)).clicked() {
                kind = Some(k);
            }
        }
    });
    if let Some(k) = kind {
        let _ = app.run("path.convertPoints", json!({"kind": k}));
    }
    rule(ui);
    section(ui, crate::i18n::tr(&app.ui.language, "Quick Actions"), true);
    let fw = full_width(ui);
    if outline_button(ui, crate::i18n::tr(&app.ui.language, "Delete Point(s)"), fw).clicked() {
        let _ = app.run("path.deletePoints", json!({}));
    }
}

/// Pathfinder panel › Convert Point: (icon, kind, tooltip).
const CONVERT_POINT: [(&str, &str, &str); 4] = [
    ("point-plain", "plain", "Plain"),
    ("point-corner", "corner", "Corner"),
    ("point-smooth", "smooth", "Smooth"),
    ("point-symmetrical", "symmetrical", "Symmetrical"),
];

/// Pathfinder panel, as InDesign's: Paths, Pathfinder, Convert Shape and Convert Point, each a row of
/// icon buttons acting on the selection.
pub fn pathfinder_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let section = |ui: &mut egui::Ui, app: &DesignApp, title: &str| {
        ui.add_space(4.0);
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, title));
        ui.add_space(2.0);
    };
    let row = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui)| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 8.0);
            add(ui);
        });
    };
    section(ui, app, "Paths:");
    row(ui, &mut |ui| {
        for (icon, cmd, params, tip) in PATHS {
            if icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run(cmd, serde_json::from_str(params).unwrap_or_default());
            }
        }
    });
    section(ui, app, "Pathfinder:");
    row(ui, &mut |ui| {
        for (icon, op, tip) in PATHFINDER {
            if icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.pathfinder", json!({"op": op}));
            }
        }
    });
    section(ui, app, "Convert Shape:");
    row(ui, &mut |ui| {
        for (icon, to, tip) in CONVERT_SHAPE {
            if icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.convertShape", json!({"to": to}));
            }
        }
    });
    section(ui, app, "Convert Point:");
    row(ui, &mut |ui| {
        for (icon, kind, tip) in CONVERT_POINT {
            if icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("path.convertPoints", json!({"kind": kind}));
            }
        }
    });
}

pub fn align_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        for (edge, icon, tip) in [
            ("left", "objalign-left", "Align left edges"),
            ("hcenter", "objalign-hcenter", "Align horizontal centers"),
            ("right", "objalign-right", "Align right edges"),
            ("top", "objalign-top", "Align top edges"),
            ("vcenter", "objalign-vcenter", "Align vertical centers"),
            ("bottom", "objalign-bottom", "Align bottom edges"),
        ] {
            if widgets::icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.align", json!({"edge": edge}));
            }
        }
    });
    ui.horizontal(|ui| {
        if widgets::icon_toggle(ui, "objdistribute-h", false, crate::i18n::tr(&app.ui.language, "Distribute ↔")).clicked() {
            let _ = app.run("object.distribute", json!({"axis": "horizontal"}));
        }
        if widgets::icon_toggle(ui, "objdistribute-v", false, crate::i18n::tr(&app.ui.language, "Distribute ↕")).clicked() {
            let _ = app.run("object.distribute", json!({"axis": "vertical"}));
        }
    });
}

pub fn effects_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(st) = app.session.active() else { return };
    let Some(it) = st.selection.items.first().and_then(|i| st.doc.item(*i)).cloned() else {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Select an object."));
        return;
    };
    use designcraft_color::BlendMode as B;
    const MODES: [B; 16] = [
        B::Normal,
        B::Multiply,
        B::Screen,
        B::Overlay,
        B::SoftLight,
        B::HardLight,
        B::ColorDodge,
        B::ColorBurn,
        B::Darken,
        B::Lighten,
        B::Difference,
        B::Exclusion,
        B::Hue,
        B::Saturation,
        B::Color,
        B::Luminosity,
    ];
    ui.horizontal(|ui| {
        let combo = craft_ui::ComboBox::from_id_salt("blend")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, it.blend.label())))
            .width(130.0)
            .show_ui(ui, |ui| {
                for m in MODES {
                    if ui.selectable_label(m == it.blend, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, m.label()))).clicked() {
                        let _ = app.run("object.opacity", json!({"opacity": it.opacity, "blend": m}));
                    }
                }
            });
        let _ = crate::widgets::name_combo(combo, crate::i18n::tr(&app.ui.language, "Blending"));
        caption(ui, crate::i18n::tr(&app.ui.language, "Opacity"));
        if let Some(v) = number(ui, "eop", Some(it.opacity as f64 * 100.0), "%", 46.0, 0) {
            let _ = app.run("object.opacity", json!({"opacity": (v / 100.0).clamp(0.0, 1.0)}));
        }
    });
    // The level the settings apply to, with its blend mode and opacity ("Group: Color Dodge 100%").
    let group = it.is_group();
    ui.horizontal(|ui| {
        let level = crate::i18n::tr(&app.ui.language, if group { "Group:" } else { "Object:" });
        crate::rtl::label(ui, egui::RichText::new(level).font(crate::theme::semibold(12.0)));
        crate::rtl::label(ui, format!("{} {}%", crate::i18n::tr(&app.ui.language, it.blend.label()), (it.opacity * 100.0).round()));
    });
    divider(ui);
    let ds = it.effects.drop_shadow.clone();
    let mut on = ds.on;
    if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Drop Shadow"))).changed() {
        let _ = app.run("object.dropShadow", json!({"on": on}));
    }
    if ds.on {
        egui::Grid::new("dsg").num_columns(4).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Distance"));
            if let Some(v) = number(ui, "dsd", Some(ds.distance), " pt", 56.0, 1) {
                let _ = app.run("object.dropShadow", json!({"on": true, "distance": v}));
            }
            caption(ui, crate::i18n::tr(&app.ui.language, "Angle"));
            let global = app.session.active().map_or(120.0, |st| st.doc.settings.global_light);
            if let Some(v) = number(ui, "dsa", Some(if ds.global_light { global } else { ds.angle }), "°", 50.0, 0) {
                // With Use Global Light on, the angle is the document's (every such shadow moves).
                let _ = if ds.global_light {
                    app.run("object.globalLight", json!({"angle": v}))
                } else {
                    app.run("object.dropShadow", json!({"on": true, "angle": v}))
                };
            }
            ui.end_row();
            crate::rtl::label(ui, "");
            crate::rtl::label(ui, "");
            caption(ui, "");
            let mut gl = ds.global_light;
            if ui.checkbox(&mut gl, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Use Global Light"))).changed() {
                let _ = app.run("object.dropShadow", json!({"on": true, "globalLight": gl}));
            }
            ui.end_row();
            caption(ui, crate::i18n::tr(&app.ui.language, "Opacity"));
            if let Some(v) = number(ui, "dso", Some(ds.opacity as f64 * 100.0), "%", 56.0, 0) {
                let _ = app.run("object.dropShadow", json!({"on": true, "opacity": v / 100.0}));
            }
            caption(ui, crate::i18n::tr(&app.ui.language, "Size"));
            if let Some(v) = number(ui, "dss", Some(ds.size), " pt", 50.0, 1) {
                let _ = app.run("object.dropShadow", json!({"on": true, "size": v}));
            }
            ui.end_row();
        });
    }
    divider(ui);
    // Groups only, as in InDesign (greyed for other objects).
    ui.horizontal(|ui| {
        let mut iso = it.isolate;
        let label = crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Isolate Blending"));
        if ui.add_enabled(group, egui::Checkbox::new(&mut iso, label)).changed() {
            let _ = app.run("object.transparencyGroup", json!({"isolate": iso}));
        }
        let mut ko = it.knockout;
        let label = crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Knockout Group"));
        if ui.add_enabled(group, egui::Checkbox::new(&mut ko, label)).changed() {
            let _ = app.run("object.transparencyGroup", json!({"knockout": ko}));
        }
    });
    // The fx button, as at the bottom of InDesign's Effects panel: each effect opens the Effects
    // dialog on its page.
    let fx = ui
        .add(egui::Button::new(egui::RichText::new("fx").italics().strong()).min_size(vec2(28.0, 20.0)))
        .on_hover_text(crate::i18n::tr(&app.ui.language, "Add an object effect to the selected target"));
    egui::Popup::menu(&fx).show(|ui| {
        for (page, label) in crate::effects_dialog::PAGES {
            let text = format!("{}…", crate::i18n::tr(&app.ui.language, label));
            if ui.button(crate::rtl::widget(ui, text)).clicked() {
                crate::effects_dialog::open(app, page);
                ui.close();
            }
        }
    });
}

/// Window › Output › Attributes: overprint fill, stroke and gap; nonprinting.
pub fn attributes_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(it) = app.session.active().and_then(|d| d.selection.items.first().and_then(|id| d.doc.item(*id)).cloned()) else {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Select an object."));
        return;
    };
    let rows = [
        ("overprintFill", "Overprint Fill", it.fill.overprint),
        ("overprintStroke", "Overprint Stroke", it.stroke.overprint),
        ("nonprinting", "Nonprinting", it.nonprinting),
        ("overprintGap", "Overprint Gap", it.stroke.gap_overprint),
    ];
    egui::Grid::new("attributes_panel").num_columns(2).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
        for (i, (key, label, on)) in rows.into_iter().enumerate() {
            let mut v = on;
            if ui.checkbox(&mut v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).changed() {
                let _ = app.run("object.attributes", json!({ key: v }));
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
}

#[cfg(test)]
mod effects_panel_tests {
    use super::*;

    /// One frame of the Effects panel: the text painted and where.
    fn frame(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<(String, egui::Rect)> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 400.0));
        let time = ctx.input(|i| i.time) + 1.0 / 60.0;
        let mut out =
            ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(time), events, ..Default::default() }, |ui| effects_panel(app, ui));
        out.textures_delta.clear();
        let mut v = Vec::new();
        for s in out.shapes {
            if let egui::Shape::Text(t) = s.shape {
                v.push((t.galley.job.text.clone(), t.visual_bounding_rect()));
            }
        }
        v
    }

    fn click(app: &mut DesignApp, ctx: &egui::Context, label: &str) {
        let labels = frame(app, ctx, vec![]);
        let pos = labels.iter().find(|(t, _)| t == label).unwrap_or_else(|| panic!("{label} is painted")).1.center();
        for pressed in [true, false] {
            frame(
                app,
                ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                ],
            );
        }
    }

    #[test]
    fn the_effects_panel_shows_the_level_and_isolate_blending_and_knockout_group_for_groups() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [72, 72, 144, 144]})).unwrap()["id"].as_u64().unwrap();
        let b = app.run("frame.create", json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        let g = app.run("object.group", json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [g]})).unwrap();
        app.run("object.opacity", json!({"opacity": 1.0, "blend": "colorDodge"})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        crate::theme::apply(&ctx, &Tokens::for_brightness(crate::theme::Brightness::default()));
        let texts: Vec<String> = frame(&mut app, &ctx, vec![]).into_iter().map(|(t, _)| t).collect();
        for expected in ["Group:", "Color Dodge 100%", "Isolate Blending", "Knockout Group"] {
            assert!(texts.iter().any(|t| t == expected), "{expected} in {texts:?}");
        }
        let item = |app: &DesignApp, id: u64| app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).cloned().unwrap();
        click(&mut app, &ctx, "Isolate Blending");
        assert!(item(&app, g).isolate);
        click(&mut app, &ctx, "Knockout Group");
        assert!(item(&app, g).knockout);
        // Another object: the level is Object, and the group options are greyed.
        app.run("selection.set", json!({"ids": [a]})).unwrap();
        let texts: Vec<String> = frame(&mut app, &ctx, vec![]).into_iter().map(|(t, _)| t).collect();
        assert!(texts.iter().any(|t| t == "Object:"), "{texts:?}");
        click(&mut app, &ctx, "Knockout Group");
        assert!(!item(&app, a).knockout);
    }

    #[test]
    fn pasted_into_frames_keep_object_level_and_disable_group_blending() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let child = app.run("frame.create", json!({"rect": [80, 80, 120, 120]})).unwrap()["id"].clone();
        app.run("selection.set", json!({"ids": [child]})).unwrap();
        app.run("edit.copy", json!({})).unwrap();
        let container = app.run("frame.create", json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [container]})).unwrap();
        app.run("edit.pasteInto", json!({})).unwrap();
        app.run("selection.set", json!({"ids": [container]})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        crate::theme::apply(&ctx, &Tokens::for_brightness(crate::theme::Brightness::default()));
        let texts: Vec<String> = frame(&mut app, &ctx, vec![]).into_iter().map(|(t, _)| t).collect();
        assert!(texts.iter().any(|t| t == "Object:"), "a frame remains an object after Paste Into: {texts:?}");
        let undo = app.session.doc().unwrap().history.undo.len();
        click(&mut app, &ctx, "Isolate Blending");
        click(&mut app, &ctx, "Knockout Group");
        let st = app.session.doc().unwrap();
        let it = st.doc.item(designcraft_doc::ItemId(container)).unwrap();
        assert!(it.has_nested_items());
        assert!(!it.isolate && !it.knockout, "group-only options stay disabled for a frame holding content");
        assert_eq!(st.history.undo.len(), undo, "disabled controls do not create undo steps");
    }
}

#[cfg(test)]
mod wrap_panel_tests {
    use super::*;

    /// One frame of the Text Wrap panel: the text painted and where.
    fn frame(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<(String, egui::Rect)> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(360.0, 300.0));
        let time = ctx.input(|i| i.time) + 1.0 / 60.0;
        let mut out =
            ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(time), events, ..Default::default() }, |ui| wrap_panel(app, ui));
        out.textures_delta.clear();
        out.shapes
            .into_iter()
            .filter_map(|s| if let egui::Shape::Text(t) = s.shape { Some((t.galley.job.text.clone(), t.visual_bounding_rect())) } else { None })
            .collect()
    }

    #[test]
    fn the_text_wrap_panel_shows_each_offset_and_invert() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 144, 144]})).unwrap()["id"].as_u64().unwrap();
        app.run("object.textWrap", json!({"mode": "boundingBox", "offset": [1, 2, 3, 4]})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        // Top and Left, then Bottom and Right, each with its own value (picas here: 0p1 = 1 pt),
        // then the Wrap To side.
        let t: Vec<String> = frame(&mut app, &ctx, vec![]).into_iter().map(|(t, _)| t).filter(|t| !t.is_empty()).collect();
        assert_eq!(t, ["Invert", "Top", "0p1", "Left", "0p2", "Bottom", "0p3", "Right", "0p4", "Wrap To", "Both Right & Left Sides"]);
        // Invert, next to the wrap buttons.
        let pos = frame(&mut app, &ctx, vec![]).into_iter().find(|(t, _)| t == "Invert").unwrap().1.center();
        for pressed in [true, false] {
            frame(
                &mut app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                ],
            );
        }
        assert!(app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().wrap.invert);
    }
}
