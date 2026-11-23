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
            let _ = app.run("object.stroke", json!({"weight": v.clamp(0.0, 1000.0)}));
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
            let _ = app.run("object.stroke", json!({"miterLimit": v.clamp(1.0, 500.0)}));
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
        let combo = craft_ui_legacy::ComboBox::from_id_salt("stype")
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
            let combo = craft_ui_legacy::ComboBox::from_id_salt(key)
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
            // The character stroke weight's range, as the character style's Weight field has it.
            let _ = app.run("type.char", json!({"attrs": {"strokeWeight": v.clamp(0.0, 800.0)}}));
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

/// Text Wrap keeps its existing artwork; only discrete input moves through the installed SDK.
#[cfg(not(target_arch = "wasm32"))]
#[derive(craft_ui::contract::MemoProps)]
struct WrapModeArtwork<'a>(&'a str);

#[cfg(not(target_arch = "wasm32"))]
impl craft_ui_widgets::primitives::Glyph for WrapModeArtwork<'_> {
    fn paint(&self, painter: &egui::Painter, _tokens: &craft_ui_widgets::style::UiTokens, rect: egui::Rect, color: egui::Color32, _mirrored: bool) {
        icons::paint(painter, rect, self.0, color);
    }
}

/// Each discrete press belongs to the current document and frame/content target, not its old occupant.
fn wrap_control_id(app: &DesignApp, ui: &egui::Ui) -> egui::Id {
    let target = app
        .session
        .active()
        .map(|st| format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, st.selection.content, &st.selection.text, &st.selection.cells)));
    ui.scope_id().with(("text-wrap-mode-invert", target))
}

fn wrap_mode_button(app: &DesignApp, ui: &mut egui::Ui, id: egui::Id, icon: &str, selected: bool, label: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::buttons::Button;
        use craft_ui_widgets::contract::{Props as _, Show as _};
        use craft_ui_widgets::primitives::IconSpec;
        use craft_theme::ControlSize;
        use craft_ui_widgets::style::{Variant};

        // Keep the original compact row and selected plate, not a six-wide row of painted labels.
        let (_, rect) = ui.allocate_space(vec2(26.0, 26.0));
        if selected {
            ui.painter().rect_filled(rect, 3.0, Tokens::get(ui.ctx()).tool_active);
        }
        let artwork = WrapModeArtwork(icon);
        return Button::new(label.to_owned())
            .id(id)
            .painted("")
            .icon(IconSpec::glyph(&artwork))
            .size(ControlSize::Sm)
            .variant(Variant::GHOST)
            .selected(selected)
            .tooltip(label.to_owned())
            .show_at(ui, rect)
            .clicked();
    }
    #[cfg(target_arch = "wasm32")]
    let _ = app;
    ui.push_id(id, |ui| icons::button(ui, icon, 26.0, selected, label)).inner.clicked()
}

fn wrap_invert_checkbox(app: &DesignApp, ui: &mut egui::Ui, id: egui::Id, value: &mut bool, label: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::choice::Checkbox;
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
        let output = Checkbox::new(Bind::given(*value)).id(id).size(craft_theme::ControlSize::Sm).label(label.to_owned()).show(ui);
        if let Some(next) = output.events.committed().copied().filter(|next| *next != *value) {
            *value = next;
            return true;
        }
        return false;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = app;
    ui.push_id(id, |ui| widgets::checkbox(ui, value, label)).inner.changed()
}

pub fn wrap_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let info = sel_info(app);
    let mode = info.as_ref().map_or("none", |i| i.wrap);
    let input_id = wrap_control_id(app, ui);
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
            if wrap_mode_button(app, ui, input_id.with(m), icon, mode == m, crate::i18n::tr(&app.ui.language, tip)) {
                let _ = app.run("object.textWrap", json!({"mode": m}));
            }
        }
        ui.add_space(6.0);
        let mut inv = info.as_ref().is_some_and(|i| i.wrap_invert);
        let changed = ui
            .add_enabled_ui(mode != "none", |ui| {
                wrap_invert_checkbox(app, ui, input_id.with("invert"), &mut inv, crate::i18n::tr(&app.ui.language, "Invert"))
            })
            .inner;
        if changed {
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
        // Detected edges and image paths aren't offered: they need the image's own analysis or
        // paths. An imported one shows by name and wraps around the frame.
        let types = [
            ("boundingBox", "Bounding Box"),
            ("alphaChannel", "Alpha Channel"),
            ("graphicFrame", "Graphic Frame"),
            ("sameAsClipping", "Same as Clipping"),
        ];
        let shown_only = [("detectEdges", "Detect Edges"), ("photoshopPath", "Photoshop Path")];
        wrap_combo(app, ui, "wrap_contour", "Contour Type", &types, &shown_only, &i.wrap_contour, "contour");
        if i.wrap_contour == "alphaChannel" {
            let mut inside = i.wrap_inside_edges;
            if widgets::checkbox(ui, &mut inside, crate::i18n::tr(&app.ui.language, "Include Inside Edges")).changed() {
                let _ = app.run("object.textWrap", json!({"insideEdges": inside}));
            }
        }
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
        let combo = craft_ui_legacy::ComboBox::from_id_salt(salt)
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
        let combo = craft_ui_legacy::ComboBox::from_id_salt("blend")
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
        // Distance and Size in the document's unit, as in the Effects dialog: a bare `7` means the
        // same in both.
        let u = units(app);
        egui::Grid::new("dsg").num_columns(4).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Distance"));
            if let Some(v) = measure(ui, "dsd", Some(ds.distance), u, 56.0) {
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
            if let Some(v) = measure(ui, "dss", Some(ds.size), u, 56.0) {
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

/// Ordinary boolean input only; the engine retains ownership of the selected objects and Undo.
fn attribute_checkbox(app: &DesignApp, ui: &mut egui::Ui, id: egui::Id, value: &mut bool, label: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::choice::Checkbox;
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
        #[cfg(all(test, not(target_arch = "wasm32")))]
        attributes_workflow_tests::observe_geometry(app, ui, "before SDK checkbox", label, None);
        let output = Checkbox::new(Bind::given(*value)).id(id).label(label.to_owned()).show(ui);
        #[cfg(all(test, not(target_arch = "wasm32")))]
        attributes_workflow_tests::observe_geometry(app, ui, "after SDK checkbox", label, Some(output.response.rect));
        if let Some(next) = output.events.committed().copied().filter(|next| *next != *value) {
            *value = next;
            return true;
        }
        return false;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, id);
    ui.checkbox(value, crate::rtl::widget(ui, label)).changed()
}

/// Window › Output › Attributes: overprint fill, stroke and gap; nonprinting.
pub fn attributes_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    attributes_workflow_tests::observe_geometry(app, ui, "before direction scope", "", None);
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::runtime::env::{Direction, with_env};
        let direction = if crate::i18n::is_rtl(&app.ui.language) { Direction::Rtl } else { Direction::Ltr };
        with_env(ui, |scope| scope.direction = Some(direction), |ui| attributes_contents(app, ui));
        return;
    }
    attributes_contents(app, ui);
}

fn attributes_contents(app: &mut DesignApp, ui: &mut egui::Ui) {
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
    let target =
        app.session.active().map(|st| format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, &st.selection.text, &st.selection.cells)));
    let id = ui.scope_id().with(("attributes", target));
    let mut show_grid = |ui: &mut egui::Ui| {
        #[cfg(all(test, not(target_arch = "wasm32")))]
        attributes_workflow_tests::observe_geometry(app, ui, "before native Grid", "", None);
        egui::Grid::new("attributes_panel").num_columns(2).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
            for (i, (key, label, on)) in rows.into_iter().enumerate() {
                let mut v = on;
                if attribute_checkbox(app, ui, id.with(key), &mut v, crate::i18n::tr(&app.ui.language, label)) {
                    let _ = app.run("object.attributes", json!({ key: v }));
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    };
    if ui.layout().prefer_right_to_left() {
        // egui Grid places cells from cursor.min, so its physical row must stay left-to-right.
        // The SDK's inherited Direction::Rtl still owns each checkbox's text and logical direction.
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), show_grid);
    } else {
        show_grid(ui);
    }
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod attributes_workflow_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use egui::{Event, Key, Modifiers, PointerButton, UiBuilder};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::{collections::HashSet, sync::Arc};

    const ROWS: [(&str, &str); 4] = [
        ("overprintFill", "Overprint Fill"),
        ("overprintStroke", "Overprint Stroke"),
        ("nonprinting", "Nonprinting"),
        ("overprintGap", "Overprint Gap"),
    ];
    struct Rig {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        enabled: bool,
        sizing: bool,
    }
    type Mark = (Arc<designcraft_doc::Document>, String, (usize, usize));

    fn step(h: &mut Harness<'_, Rig>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, Rig>, frames: usize) {
        for _ in 0..frames {
            step(h);
        }
    }
    fn event(h: &mut Harness<'_, Rig>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn key(h: &mut Harness<'_, Rig>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
    }
    fn pointer(h: &mut Harness<'_, Rig>, pos: egui::Pos2, pressed: bool) {
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    /// Genuine already-installed native face, using the protected current47 Interactive fixture contract.
    /// This is a strict test precondition; it does not change the production/default font input.
    fn install_arabic_fixture(ctx: &egui::Context, language: &str) {
        #[cfg(target_os = "macos")]
        let family = "Geeza Pro";
        #[cfg(target_os = "windows")]
        let family = "Segoe UI";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let family = "DejaVu Sans";
        let db = designcraft_fonts::FontDb::global();
        assert!(db.has_family(family), "the native Arabic fixture requires the already-installed {family}; do not skip Arabic or substitute tofu");
        let face = db.face(family, "Regular");
        assert!(face.family.eq_ignore_ascii_case(family), "the named fixture must resolve to {family}, not {}", face.family);
        assert!(
            !face.is_placeholder() && face.covers('\u{0627}') && face.covers('\u{0625}') && face.covers('\u{0637}') && face.covers('\u{0639}'),
            "the real existing face must contain U+0627/U+0625 and both actual failed U+0637/U+0639 glyphs"
        );
        #[cfg(target_os = "macos")]
        assert_eq!(face.index(), 0, "the observed existing Geeza Pro Regular fixture is TTC face0");
        assert!(face.style.eq_ignore_ascii_case("Regular"), "the genuine fixture requires the requested regular face");
        for key in ROWS.iter().map(|row| row.1).chain(["Select an object."]) {
            for c in crate::i18n::tr(language, key).chars().filter(|c| designcraft_fonts::is_rtl(*c)) {
                assert!(face.covers(c), "{family} lacks the real glyph U+{:04X} from {key}", u32::from(c));
            }
        }
        let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        let fallback = crate::theme::SystemFont::new("Arab", face.family.clone(), face.data().to_vec(), face.index());
        crate::theme::add_system_fallbacks(&mut definitions, &[fallback], language);
        ctx.set_fonts(definitions);
        eprintln!(
            "Attributes Arabic fixture: real existing {} {}, face {}, {} font bytes, full original Attributes Arabic-string glyph coverage checked",
            face.family,
            face.style,
            face.index(),
            face.data().len()
        );
    }

    /// Read-only boundary evidence for the original negative-child-size failure. Never repairs a rectangle.
    pub(super) fn observe_geometry(app: &DesignApp, ui: &egui::Ui, stage: &str, label: &str, reply: Option<egui::Rect>) {
        eprintln!(
            "Attributes geometry: stage={stage} locale={:?} installed={} label={label:?} layout={:?} cursor={:?} max={:?} min={:?} clip={:?} available={:?} sizing={} reply={reply:?}",
            app.ui.language,
            app.framework_installed,
            ui.layout(),
            ui.cursor(),
            ui.max_rect(),
            ui.min_rect(),
            ui.clip_rect(),
            ui.available_size(),
            ui.is_sizing_pass(),
        );
    }

    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, Rig> {
        app.framework_opacity = true;
        let ctx = egui::Context::default();
        if installed {
            app.install_framework(&ctx);
        }
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut ready = false;
        let mut locale_fonts_ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(680.0, 300.0)).build_ui_state(
            move |ui, state: &mut Rig| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    ready = true;
                    return;
                }
                if !locale_fonts_ready {
                    locale_fonts_ready = true;
                    if state.app.ui.language == "ar" {
                        install_arabic_fixture(ui.ctx(), &state.app.ui.language);
                        return;
                    }
                }
                let builder = if state.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(state.enabled, |ui| {
                        state.controls = record(ui, egui::Id::unique("attributes-workflow"), |ui| attributes_panel(&mut state.app, ui)).1;
                    })
                });
            },
            Rig { app, controls: None, enabled: true, sizing: false },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }
    fn fixture() -> (DesignApp, [u64; 3]) {
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let mut ids = Vec::new();
        for rect in [[10, 10, 40, 40], [60, 10, 90, 40], [110, 10, 140, 40]] {
            ids.push(session.execute("frame.create", &json!({"rect":rect})).unwrap()["id"].as_u64().unwrap());
        }
        session.execute("object.attributes", &json!({"ids":[ids[2]], "overprintGap":true, "nonprinting":true})).unwrap();
        session.execute("frame.create", &json!({"rect":[160,10,190,40]})).unwrap();
        session.execute("edit.undo", &json!({})).unwrap();
        session.execute("selection.set", &json!({"ids":[ids[0]]})).unwrap();
        assert!(!session.active().unwrap().history.undo.is_empty());
        assert_eq!(session.active().unwrap().history.redo.len(), 1);
        (DesignApp::new_for_panel_tests(session, crate::Services::default()), ids.try_into().unwrap())
    }
    fn mark(h: &Harness<'_, Rig>) -> Mark {
        let st = h.state().app.session.active().unwrap();
        (
            st.doc.clone(),
            format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, &st.selection.text, &st.selection.cells)),
            (st.history.undo.len(), st.history.redo.len()),
        )
    }
    fn unchanged(h: &Harness<'_, Rig>, before: &Mark) {
        let now = mark(h);
        assert!(Arc::ptr_eq(&now.0, &before.0), "no document COW on rendering or refused input");
        assert_eq!((now.1, now.2), (before.1.clone(), before.2));
    }
    fn item(h: &Harness<'_, Rig>, id: u64) -> designcraft_doc::Item {
        h.state().app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().clone()
    }
    fn flags(h: &Harness<'_, Rig>, id: u64) -> [bool; 4] {
        let it = item(h, id);
        [it.fill.overprint, it.stroke.overprint, it.nonprinting, it.stroke.gap_overprint]
    }
    fn name(h: &Harness<'_, Rig>, key: &str) -> String {
        let label = ROWS.iter().find(|r| r.0 == key).unwrap().1;
        crate::i18n::tr(&h.state().app.ui.language, label).to_string()
    }
    fn checkbox(h: &Harness<'_, Rig>, key: &str) -> Element {
        let name = name(h, key);
        let found: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.component == "Checkbox" && e.part.is_none() && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some(name.as_str())))
            .cloned()
            .collect();
        assert_eq!(found.len(), 1, "one complete named Attributes Checkbox: {name}");
        let field = found.into_iter().next().unwrap();
        assert_eq!(field.facts.as_ref().unwrap().role, format!("{:?}", egui::accesskit::Role::CheckBox));
        let node = h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(name.as_str()));
        let rect = node.rect();
        assert!(rect.width() > 0.0 && rect.height() > 0.0 && field.rect.contains(rect.center()), "actual AX hit target");
        field
    }
    fn checked(h: &Harness<'_, Rig>, key: &str, expected: bool) {
        checkbox(h, key);
        let name = name(h, key);
        let node = h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(name.as_str()));
        assert_eq!(node.accesskit_node().toggled(), Some(if expected { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False }));
    }
    fn click(h: &mut Harness<'_, Rig>, key: &str) {
        checkbox(h, key);
        let name = name(h, key);
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(name.as_str())).rect().center();
        pointer(h, pos, true);
        pointer(h, pos, false);
    }
    fn tab_to(h: &mut Harness<'_, Rig>, key_name: &str) {
        let field = checkbox(h, key_name);
        let name = name(h, key_name);
        let mut seen = HashSet::new();
        loop {
            let focused = {
                let current = checkbox(h, key_name);
                assert_eq!(current.id, field.id, "the actual target Checkbox remains current");
                assert!(!current.facts.as_ref().unwrap().disabled, "the current Checkbox target must be enabled");
                let control = h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(name.as_str()));
                let access = control.accesskit_node();
                assert!(!access.is_disabled(), "the actual named CheckBox AX member must be enabled");
                access.is_focused()
            };
            if focused {
                return;
            }
            // Native focus can belong to an arena root; cycle identities must be actual AX members.
            let focused_members: Vec<_> = {
                let root = h.queryable_node();
                std::iter::once(root.clone())
                    .chain(root.children_recursive())
                    .filter_map(|node| {
                        let access = node.accesskit_node();
                        access.is_focused().then(|| format!("{:?}", access.id()))
                    })
                    .collect()
            };
            assert!(seen.insert(focused_members.clone()), "natural Tab reaches {name}; repeated AX focus {focused_members:?}");
            key(h, Key::Tab);
        }
    }
    fn journal_one(h: &Harness<'_, Rig>, start: usize, key: &str, value: bool) {
        assert_eq!(&h.state().app.session.journal[start..], &[("object.attributes".to_string(), json!({key:value}))]);
    }

    #[test]
    fn installed_attributes_idle_keeps_real_model_identity_nonempty_undo_redo_and_four_ids() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        let controls: Vec<_> = ROWS.iter().map(|r| checkbox(&h, r.0).id).collect();
        assert_eq!(controls.iter().copied().collect::<HashSet<_>>().len(), 4);
        for (index, row) in ROWS.iter().enumerate() {
            checked(&h, row.0, flags(&h, ids[0])[index]);
        }
        idle(&mut h, 8);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        assert_eq!(controls, ROWS.iter().map(|r| checkbox(&h, r.0).id).collect::<Vec<_>>());
    }

    #[test]
    fn installed_attributes_each_pointer_action_has_exact_payload_one_undo_and_real_redo() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        for (index, (key, _)) in ROWS.iter().enumerate() {
            let before = mark(&h);
            let other = [item(&h, ids[1]), item(&h, ids[2])];
            let mut expected = item(&h, ids[0]);
            match index {
                0 => expected.fill.overprint = true,
                1 => expected.stroke.overprint = true,
                2 => expected.nonprinting = true,
                _ => expected.stroke.gap_overprint = true,
            }
            let start = h.state().app.session.journal.len();
            click(&mut h, key);
            journal_one(&h, start, key, true);
            assert_eq!(item(&h, ids[0]), expected);
            assert_eq!([item(&h, ids[1]), item(&h, ids[2])], other);
            assert_eq!(mark(&h).1, before.1);
            assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
            checked(&h, key, true);
            h.state_mut().app.run("edit.undo", json!({})).unwrap();
            idle(&mut h, 2);
            assert!(!flags(&h, ids[0])[index]);
            assert_eq!(mark(&h).2, (before.2.0, 1));
            checked(&h, key, false);
            h.state_mut().app.run("edit.redo", json!({})).unwrap();
            idle(&mut h, 2);
            assert_eq!(item(&h, ids[0]), expected);
            assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
            h.state_mut().app.run("edit.undo", json!({})).unwrap();
            idle(&mut h, 2);
        }
    }

    #[test]
    fn installed_attributes_natural_tab_space_and_enter_use_real_focus_once() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        for (index, (key_name, _)) in ROWS.iter().enumerate() {
            let start = h.state().app.session.journal.len();
            let before = mark(&h);
            tab_to(&mut h, key_name);
            unchanged(&h, &before);
            key(&mut h, if index % 2 == 0 { Key::Space } else { Key::Enter });
            journal_one(&h, start, key_name, true);
            assert!(flags(&h, ids[0])[index]);
            checked(&h, key_name, true);
            idle(&mut h, 3);
            assert_eq!(h.state().app.session.journal.len(), start + 1);
        }
    }

    #[test]
    fn installed_attributes_mixed_multiselection_keeps_first_item_display_and_targets_all_selected() {
        let (mut app, ids) = fixture();
        app.run("object.attributes", json!({"ids":[ids[1]],"overprintFill":true})).unwrap();
        app.run("selection.set", json!({"ids":[ids[0],ids[1]]})).unwrap();
        let mut h = harness(app, true);
        let before = mark(&h);
        let untouched = item(&h, ids[2]);
        checked(&h, "overprintFill", false);
        let start = h.state().app.session.journal.len();
        click(&mut h, "overprintFill");
        journal_one(&h, start, "overprintFill", true);
        assert!(flags(&h, ids[0])[0] && flags(&h, ids[1])[0]);
        assert_eq!(item(&h, ids[2]), untouched);
        assert_eq!(mark(&h).1, before.1);
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert!(!flags(&h, ids[0])[0] && flags(&h, ids[1])[0]);
        checked(&h, "overprintFill", false);
        h.state_mut().app.run("selection.set", json!({"ids":[ids[1],ids[0]]})).unwrap();
        idle(&mut h, 2);
        checked(&h, "overprintFill", true);
        let start = h.state().app.session.journal.len();
        click(&mut h, "overprintFill");
        journal_one(&h, start, "overprintFill", false);
        assert!(!flags(&h, ids[0])[0] && !flags(&h, ids[1])[0]);
        assert_eq!(item(&h, ids[2]), untouched);
    }

    #[test]
    fn installed_attributes_model_refresh_uses_current_boolean_without_a_draft_or_extra_command() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        let field = checkbox(&h, "overprintGap").id;
        h.state_mut().app.run("object.attributes", json!({"overprintGap":true})).unwrap();
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        idle(&mut h, 4);
        unchanged(&h, &before);
        checked(&h, "overprintGap", true);
        assert_eq!(checkbox(&h, "overprintGap").id, field);
        assert_eq!(h.state().app.session.journal.len(), start);
        click(&mut h, "overprintGap");
        journal_one(&h, start, "overprintGap", false);
        assert!(!flags(&h, ids[0])[3]);
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        checked(&h, "overprintGap", true);
    }

    #[test]
    fn installed_attributes_group_nonprinting_keeps_children_and_group_identity() {
        let (mut app, ids) = fixture();
        let group = app.run("object.group", json!({"ids":[ids[0],ids[1]]})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids":[group]})).unwrap();
        let mut h = harness(app, true);
        let before = mark(&h);
        let children = [item(&h, ids[0]), item(&h, ids[1])];
        let untouched = item(&h, ids[2]);
        let mut expected = item(&h, group);
        expected.nonprinting = true;
        let start = h.state().app.session.journal.len();
        click(&mut h, "nonprinting");
        journal_one(&h, start, "nonprinting", true);
        assert_eq!(item(&h, group), expected);
        assert_eq!([item(&h, ids[0]), item(&h, ids[1])], children);
        assert_eq!(item(&h, ids[2]), untouched);
        assert_eq!(mark(&h).1, before.1);
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert!(!item(&h, group).nonprinting);
        assert_eq!([item(&h, ids[0]), item(&h, ids[1])], children);
    }

    #[test]
    fn installed_attributes_held_press_cannot_follow_an_expanded_or_reordered_selection() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        for selected in [vec![ids[0], ids[1]], vec![ids[1], ids[0]]] {
            let old = checkbox(&h, "overprintFill");
            let label = name(&h, "overprintFill");
            let pos = h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(label.as_str())).rect().center();
            let press_start = h.state().app.session.journal.len();
            pointer(&mut h, pos, true);
            assert_eq!(h.state().app.session.journal.len(), press_start, "native-compatible activation waits for release");
            h.state_mut().app.run("selection.set", json!({"ids":selected})).unwrap();
            idle(&mut h, 2);
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            assert_ne!(checkbox(&h, "overprintFill").id, old.id);
            pointer(&mut h, pos, false);
            idle(&mut h, 2);
            unchanged(&h, &before);
            assert_eq!(h.state().app.session.journal.len(), start);
            assert!(!flags(&h, ids[0])[0] && !flags(&h, ids[1])[0]);
        }
        let start = h.state().app.session.journal.len();
        click(&mut h, "overprintFill");
        journal_one(&h, start, "overprintFill", true);
        assert!(flags(&h, ids[0])[0] && flags(&h, ids[1])[0]);
    }

    #[test]
    fn installed_attributes_held_press_cannot_cross_documents_with_identical_object_ids() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        let original = mark(&h);
        let old = checkbox(&h, "nonprinting");
        let label = name(&h, "nonprinting");
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(label.as_str())).rect().center();
        pointer(&mut h, pos, true);
        h.state_mut().app.run("file.new", json!({})).unwrap();
        let other = h.state_mut().app.run("frame.create", json!({"rect":[10,10,40,40]})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(other, ids[0], "real different documents reuse document-local object IDs");
        let dir = tempfile::tempdir().unwrap();
        h.state_mut().app.run("file.saveAs", json!({"path":dir.path().join("Other.designcraft")})).unwrap();
        idle(&mut h, 2);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        assert_ne!(checkbox(&h, "nonprinting").id, old.id);
        pointer(&mut h, pos, false);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        assert!(!flags(&h, other)[2]);
        h.state_mut().app.session.execute("file.close", &json!({})).unwrap();
        idle(&mut h, 2);
        unchanged(&h, &original);
        assert!(!flags(&h, ids[0])[2]);
        let start = h.state().app.session.journal.len();
        click(&mut h, "nonprinting");
        journal_one(&h, start, "nonprinting", true);
        assert!(flags(&h, ids[0])[2]);
    }

    #[test]
    fn installed_attributes_empty_selection_and_no_document_keep_the_real_engine_refusal() {
        let (app, _) = fixture();
        let mut h = harness(app, true);
        let label = name(&h, "overprintFill");
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(label.as_str())).rect().center();
        pointer(&mut h, pos, true);
        h.state_mut().app.run("selection.set", json!({"ids":[]})).unwrap();
        idle(&mut h, 2);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        assert_eq!(h.query_all_by_role(egui::accesskit::Role::CheckBox).count(), 0);
        pointer(&mut h, pos, false);
        key(&mut h, Key::Tab);
        key(&mut h, Key::Space);
        key(&mut h, Key::Enter);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        assert!(h.state_mut().app.run("object.attributes", json!({"overprintFill":true})).is_err());
        unchanged(&h, &before);
        let app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        let mut empty = harness(app, true);
        let start = empty.state().app.session.journal.len();
        assert_eq!(empty.query_all_by_role(egui::accesskit::Role::CheckBox).count(), 0);
        key(&mut empty, Key::Tab);
        key(&mut empty, Key::Space);
        key(&mut empty, Key::Enter);
        idle(&mut empty, 3);
        assert!(empty.state().app.session.active().is_none());
        assert_eq!(empty.state().app.session.journal.len(), start);
    }

    #[test]
    fn installed_attributes_disabled_and_sizing_passes_cannot_commit_then_recover() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        let before = mark(&h);
        let label = name(&h, "overprintFill");
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(label.as_str())).rect().center();
        let start = h.state().app.session.journal.len();
        pointer(&mut h, pos, true);
        h.state_mut().enabled = false;
        idle(&mut h, 2);
        for row in ROWS {
            assert!(checkbox(&h, row.0).facts.as_ref().unwrap().disabled);
            let label = name(&h, row.0);
            assert!(
                h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(label.as_str()))
                    .accesskit_node()
                    .is_disabled()
            );
        }
        pointer(&mut h, pos, false);
        key(&mut h, Key::Space);
        key(&mut h, Key::Enter);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        h.state_mut().enabled = true;
        h.state_mut().sizing = true;
        idle(&mut h, 2);
        pointer(&mut h, pos, true);
        pointer(&mut h, pos, false);
        key(&mut h, Key::Space);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        h.state_mut().sizing = false;
        idle(&mut h, 2);
        click(&mut h, "overprintStroke");
        journal_one(&h, start, "overprintStroke", true);
        assert_eq!(flags(&h, ids[0]), [false, true, false, false]);
    }

    #[test]
    fn installed_attributes_german_and_arabic_names_remain_complete_and_operable() {
        for language in ["de", "ar"] {
            let (mut app, ids) = fixture();
            app.ui.language = language.to_owned();
            let mut h = harness(app, true);
            let before = mark(&h);
            let mut seen = HashSet::new();
            for row in ROWS {
                let field = checkbox(&h, row.0);
                assert!(seen.insert(field.id));
                checked(&h, row.0, false);
            }
            idle(&mut h, 2);
            unchanged(&h, &before);
            let start = h.state().app.session.journal.len();
            tab_to(&mut h, "overprintGap");
            key(&mut h, Key::Space);
            journal_one(&h, start, "overprintGap", true);
            assert!(flags(&h, ids[0])[3]);
            checked(&h, "overprintGap", true);
        }
    }

    #[test]
    fn installed_attributes_saved_document_reopens_with_all_four_flags_and_unselected_item_intact() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        let untouched = item(&h, ids[2]);
        for row in ROWS {
            let start = h.state().app.session.journal.len();
            click(&mut h, row.0);
            journal_one(&h, start, row.0, true);
        }
        assert_eq!(flags(&h, ids[0]), [true; 4]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Attributes & α.designcraft");
        h.state_mut().app.run("file.saveAs", json!({"path":path})).unwrap();
        assert!(path.is_file());
        let mut reopened = designcraft_engine::Session::new();
        reopened.execute("file.open", &json!({"path":path})).unwrap();
        let doc = &reopened.active().unwrap().doc;
        let it = doc.item(designcraft_doc::ItemId(ids[0])).unwrap();
        assert!(it.fill.overprint && it.stroke.overprint && it.nonprinting && it.stroke.gap_overprint);
        assert_eq!(doc.item(designcraft_doc::ItemId(ids[2])).unwrap(), &untouched);
    }

    #[test]
    fn uninstalled_attributes_retains_native_checkbox_input_and_exact_commands() {
        let (app, ids) = fixture();
        let mut h = harness(app, false);
        assert!(h.state().controls.as_ref().is_none_or(|recorded| recorded.elements.iter().all(|e| e.component != "Checkbox")));
        for (index, row) in ROWS.iter().enumerate() {
            let label = name(&h, row.0);
            let pos = h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(label.as_str())).rect().center();
            let start = h.state().app.session.journal.len();
            pointer(&mut h, pos, true);
            pointer(&mut h, pos, false);
            journal_one(&h, start, row.0, true);
            assert!(flags(&h, ids[0])[index]);
        }
        assert_eq!(flags(&h, ids[0]), [true; 4]);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui223_text_wrap_mode_invert_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use designcraft_doc::{Content, DocError, Document, ItemId, WrapMode};
    use egui::{Event, Key, Modifiers, PointerButton, UiBuilder};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use serde_json::Value;
    use std::{collections::HashSet, sync::Arc};

    const MODES: [(&str, &str); 5] = [
        ("none", "No Text Wrap"),
        ("boundingBox", "Wrap Around Bounding Box"),
        ("contour", "Wrap Around Object Shape"),
        ("jumpObject", "Jump Object"),
        ("jumpToNextColumn", "Jump to Next Column"),
    ];

    struct Rig {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        enabled: bool,
        sizing: bool,
        visible: bool,
        height: f32,
        viewport: Option<egui::Rect>,
        offset: egui::Vec2,
        content: egui::Vec2,
    }
    struct Snapshot {
        doc: Arc<Document>,
        saved: Arc<Document>,
        owner: (u64, u64),
        revision: (u64, u64),
        selection: String,
        history: (usize, usize),
        journal: usize,
    }

    fn frame(h: &mut Harness<'_, Rig>, event: Option<Event>) {
        let mut raw = std::mem::take(h.input_mut());
        if let Some(event) = event {
            raw.events.push(event);
        }
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, Rig>, frames: usize) {
        for _ in 0..frames {
            frame(h, None);
        }
    }
    fn pointer(h: &mut Harness<'_, Rig>, at: egui::Pos2, pressed: bool) {
        frame(h, Some(Event::PointerMoved(at)));
        frame(h, Some(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
    }
    fn key(h: &mut Harness<'_, Rig>, key: Key) {
        for pressed in [true, false] {
            frame(h, Some(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }));
        }
        idle(h, 2);
    }
    fn harness(mut app: DesignApp, installed: bool, width: f32) -> Harness<'static, Rig> {
        app.framework_transform = false;
        app.framework_opacity = installed;
        let ctx = egui::Context::default();
        if installed {
            app.install_framework(&ctx);
        }
        assert_eq!(app.framework_installed, installed);
        app.egui_ctx = Some(ctx.clone());
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut fonts = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(width, 320.0)).with_step_dt(0.05).build_ui_state(
            move |ui, state: &mut Rig| {
                if !fonts {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    fonts = true;
                    return;
                }
                state.controls = None;
                state.viewport = None;
                if !state.visible {
                    return;
                }
                let builder = if state.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(state.enabled, |ui| {
                        state.controls = record(ui, egui::Id::unique("text-wrap-mode-invert-workflow"), |ui| {
                            // This is the shipping panel-body dispatcher and the real Dock body's scroll/margin policy.
                            egui::Frame::NONE.inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                                let output = egui::ScrollArea::vertical()
                                    .id_salt(("shared-panel-body", "textWrap"))
                                    .max_height(state.height)
                                    .auto_shrink([false, true])
                                    .show(ui, |ui| crate::dock::panel_body(&mut state.app, ui, "textWrap"));
                                state.viewport = Some(output.inner_rect);
                                state.offset = output.state.offset();
                                state.content = output.content_size;
                            });
                        })
                        .1;
                    });
                });
            },
            Rig {
                app,
                controls: None,
                enabled: true,
                sizing: false,
                visible: true,
                height: 240.0,
                viewport: None,
                offset: egui::Vec2::ZERO,
                content: egui::Vec2::ZERO,
            },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }
    fn fixture() -> (DesignApp, [u64; 3]) {
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let mut ids = Vec::new();
        for rect in [[10, 10, 40, 40], [60, 10, 90, 40], [110, 10, 140, 40]] {
            ids.push(session.execute("frame.create", &json!({"rect": rect})).unwrap()["id"].as_u64().unwrap());
        }
        session
            .execute("object.textWrap", &json!({"ids":[ids[0]], "mode":"contour", "offset":[1,2,3,4], "side":"leftSide", "contour":"graphicFrame"}))
            .unwrap();
        session
            .execute("object.textWrap", &json!({"ids":[ids[1]], "mode":"jumpObject", "invert":true, "offset":[5,6,7,8], "side":"rightSide"}))
            .unwrap();
        session.execute("object.textWrap", &json!({"ids":[ids[2]], "mode":"boundingBox", "invert":true, "offset":9})).unwrap();
        session.execute("object.attributes", &json!({"ids":[ids[2]], "nonprinting":true})).unwrap();
        // Real imported-boundary state: mode/Invert must not clear it like an offset/contour edit would.
        session
            .edit(|doc, _| {
                let it = doc.item_mut(ItemId(ids[0])).ok_or(DocError::NoItem(ItemId(ids[0])))?;
                it.wrap.path = Some(it.path.clone());
                Ok(())
            })
            .unwrap();
        session.execute("selection.set", &json!({"ids":[ids[0],ids[1]]})).unwrap();
        session.execute("frame.create", &json!({"rect":[160,10,190,40]})).unwrap();
        session.execute("edit.undo", &json!({})).unwrap();
        assert!(!session.active().unwrap().history.undo.is_empty());
        assert_eq!(session.active().unwrap().history.redo.len(), 1);
        (DesignApp::new_for_panel_tests(session, crate::Services::default()), ids.try_into().unwrap())
    }
    fn graphic_fixture() -> (DesignApp, [u64; 3]) {
        let (mut app, ids) = fixture();
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
        app.run("file.place", json!({"base64": designcraft_engine::cmd::base64_encode(&png), "name":"wrap-target.png", "frame":ids[0]})).unwrap();
        app.run("selection.set", json!({"ids":[ids[0]], "content":true})).unwrap();
        app.run("object.textWrap", json!({"mode":"jumpObject", "offset":[11,12,13,14], "invert":false, "side":"rightSide"})).unwrap();
        assert_eq!(app.session.active().unwrap().doc.item(ItemId(ids[0])).unwrap().wrap.mode, WrapMode::Contour);
        (app, ids)
    }
    fn snapshot(h: &Harness<'_, Rig>) -> Snapshot {
        let st = h.state().app.session.active().unwrap();
        Snapshot {
            doc: st.doc.clone(),
            saved: st.saved_doc.clone(),
            owner: (st.uid, st.identity_epoch),
            revision: (st.revision, st.saved_revision),
            selection: format!("{:?}", st.selection),
            history: (st.history.undo.len(), st.history.redo.len()),
            journal: h.state().app.session.journal.len(),
        }
    }
    fn unchanged(h: &Harness<'_, Rig>, old: &Snapshot) {
        let now = snapshot(h);
        assert!(Arc::ptr_eq(&old.doc, &now.doc), "refused/retired/idle input must not COW the document");
        assert!(Arc::ptr_eq(&old.saved, &now.saved));
        assert_eq!(
            (now.owner, now.revision, now.selection, now.history, now.journal),
            (old.owner, old.revision, old.selection.clone(), old.history, old.journal)
        );
    }
    fn domain(h: &Harness<'_, Rig>, expected: &Document, owner: &Snapshot, history: (usize, usize)) {
        let now = snapshot(h);
        assert_eq!(now.doc.as_ref(), expected, "complete document, not only the displayed first item");
        assert_eq!((now.owner, now.selection, now.history), (owner.owner, owner.selection.clone(), history));
        assert!(Arc::ptr_eq(&now.saved, &owner.saved));
    }
    fn expected(doc: &Document, ids: &[u64], content: bool, mode: Option<&str>, invert: Option<bool>) -> Document {
        let mut doc = doc.clone();
        for id in ids {
            let it = doc.item_mut(ItemId(*id)).unwrap();
            let wrap = if content {
                let Content::Graphic(graphic) = &mut it.content else { panic!("the real placed graphic is the target") };
                &mut graphic.wrap
            } else {
                &mut it.wrap
            };
            if let Some(mode) = mode {
                wrap.mode = serde_json::from_value(json!(mode)).unwrap();
            }
            if let Some(invert) = invert {
                wrap.invert = invert;
            }
        }
        doc
    }
    fn calls(h: &Harness<'_, Rig>, from: usize) -> Vec<Value> {
        h.state().app.session.journal[from..].iter().filter(|(id, _)| id == "object.textWrap").map(|(_, params)| params.clone()).collect()
    }
    fn label(h: &Harness<'_, Rig>, action: &str) -> String {
        let english = if action == "invert" { "Invert" } else { MODES.iter().find(|(mode, _)| *mode == action).unwrap().1 };
        crate::i18n::tr(&h.state().app.ui.language, english).to_owned()
    }
    fn role(action: &str) -> egui::accesskit::Role {
        if action == "invert" { egui::accesskit::Role::CheckBox } else { egui::accesskit::Role::Button }
    }
    fn position(h: &Harness<'_, Rig>, action: &str) -> egui::Pos2 {
        let name = label(h, action);
        assert_eq!(h.query_all_by(|n| n.role() == role(action) && n.label().as_deref() == Some(name.as_str())).count(), 1, "one full AX label");
        let rect = h.get_by(|n| n.role() == role(action) && n.label().as_deref() == Some(name.as_str())).rect();
        assert!(rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0);
        assert!(h.state().viewport.unwrap().contains(rect.center()), "the actual control must be visible, not a fabricated offscreen click");
        rect.center()
    }
    fn control(h: &Harness<'_, Rig>, action: &str) -> Element {
        let name = label(h, action);
        let component = if action == "invert" { "Checkbox" } else { "Button" };
        let found: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.component == component && e.part.is_none() && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some(name.as_str())))
            .cloned()
            .collect();
        assert_eq!(found.len(), 1, "one real public SDK {component} for the complete name {name}");
        let field = found.into_iter().next().unwrap();
        assert_eq!(field.facts.as_ref().unwrap().role, format!("{:?}", role(action)));
        assert!(field.rect.is_finite() && field.rect.width() > 0.0 && field.rect.height() > 0.0);
        field
    }
    fn click(h: &mut Harness<'_, Rig>, action: &str) {
        let at = position(h, action);
        pointer(h, at, true);
        pointer(h, at, false);
        idle(h, 2);
    }
    fn shown(h: &Harness<'_, Rig>, mode: &str, invert: bool, disabled: bool) {
        for (key, _) in MODES {
            let name = label(h, key);
            let node = h.get_by(|n| n.role() == role(key) && n.label().as_deref() == Some(name.as_str()));
            assert_eq!(node.accesskit_node().is_selected().unwrap_or(false), key == mode);
        }
        let name = label(h, "invert");
        let node = h.get_by(|n| n.role() == role("invert") && n.label().as_deref() == Some(name.as_str()));
        assert_eq!(node.accesskit_node().toggled(), Some(if invert { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False }));
        assert_eq!(node.accesskit_node().is_disabled(), disabled);
    }
    /// The actual retained native icon button uses WidgetInfo::selected -> AccessKit Toggled,
    /// not the installed SDK button's is_selected property. Keep both exact contracts strict.
    fn native_shown(h: &Harness<'_, Rig>, mode: &str, invert: bool, disabled: bool) {
        assert!(!h.state().app.framework_installed);
        for (key, _) in MODES {
            let name = label(h, key);
            assert_eq!(
                h.query_all_by(|n| n.role() == role(key) && n.label().as_deref() == Some(name.as_str())).count(),
                1,
                "one full native mode identity"
            );
            let node = h.get_by(|n| n.role() == role(key) && n.label().as_deref() == Some(name.as_str()));
            assert_eq!(node.accesskit_node().is_selected(), None, "the actual native route does not publish the SDK-only property");
            assert_eq!(
                node.accesskit_node().toggled(),
                Some(if key == mode { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False }),
                "the exact real native selected mode, not an absent-property false default"
            );
        }
        let name = label(h, "invert");
        assert_eq!(
            h.query_all_by(|n| n.role() == role("invert") && n.label().as_deref() == Some(name.as_str())).count(),
            1,
            "one full native Invert identity"
        );
        let node = h.get_by(|n| n.role() == role("invert") && n.label().as_deref() == Some(name.as_str()));
        assert_eq!(node.accesskit_node().toggled(), Some(if invert { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False }));
        assert_eq!(node.accesskit_node().is_disabled(), disabled);
    }
    fn tab_to(h: &mut Harness<'_, Rig>, action: &str) {
        let name = label(h, action);
        let id = control(h, action).id;
        let mut seen = HashSet::new();
        loop {
            assert_eq!(control(h, action).id, id);
            let node = h.get_by(|n| n.role() == role(action) && n.label().as_deref() == Some(name.as_str()));
            assert!(!node.accesskit_node().is_disabled());
            if node.accesskit_node().is_focused() {
                return;
            }
            let focused: Vec<_> = {
                let root = h.queryable_node();
                std::iter::once(root.clone())
                    .chain(root.children_recursive())
                    .filter_map(|n| n.accesskit_node().is_focused().then(|| format!("{:?}", n.accesskit_node().id())))
                    .collect()
            };
            assert!(seen.insert(focused.clone()), "natural Tab must reach {name} before repeating actual AX focus {focused:?}");
            key(h, Key::Tab);
        }
    }
    fn undo(h: &mut Harness<'_, Rig>) {
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(h, 2);
    }

    #[test]
    fn installed_mode_then_invert_then_undo_redo_preserves_the_complete_document_and_idle_identity() {
        let (app, ids) = fixture();
        let mut h = harness(app, true, 360.0);
        let before = snapshot(&h);
        let original_ids: Vec<_> = MODES.iter().map(|(mode, _)| control(&h, mode).id).chain([control(&h, "invert").id]).collect();
        assert_eq!(original_ids.iter().copied().collect::<HashSet<_>>().len(), 6);
        shown(&h, "contour", false, false);
        click(&mut h, "boundingBox");
        let modes = expected(&before.doc, &ids[..2], false, Some("boundingBox"), None);
        domain(&h, &modes, &before, (before.history.0 + 1, 0));
        shown(&h, "boundingBox", false, false);
        click(&mut h, "invert");
        let inverted = expected(&modes, &ids[..2], false, Some("boundingBox"), Some(true));
        domain(&h, &inverted, &before, (before.history.0 + 2, 0));
        shown(&h, "boundingBox", true, false);
        assert_eq!(calls(&h, before.journal), [json!({"mode":"boundingBox"}), json!({"mode":"boundingBox","invert":true})]);
        let after = snapshot(&h);
        idle(&mut h, 4);
        unchanged(&h, &after);
        assert_eq!(original_ids, MODES.iter().map(|(mode, _)| control(&h, mode).id).chain([control(&h, "invert").id]).collect::<Vec<_>>());
        undo(&mut h);
        domain(&h, &modes, &before, (before.history.0 + 1, 1));
        undo(&mut h);
        domain(&h, &before.doc, &before, (before.history.0, 2));
        shown(&h, "contour", false, false);
        for _ in 0..2 {
            h.state_mut().app.run("edit.redo", json!({})).unwrap();
            idle(&mut h, 2);
        }
        domain(&h, &inverted, &before, (before.history.0 + 2, 0));
        assert_eq!(calls(&h, before.journal).len(), 2, "Undo/Redo is not replayed UI input");
    }

    #[test]
    fn installed_all_five_mode_actions_keep_offsets_custom_boundary_and_unselected_items_with_one_undo_each() {
        for (mode, _) in MODES {
            let (app, ids) = fixture();
            let mut h = harness(app, true, 280.0);
            let before = snapshot(&h);
            for (action, _) in MODES {
                let button = control(&h, action);
                let center = position(&h, action);
                assert_eq!(button.rect.size(), vec2(26.0, 26.0), "the full labels are not painted as wide mode buttons");
                assert!(button.rect.contains(center));
            }
            click(&mut h, mode);
            let changed = expected(&before.doc, &ids[..2], false, Some(mode), None);
            domain(&h, &changed, &before, (before.history.0 + 1, 0));
            shown(&h, mode, false, mode == "none");
            assert_eq!(calls(&h, before.journal), [json!({"mode":mode})]);
            let settled = snapshot(&h);
            idle(&mut h, 4);
            unchanged(&h, &settled);
            undo(&mut h);
            domain(&h, &before.doc, &before, (before.history.0, 1));
        }
    }

    #[test]
    fn installed_natural_tab_enter_and_space_drive_mode_invert_and_two_real_undos() {
        let (app, ids) = fixture();
        let mut h = harness(app, true, 360.0);
        let before = snapshot(&h);
        tab_to(&mut h, "jumpToNextColumn");
        key(&mut h, Key::Enter);
        let changed = expected(&before.doc, &ids[..2], false, Some("jumpToNextColumn"), None);
        domain(&h, &changed, &before, (before.history.0 + 1, 0));
        tab_to(&mut h, "invert");
        key(&mut h, Key::Space);
        let inverted = expected(&changed, &ids[..2], false, Some("jumpToNextColumn"), Some(true));
        domain(&h, &inverted, &before, (before.history.0 + 2, 0));
        assert_eq!(calls(&h, before.journal), [json!({"mode":"jumpToNextColumn"}), json!({"mode":"jumpToNextColumn","invert":true})]);
        undo(&mut h);
        domain(&h, &changed, &before, (before.history.0 + 1, 1));
        undo(&mut h);
        domain(&h, &before.doc, &before, (before.history.0, 2));
    }

    #[test]
    fn installed_real_graphic_content_and_frame_are_distinct_live_mode_invert_targets_with_full_undo() {
        let (app, ids) = graphic_fixture();
        let mut h = harness(app, true, 360.0);
        let content = snapshot(&h);
        let content_id = control(&h, "boundingBox").id;
        shown(&h, "jumpObject", false, false);
        click(&mut h, "boundingBox");
        click(&mut h, "invert");
        let changed = expected(&content.doc, &ids[..1], true, Some("boundingBox"), Some(true));
        domain(&h, &changed, &content, (content.history.0 + 2, 0));
        assert_eq!(calls(&h, content.journal), [json!({"mode":"boundingBox"}), json!({"mode":"boundingBox","invert":true})]);
        undo(&mut h);
        undo(&mut h);
        domain(&h, &content.doc, &content, (content.history.0, 2));
        h.state_mut().app.run("selection.set", json!({"ids":[ids[0]],"content":false})).unwrap();
        idle(&mut h, 2);
        let frame = snapshot(&h);
        assert_ne!(content_id, control(&h, "boundingBox").id);
        shown(&h, "contour", false, false);
        click(&mut h, "jumpToNextColumn");
        click(&mut h, "invert");
        let changed = expected(&frame.doc, &ids[..1], false, Some("jumpToNextColumn"), Some(true));
        domain(&h, &changed, &frame, (frame.history.0 + 2, 0));
        undo(&mut h);
        undo(&mut h);
        domain(&h, &frame.doc, &frame, (frame.history.0, 2));
    }

    #[test]
    fn installed_held_mode_and_invert_presses_retire_on_selection_content_document_and_same_uid_epoch_replacement() {
        for action in ["boundingBox", "invert"] {
            for change in ["selection", "content", "document", "epoch", "no-selection"] {
                let (app, ids) = graphic_fixture();
                let mut h = harness(app, true, 360.0);
                let original = snapshot(&h);
                let old = control(&h, action);
                let at = position(&h, action);
                pointer(&mut h, at, true);
                match change {
                    "selection" => {
                        h.state_mut().app.run("selection.set", json!({"ids":[ids[2]]})).unwrap();
                    }
                    "content" => {
                        h.state_mut().app.run("selection.set", json!({"ids":[ids[0]],"content":false})).unwrap();
                    }
                    "document" => {
                        h.state_mut().app.run("file.new", json!({})).unwrap();
                        let other = h.state_mut().app.run("frame.create", json!({"rect":[10,10,40,40]})).unwrap()["id"].as_u64().unwrap();
                        assert_eq!(other, ids[0], "document-local IDs really are reused");
                    }
                    "epoch" => {
                        let old = h.state().app.session.active().unwrap();
                        let mut replacement = designcraft_engine::DocState::new((*old.doc).clone(), old.path.clone());
                        replacement.uid = old.uid;
                        replacement.selection = old.selection.clone();
                        assert_ne!(replacement.identity_epoch, old.identity_epoch);
                        *h.state_mut().app.session.active_mut().unwrap() = replacement;
                    }
                    "no-selection" => {
                        h.state_mut().app.run("selection.set", json!({"ids":[]})).unwrap();
                    }
                    _ => unreachable!(),
                }
                let retired = snapshot(&h);
                idle(&mut h, 2);
                unchanged(&h, &retired);
                assert_ne!(control(&h, action).id, old.id);
                pointer(&mut h, at, false);
                idle(&mut h, 2);
                unchanged(&h, &retired);
                assert!(calls(&h, retired.journal).is_empty());
                if change == "document" {
                    h.state_mut().app.session.execute("file.close", &json!({})).unwrap();
                    idle(&mut h, 2);
                    domain(&h, &original.doc, &original, original.history);
                }
            }
        }
    }

    #[test]
    fn installed_hidden_disabled_and_sizing_parents_cannot_commit_held_or_keyboard_input_then_recover() {
        for held_action in ["boundingBox", "invert"] {
            for phase in ["hidden", "disabled", "sizing"] {
                let (app, ids) = fixture();
                let mut h = harness(app, true, 360.0);
                let before = snapshot(&h);
                let at = position(&h, held_action);
                pointer(&mut h, at, true);
                match phase {
                    "hidden" => h.state_mut().visible = false,
                    "disabled" => h.state_mut().enabled = false,
                    _ => h.state_mut().sizing = true,
                }
                idle(&mut h, 2);
                if phase == "disabled" {
                    for action in ["none", "boundingBox", "contour", "jumpObject", "jumpToNextColumn", "invert"] {
                        assert!(control(&h, action).facts.as_ref().unwrap().disabled);
                        let name = label(&h, action);
                        assert!(h.get_by(|n| n.role() == role(action) && n.label().as_deref() == Some(name.as_str())).accesskit_node().is_disabled());
                    }
                } else {
                    assert!(h.state().controls.is_none(), "no stale record is reused for hidden/measuring input");
                }
                pointer(&mut h, at, false);
                key(&mut h, Key::Space);
                key(&mut h, Key::Enter);
                idle(&mut h, 2);
                unchanged(&h, &before);
                h.state_mut().visible = true;
                h.state_mut().enabled = true;
                h.state_mut().sizing = false;
                idle(&mut h, 2);
                unchanged(&h, &before);
                click(&mut h, "boundingBox");
                let changed = expected(&before.doc, &ids[..2], false, Some("boundingBox"), None);
                domain(&h, &changed, &before, (before.history.0 + 1, 0));
                assert_eq!(calls(&h, before.journal), [json!({"mode":"boundingBox"})]);
            }
        }
    }

    #[test]
    fn installed_closing_the_document_retires_both_held_input_types_without_recreating_an_owner() {
        for action in ["boundingBox", "invert"] {
            let (app, _) = fixture();
            let mut h = harness(app, true, 360.0);
            let old = control(&h, action).id;
            let at = position(&h, action);
            pointer(&mut h, at, true);
            h.state_mut().app.session.execute("file.close", &json!({})).unwrap();
            let journal = h.state().app.session.journal.len();
            idle(&mut h, 2);
            assert!(h.state().app.session.active().is_none());
            assert_ne!(control(&h, action).id, old);
            pointer(&mut h, at, false);
            key(&mut h, Key::Space);
            key(&mut h, Key::Enter);
            idle(&mut h, 2);
            assert!(h.state().app.session.active().is_none());
            assert_eq!(h.state().app.session.journal.len(), journal);
            assert!(calls(&h, journal).is_empty());
        }
    }

    #[test]
    fn installed_none_mode_disables_invert_without_clearing_its_stored_value_or_dispatching_a_second_edit() {
        let (mut app, ids) = fixture();
        app.run("selection.set", json!({"ids":[ids[1]]})).unwrap();
        let mut h = harness(app, true, 360.0);
        let before = snapshot(&h);
        click(&mut h, "none");
        let none = expected(&before.doc, &ids[1..2], false, Some("none"), None);
        domain(&h, &none, &before, (before.history.0 + 1, 0));
        shown(&h, "none", true, true);
        assert!(control(&h, "invert").facts.as_ref().unwrap().disabled);
        let disabled = snapshot(&h);
        click(&mut h, "invert");
        unchanged(&h, &disabled);
        assert_eq!(calls(&h, before.journal), [json!({"mode":"none"})]);
        undo(&mut h);
        domain(&h, &before.doc, &before, (before.history.0, 1));
        shown(&h, "jumpObject", true, false);
    }

    #[test]
    fn installed_no_selection_and_no_document_keep_the_actual_engine_refusal_and_never_create_history() {
        let (mut app, _) = fixture();
        app.run("selection.set", json!({"ids":[]})).unwrap();
        let mut h = harness(app, true, 360.0);
        let before = snapshot(&h);
        shown(&h, "none", false, true);
        assert!(!control(&h, "boundingBox").facts.as_ref().unwrap().disabled, "preserve existing active mode/refused-command policy");
        click(&mut h, "boundingBox");
        unchanged(&h, &before);
        assert!(h.state_mut().app.run("object.textWrap", json!({"mode":"boundingBox"})).is_err());
        unchanged(&h, &before);
        let mut empty = harness(DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default()), true, 360.0);
        let journal = empty.state().app.session.journal.len();
        shown(&empty, "none", false, true);
        click(&mut empty, "boundingBox");
        assert!(empty.state().app.session.active().is_none());
        assert_eq!(empty.state().app.session.journal.len(), journal);
    }

    #[test]
    fn installed_real_parent_wheel_moves_the_same_physical_controls_without_model_edits_then_input_still_works() {
        let (app, ids) = fixture();
        let mut h = harness(app, true, 360.0);
        h.state_mut().height = 72.0;
        idle(&mut h, 2);
        let viewport = h.state().viewport.unwrap();
        assert!(h.state().content.y > viewport.height(), "real body overflow is the wheel precondition");
        let old = control(&h, "boundingBox");
        let before = snapshot(&h);
        frame(&mut h, Some(Event::PointerMoved(viewport.center())));
        frame(
            &mut h,
            Some(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -100.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Modifiers::NONE,
            }),
        );
        idle(&mut h, 2);
        let down = control(&h, "boundingBox");
        assert_eq!(down.id, old.id);
        assert!(h.state().offset.y > 0.0);
        assert!(down.rect.top() < old.rect.top());
        unchanged(&h, &before);
        frame(
            &mut h,
            Some(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 100.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Modifiers::NONE,
            }),
        );
        idle(&mut h, 2);
        assert_eq!(control(&h, "boundingBox").id, old.id);
        unchanged(&h, &before);
        click(&mut h, "boundingBox");
        let changed = expected(&before.doc, &ids[..2], false, Some("boundingBox"), None);
        domain(&h, &changed, &before, (before.history.0 + 1, 0));
        undo(&mut h);
        domain(&h, &before.doc, &before, (before.history.0, 1));
    }

    #[test]
    fn uninstalled_native_mode_invert_and_undo_keep_the_actual_artwork_and_engine_route() {
        let (app, ids) = fixture();
        let mut h = harness(app, false, 360.0);
        let before = snapshot(&h);
        assert!(h.state().controls.as_ref().unwrap().elements.iter().all(|e| e.component != "Button" && e.component != "Checkbox"));
        native_shown(&h, "contour", false, false);
        click(&mut h, "boundingBox");
        click(&mut h, "invert");
        let changed = expected(&before.doc, &ids[..2], false, Some("boundingBox"), Some(true));
        domain(&h, &changed, &before, (before.history.0 + 2, 0));
        native_shown(&h, "boundingBox", true, false);
        assert_eq!(calls(&h, before.journal), [json!({"mode":"boundingBox"}), json!({"mode":"boundingBox","invert":true})]);
        undo(&mut h);
        undo(&mut h);
        domain(&h, &before.doc, &before, (before.history.0, 2));
    }

    #[test]
    fn installed_german_full_names_remain_compact_real_parent_controls_and_drive_the_complete_workflow() {
        let (mut app, ids) = fixture();
        app.ui.language = "de".to_owned();
        let mut h = harness(app, true, 360.0);
        let before = snapshot(&h);
        let mut rectangles = Vec::new();
        let translated =
            ["Kein Textumfluss", "Umfluss um Begrenzungsrahmen", "Umfluss um Objektform", "Objekt überspringen", "In nächste Spalte springen"];
        for ((mode, english), expected_name) in MODES.into_iter().zip(translated) {
            let name = label(&h, mode);
            assert_eq!(name, expected_name, "the full actual German translation, not a fallback");
            assert_ne!(name, english);
            let button = control(&h, mode);
            assert_eq!(button.rect.size(), vec2(26.0, 26.0));
            position(&h, mode);
            rectangles.push(button.rect);
        }
        for pair in rectangles.windows(2) {
            assert!(!pair[0].intersects(pair[1]), "five actual distinct compact physical inputs");
        }
        assert_eq!(label(&h, "invert"), "Umkehren");
        control(&h, "invert");
        position(&h, "invert");
        click(&mut h, "boundingBox");
        click(&mut h, "invert");
        let changed = expected(&before.doc, &ids[..2], false, Some("boundingBox"), Some(true));
        domain(&h, &changed, &before, (before.history.0 + 2, 0));
        undo(&mut h);
        undo(&mut h);
        domain(&h, &before.doc, &before, (before.history.0, 2));
    }
}
