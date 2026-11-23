use super::{
    CornerShape, DesignApp, FIELD_H, NumField, Rect, SelInfo, Sense, Stroke, StrokeKind, Tokens, Ui, Unit, Value, block, full_width, icon_toggle,
    icons, json, link_label, measure, more_options, outline_button, place, pos2, set_temp_bool, show_panel, sub, temp_bool, units, vec2, widgets,
};

// ---------------------------------------------------------------- Objects
//
// Every section here has two bodies. With the craft UI framework's runtime on the context (the desktop app) the
// section is drawn whole by the framework (`framework` below): every control is a framework component, so Tab, Escape
// and focus run through one order. The browser build has no framework, and a context without its runtime takes no
// framework input, so there the native body below draws the same controls over the same engine commands.

/// The stroke types Properties › Appearance offers, as InDesign's list names them, with their engine kinds.
const STROKE_TYPES: &[(&str, &str)] = &[
    ("Solid", "solid"),
    ("Dashed", "dashed"),
    ("Dotted", "dotted"),
    ("Thick - Thin", "thickThin"),
    ("Thin - Thick", "thinThick"),
    ("Thin - Thin", "thinThin"),
    ("Thick - Thick", "thickThick"),
];

/// The stroke weights the weight field's presets list offers, in points.
const STROKE_WEIGHTS: &[f64] = &[0.0, 0.25, 0.5, 0.75, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 20.0, 30.0, 40.0, 50.0, 100.0];

/// The `object.stroke` params that set the stroke type `kind`: a dashed stroke takes InDesign's default dashes.
fn stroke_type_params(kind: &str) -> Value {
    let ty = if kind == "dashed" { json!({"kind": "dashed", "pattern": [12.0, 4.0]}) } else { json!({"kind": kind}) };
    json!({"type": ty})
}

/// The `object.cornerOptions` params a picked corner shape runs with: the current size, or InDesign's 12 pt for
/// corners that had none.
fn corner_shape_params(shape: CornerShape, size: f64) -> Value {
    json!({"shape": shape, "size": if size > 0.0 { size } else { 12.0 }})
}

/// The `object.cornerOptions` params a typed corner size runs with: square corners become rounded ones.
fn corner_size_params(shape: CornerShape, size: f64) -> Value {
    let shape = if shape == CornerShape::None { CornerShape::Rounded } else { shape };
    json!({"shape": shape, "size": size})
}

/// Transform's angle and scale rows: icon, its tooltip, the `transform.info` key, the shown suffix and the native id.
const ANGLE_ROWS: [[(&str, &str, &str, &str, &str); 2]; 2] = [
    [("tool-rotate", "Rotation Angle", "rotation", "°", "p_rot"), ("tool-scale", "Scale X Percentage", "scaleX", "%", "p_sx")],
    [("tool-shear", "Shear X Angle", "shear", "°", "p_shear"), ("tool-scale", "Scale Y Percentage", "scaleY", "%", "p_sy")],
];

/// The quarter turns and flips under Transform's angles: icon, name, command and its params.
fn rotate_flip() -> [(&'static str, &'static str, &'static str, Value); 4] {
    [
        ("rotate-clockwise", "Rotate 90° Clockwise", "transform.rotate", json!({"angle": -90})),
        ("tool-rotate", "Rotate 90° Counterclockwise", "transform.rotate", json!({"angle": 90})),
        ("flip-horizontal", "Flip Horizontal", "transform.flip", json!({"axis": "horizontal"})),
        ("flip-vertical", "Flip Vertical", "transform.flip", json!({"axis": "vertical"})),
    ]
}

/// The Align To menu's rows: label and target.
const ALIGN_TO: [(&str, &str); 5] = [
    ("Align to Selection", "selection"),
    ("Align to Key Object", "keyObject"),
    ("Align to Margins", "margins"),
    ("Align to Page", "page"),
    ("Align to Spread", "spread"),
];

/// The align buttons after Align To: edge, icon and name.
const ALIGN_EDGES: [(&str, &str, &str); 6] = [
    ("left", "objalign-left", "Align left edges"),
    ("hcenter", "objalign-hcenter", "Align horizontal centers"),
    ("right", "objalign-right", "Align right edges"),
    ("top", "objalign-top", "Align top edges"),
    ("vcenter", "objalign-vcenter", "Align vertical centers"),
    ("bottom", "objalign-bottom", "Align bottom edges"),
];

/// The text wrap buttons: icon, mode and name.
const WRAP_MODES: [(&str, &str, &str); 5] = [
    ("wrap-none", "none", "No Text Wrap"),
    ("wrap-bbox", "boundingBox", "Wrap Around Bounding Box"),
    ("wrap-contour", "contour", "Wrap Around Object Shape"),
    ("wrap-jump", "jumpObject", "Jump Object"),
    ("wrap-next", "jumpToNextColumn", "Jump to Next Column"),
];

/// The wrap offset fields: the offset's index in `[top, left, bottom, right]`, its key, icon and name, and where the
/// field's icon sits in the block.
const WRAP_OFFSETS: [(usize, &str, &str, &str, f32, f32); 4] = [
    (0, "wrapTop", "margin-top", "Top", 0.0, 0.0),
    (2, "wrapBottom", "margin-bottom", "Bottom", 0.0, FIELD_H + 5.0),
    (1, "wrapLeft", "margin-inside", "Left", 140.0, 0.0),
    (3, "wrapRight", "margin-outside", "Right", 140.0, FIELD_H + 5.0),
];

/// The Arrange menu's rows: label and command.
const ARRANGE: [(&str, &str); 4] = [
    ("Bring to Front", "object.bringToFront"),
    ("Bring Forward", "object.bringForward"),
    ("Send Backward", "object.sendBackward"),
    ("Send to Back", "object.sendToBack"),
];

/// The Convert Shape menu's rows: label and target shape.
const CONVERT_SHAPES: [(&str, &str); 9] = [
    ("Rectangle", "rectangle"),
    ("Rounded Rectangle", "roundedRectangle"),
    ("Beveled Rectangle", "beveledRectangle"),
    ("Inverse Rounded Rectangle", "inverseRoundedRectangle"),
    ("Ellipse", "ellipse"),
    ("Triangle", "triangle"),
    ("Polygon", "polygon"),
    ("Line", "line"),
    ("Orthogonal Line", "orthogonalLine"),
];

/// The 9-point reference-point proxy; returns a newly picked point (0..8, row-major).
pub(crate) fn ref_point_proxy(ui: &mut Ui, r: Rect, cur: u8) -> Option<u8> {
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    let pitch = r.width() / 3.0;
    for i in 0..9u8 {
        let c = pos2(r.min.x + pitch * (i % 3) as f32 + pitch / 2.0, r.min.y + pitch * (i / 3) as f32 + pitch / 2.0);
        let sq = Rect::from_center_size(c, vec2(6.0, 6.0));
        let resp = ui.interact(sq.expand(1.5), ui.scope_id().with(("refpt", i)), Sense::click());
        if i == cur {
            ui.painter().rect_filled(sq.expand(0.5), 0.0, egui::Color32::WHITE);
        } else {
            ui.painter().rect_stroke(sq, 0.0, Stroke::new(1.0, if resp.hovered() { t.text_strong } else { t.icon }), StrokeKind::Inside);
        }
        if resp.clicked() {
            out = Some(i);
        }
    }
    // Connecting lines between the squares.
    for k in 0..3 {
        let y = r.min.y + pitch * k as f32 + pitch / 2.0;
        let x = r.min.x + pitch * k as f32 + pitch / 2.0;
        for j in 0..2 {
            let a = r.min.x + pitch * j as f32 + pitch / 2.0 + 3.0;
            ui.painter().line_segment([pos2(a, y), pos2(a + pitch - 6.0, y)], Stroke::new(1.0, t.icon));
            let a = r.min.y + pitch * j as f32 + pitch / 2.0 + 3.0;
            ui.painter().line_segment([pos2(x, a), pos2(x, a + pitch - 6.0)], Stroke::new(1.0, t.icon));
        }
    }
    out
}

/// The reference point's position on the selection's bounds `r`, in page coordinates.
fn reference_position(r: designcraft_geom::Rect, rp: u8) -> (f64, f64) {
    let (fx, fy) = ((rp % 3) as f64 / 2.0, (rp / 3) as f64 / 2.0);
    (r.x0 + r.width() * fx, r.y0 + r.height() * fy)
}

pub(super) fn transform_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::transform_section(app, ui, i);
    }
    let u = units(app);
    let rp = app.ui.ref_point;
    let b = block(ui, 2.0 * FIELD_H + 4.0);
    // Reference-point position of the bounds.
    let r = i.page_rect;
    let (x, y) = reference_position(r, rp);
    if let Some(p) = ref_point_proxy(ui, sub(b, 5.0, 10.0, 26.0, 26.0), rp) {
        app.ui.ref_point = p;
    }
    let constrain = temp_bool(ui, "constrain_wh", false);
    let mut set = serde_json::Map::new();
    for (row, (lx, vx, kx, lw, vw, kw)) in
        [("X:", x, "x", "W:", r.width(), "width"), ("Y:", y, "y", "H:", r.height(), "height")].into_iter().enumerate()
    {
        let yy = row as f32 * 25.0;
        place(ui, sub(b, 40.0, yy, 18.0, FIELD_H), |ui| widgets::caption_w(ui, lx, 18.0));
        if let Some(v) = place(ui, sub(b, 59.0, yy, 54.0, FIELD_H), |ui| measure(ui, kx, Some(vx), u, 54.0)) {
            set.insert(kx.into(), json!(v));
        }
        place(ui, sub(b, 120.0, yy, 19.0, FIELD_H), |ui| widgets::caption_w(ui, lw, 19.0));
        if let Some(v) = place(ui, sub(b, 140.0, yy, 54.0, FIELD_H), |ui| measure(ui, kw, Some(vw), u, 54.0)) {
            set.insert(kw.into(), json!(v));
            if constrain && vw > 0.0 {
                let k = v / vw;
                let (ok, ov) = if kw == "width" { ("height", r.height()) } else { ("width", r.width()) };
                set.insert(ok.into(), json!(ov * k));
            }
        }
    }
    let clicked = place(ui, sub(b, 200.0, 12.0, 22.0, FIELD_H), |ui| {
        widgets::icon_toggle_sized(
            ui,
            if constrain { "link" } else { "link-broken" },
            constrain,
            "Constrain proportions for width and height",
            vec2(22.0, 21.0),
        )
        .clicked()
    });
    if clicked {
        set_temp_bool(ui, "constrain_wh", !constrain);
    }
    if !set.is_empty() {
        set.insert("ref".into(), json!(rp));
        let _ = app.run("transform.set", Value::Object(set));
    }
    transform_angles(app, ui, rp);
    if more_options(ui, &app.ui.language).clicked() {
        show_panel(app, "info");
    }
}

/// Properties › Transform for points selected with Direct Selection: X/Y/W/H of the points'
/// bounds (from the reference point); X and Y move the points, W and H spread or gather them
/// about the reference point. No angles: those turn objects.
pub(super) fn anchor_transform_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::anchor_transform_section(app, ui, i);
    }
    let u = units(app);
    let rp = app.ui.ref_point;
    let b = block(ui, 2.0 * FIELD_H + 4.0);
    let r = i.page_rect;
    let (x, y) = reference_position(r, rp);
    if let Some(p) = ref_point_proxy(ui, sub(b, 5.0, 10.0, 26.0, 26.0), rp) {
        app.ui.ref_point = p;
    }
    let mut set = serde_json::Map::new();
    let (single, w, h) = anchor_size(app, i);
    for (row, (lx, vx, kx, lw, vw, kw)) in [("X:", x, "x", "W:", w, "width"), ("Y:", y, "y", "H:", h, "height")].into_iter().enumerate() {
        let yy = row as f32 * 25.0;
        place(ui, sub(b, 40.0, yy, 18.0, FIELD_H), |ui| widgets::caption_w(ui, lx, 18.0));
        if let Some(v) = place(ui, sub(b, 59.0, yy, 54.0, FIELD_H), |ui| measure(ui, &format!("pt_{kx}"), Some(vx), u, 54.0)) {
            set.insert(kx.into(), json!(v));
        }
        place(ui, sub(b, 120.0, yy, 19.0, FIELD_H), |ui| widgets::caption_w(ui, lw, 19.0));
        let id = format!("pt_{kw}");
        let field = NumField::measure(&id, Some(vw), u).width(54.0).enabled(!single);
        if let Some(v) = place(ui, sub(b, 140.0, yy, 54.0, FIELD_H), |ui| field.show(ui)) {
            set.insert(kw.into(), json!(v));
        }
    }
    if !set.is_empty() {
        let _ = transform_anchors(app, &Value::Object(set));
    }
    if more_options(ui, &app.ui.language).clicked() {
        show_panel(app, "info");
    }
}

/// Whether one point is selected, and the width and height the anchor fields show: a single point has no size, so
/// as InDesign W and H show its object's, greyed out.
fn anchor_size(app: &DesignApp, i: &SelInfo) -> (bool, f64, f64) {
    let single = crate::panels::selected_anchor_points(app).len() == 1;
    let (w, h) = if single { i.object_size } else { (i.page_rect.width(), i.page_rect.height()) };
    (single, w, h)
}

/// Move the selected points as Properties › Transform's `set` asks ({x?, y?, width?, height?},
/// in the page coordinates the fields show, from the reference point): one `path.moveAnchors`
/// step, each point by its own movement. A width or height of a single point (0) can't scale.
pub(crate) fn transform_anchors(app: &mut DesignApp, set: &Value) -> Result<Value, String> {
    let points = crate::panels::selected_anchor_points(app);
    let Some(i) = crate::panels::sel_info(app) else { return Ok(Value::Null) };
    if points.is_empty() {
        return Ok(Value::Null);
    }
    let rp = app.ui.ref_point;
    let r = i.page_rect;
    let (fx, fy) = ((rp % 3) as f64 / 2.0, (rp / 3) as f64 / 2.0);
    // Page coordinates and spread coordinates differ by the ruler origin only.
    let b = points.iter().skip(1).fold(designcraft_geom::Rect::from_points(points[0].1, points[0].1), |b, (_, p)| b.union_pt(*p));
    let origin = (b.x0 - r.x0, b.y0 - r.y0);
    let reference = (r.x0 + r.width() * fx + origin.0, r.y0 + r.height() * fy + origin.1);
    let num = |k: &str| set.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    let scale = |new: Option<f64>, old: f64| new.filter(|_| old > 1e-9).map_or(1.0, |v| v.max(0.0) / old);
    let (kx, ky) = (scale(num("width"), r.width()), scale(num("height"), r.height()));
    let shift = (num("x").map_or(0.0, |v| v - (r.x0 + r.width() * fx)), num("y").map_or(0.0, |v| v - (r.y0 + r.height() * fy)));
    let moved: Vec<Value> = points
        .iter()
        .map(|&((id, s, a), p)| {
            let nx = reference.0 + (p.x - reference.0) * kx + shift.0;
            let ny = reference.1 + (p.y - reference.1) * ky + shift.1;
            json!([id.0, s, a, nx - p.x, ny - p.y])
        })
        .collect();
    app.run("path.moveAnchors", json!({"points": moved}))
}

/// Properties › Transform for selected ruler guides: X for one vertical guide, Y for one
/// horizontal guide; with several guides both fields are empty.
pub(super) fn guide_section(app: &mut DesignApp, ui: &mut Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::guide_section(app, ui);
    }
    let guide = crate::panels::guide_field(app);
    let b = block(ui, 2.0 * FIELD_H + 4.0);
    for (row, (label, key)) in [("X:", "gx"), ("Y:", "gy")].into_iter().enumerate() {
        let yy = row as f32 * 25.0;
        let on = guide.filter(|g| g.row() == row);
        place(ui, sub(b, 40.0, yy, 18.0, FIELD_H), |ui| widgets::caption_w(ui, label, 18.0));
        let typed = place(ui, sub(b, 59.0, yy, 135.0, FIELD_H), |ui| {
            if on.is_none() {
                ui.disable();
            }
            measure(ui, key, on.map(|g| g.value), on.map_or_else(|| units(app), |g| g.unit), 135.0)
        });
        if let (Some(v), Some(g)) = (typed, on) {
            let _ = app.run("guide.move", g.move_to(v));
        }
    }
}

/// Properties › Transform, below X/Y/W/H as in InDesign: rotation and shear angles beside the
/// X and Y scales (totals, as Transformations are Totals), then rotate a quarter turn and flip.
fn transform_angles(app: &mut DesignApp, ui: &mut Ui, rp: u8) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let tv = app.session.execute("transform.info", &json!({})).ok();
    let tv_get = |k: &str| tv.as_ref().and_then(|v| v[k].as_f64());
    ui.add_space(4.0);
    let b = block(ui, 2.0 * FIELD_H + 4.0);
    let mut set = serde_json::Map::new();
    for (row, cells) in ANGLE_ROWS.into_iter().enumerate() {
        let yy = row as f32 * 25.0;
        for (col, (icon, tip, key, suffix, id)) in cells.into_iter().enumerate() {
            let x = if col == 0 { 40.0 } else { 120.0 };
            let ir = sub(b, x, yy + 2.0, 17.0, 17.0);
            icons::paint(ui.painter(), ir, icon, t.icon);
            ui.interact(ir, ui.scope_id().with(id), Sense::hover()).on_hover_text(crate::i18n::tr(&lang, tip));
            if let Some(v) = place(ui, sub(b, x + 19.0, yy, 54.0, FIELD_H), |ui| widgets::number(ui, id, tv_get(key), suffix, 54.0, 1)) {
                set.insert(key.into(), json!(v));
            }
        }
    }
    if !set.is_empty() {
        set.insert("ref".into(), json!(rp));
        let _ = app.run("transform.set", Value::Object(set));
    }
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.add_space(36.0);
        ui.spacing_mut().item_spacing.x = 10.0;
        for (icon, tip, cmd, params) in rotate_flip() {
            let label = crate::i18n::tr(&lang, tip);
            let resp = icon_toggle(ui, icon, false, label);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            if resp.clicked() {
                let _ = app.run(cmd, params);
            }
        }
    });
}

/// A stroke-type preview dropdown (light box with the line style).
pub(crate) fn stroke_type_dropdown(app: &mut DesignApp, ui: &mut Ui, cur: &str, w: f32) {
    let resp = preview_dropdown(ui, w, crate::i18n::tr(&app.ui.language, "Stroke Type"), |p, r| paint_stroke_type(p, r, cur, egui::Color32::BLACK));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(170.0);
        for (label, kind) in STROKE_TYPES {
            if preview_row(ui, *kind == cur, crate::i18n::tr(&app.ui.language, label), 40.0, |p, r, c| paint_stroke_type(p, r, kind, c)).clicked() {
                let _ = app.run("object.stroke", stroke_type_params(kind));
                ui.close();
            }
        }
    });
}

fn paint_stroke_type(p: &egui::Painter, r: Rect, kind: &str, c: egui::Color32) {
    let (x0, x1, y) = (r.min.x + 5.0, r.max.x - 5.0, r.center().y);
    match kind {
        "dashed" => {
            let mut x = x0;
            while x < x1 {
                p.line_segment([pos2(x, y), pos2((x + 6.0).min(x1), y)], Stroke::new(4.0, c));
                x += 9.0;
            }
        }
        "dotted" => {
            let mut x = x0 + 2.0;
            while x < x1 {
                p.circle_filled(pos2(x, y), 2.0, c);
                x += 6.0;
            }
        }
        "thickThin" | "thinThick" | "thinThin" | "thickThick" => {
            let (a, b) = match kind {
                "thickThin" => (3.0, 1.0),
                "thinThick" => (1.0, 3.0),
                "thinThin" => (1.0, 1.0),
                _ => (2.5, 2.5),
            };
            p.line_segment([pos2(x0, y - 3.0), pos2(x1, y - 3.0)], Stroke::new(a, c));
            p.line_segment([pos2(x0, y + 3.0), pos2(x1, y + 3.0)], Stroke::new(b, c));
        }
        _ => {
            p.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(5.0, c));
        }
    }
}

/// A row of a preview dropdown's list, as InDesign's: a small preview of the choice (`preview_w`
/// wide), then its name. Selected rows are highlighted.
pub(crate) fn preview_row(
    ui: &mut Ui,
    selected: bool,
    label: &str,
    preview_w: f32,
    paint: impl FnOnce(&egui::Painter, Rect, egui::Color32),
) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    // As wide as the preview and its name, translated (at least 130 pt for the name), and as the
    // popup once its widest row has set it: the rows' highlights then end together.
    let font = egui::FontId::proportional(12.0);
    let text_w = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font, egui::Color32::WHITE).size().x);
    let w = (preview_w + 12.0 + text_w + 12.0).max(preview_w + 130.0).max(ui.available_width().min(600.0));
    let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
    if selected || resp.hovered() {
        ui.painter().rect_filled(r, 2.0, if selected { t.row_selected } else { t.hover });
    }
    let ink = if selected { t.text_strong } else { t.text };
    paint(ui.painter(), Rect::from_min_size(r.min + vec2(4.0, 0.0), vec2(preview_w, 22.0)), ink);
    crate::rtl::paint(
        ui.painter(),
        pos2(r.min.x + preview_w + 12.0, r.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        ink,
    );
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    resp
}

/// A dropdown whose face is a light preview box (stroke type, corner shape), named `label`.
pub(super) fn preview_dropdown(ui: &mut Ui, w: f32, label: &str, paint: impl FnOnce(&egui::Painter, Rect)) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(w, FIELD_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label));
    let face = Rect::from_min_max(r.min, pos2(r.max.x - widgets::SEG_W, r.max.y));
    ui.painter().rect_filled(face, 0.0, egui::Color32::from_gray(0xdf));
    paint(ui.painter(), face);
    let seg = Rect::from_min_max(pos2(face.max.x, r.min.y), r.max);
    ui.painter().rect_filled(seg, 0.0, if resp.hovered() { t.hover } else { t.input });
    widgets::chevron_down(ui.painter(), seg.center(), t.icon);
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    resp
}

/// A corner shape's glyph, as InDesign draws it in its corner lists: the top edge running into the
/// corner and the right edge leaving it, in a square centred in `r`.
pub(crate) fn paint_corner_shape(p: &egui::Painter, r: Rect, shape: CornerShape, c: egui::Color32) {
    let side = (r.width() - 3.0).min(r.height() - 3.0).max(4.0);
    let o = pos2(r.center().x - side / 2.0, r.center().y - side / 2.0);
    let at = |x: f32, y: f32| pos2(o.x + x * side, o.y + y * side);
    let quarter = |cx: f32, cy: f32, rad: f32, from: f32, to: f32| -> Vec<egui::Pos2> {
        (0..=10).map(|i| from + (to - from) * i as f32 / 10.0).map(|a| at(cx + rad * a.cos(), cy + rad * a.sin())).collect()
    };
    use std::f32::consts::{FRAC_PI_2, PI};
    let stroke = Stroke::new(1.6, c);
    let line = |pts: Vec<egui::Pos2>| {
        p.add(egui::Shape::line(pts, stroke));
    };
    match shape {
        CornerShape::None => line(vec![at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0)]),
        // The corner cut by a diagonal, half the side.
        CornerShape::Bevel => line(vec![at(0.0, 0.0), at(0.5, 0.0), at(1.0, 0.5), at(1.0, 1.0)]),
        // A square notch.
        CornerShape::Inset => line(vec![at(0.0, 0.0), at(0.5, 0.0), at(0.5, 0.45), at(1.0, 0.45), at(1.0, 1.0)]),
        // A concave quarter circle centred on the corner.
        CornerShape::InverseRounded => {
            let mut v = vec![at(0.0, 0.0)];
            v.extend(quarter(1.0, 0.0, 0.5, PI, FRAC_PI_2));
            v.push(at(1.0, 1.0));
            line(v)
        }
        // A large convex arc.
        CornerShape::Rounded => {
            let mut v = vec![at(0.0, 0.0)];
            v.extend(quarter(0.08, 0.92, 0.92, -FRAC_PI_2, 0.0));
            v.push(at(1.0, 1.0));
            line(v)
        }
        // Steps into the corner with a small square loop between them.
        CornerShape::Fancy => {
            line(vec![at(0.0, 0.0), at(0.38, 0.0), at(0.38, 0.2), at(0.52, 0.2)]);
            p.rect_stroke(Rect::from_min_max(at(0.52, 0.06), at(0.76, 0.3)), 0.0, stroke, StrokeKind::Middle);
            line(vec![at(0.76, 0.3), at(0.76, 0.48), at(1.0, 0.48), at(1.0, 1.0)]);
        }
    }
}

/// A corner shape picker's face: the shape's glyph and the dropdown arrow (the menu is the caller's).
pub(crate) fn corner_shape_button(ui: &mut Ui, shape: CornerShape, width: f32) -> egui::Response {
    // The caller names the control (widget_info) once it knows the corner.
    preview_dropdown(ui, width, "", |p, r| paint_corner_shape(p, r, shape, egui::Color32::BLACK))
}

pub(crate) fn corner_type_dropdown(app: &mut DesignApp, ui: &mut Ui, shape: CornerShape, size: f64, width: f32) {
    let resp =
        preview_dropdown(ui, width, crate::i18n::tr(&app.ui.language, "Corner Shape"), |p, r| paint_corner_shape(p, r, shape, egui::Color32::BLACK));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(150.0);
        for s in CornerShape::ALL {
            if preview_row(ui, s == shape, crate::i18n::tr(&app.ui.language, s.label()), 22.0, |p, r, c| paint_corner_shape(p, r, s, c)).clicked() {
                let _ = app.run("object.cornerOptions", corner_shape_params(s, size));
                ui.close();
            }
        }
    });
}

pub(super) fn appearance_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::appearance_section(app, ui, i);
    }
    let t = Tokens::get(ui.ctx());
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let text_frames = all_text_frames(app, i);
    // Fill.
    let b = block(ui, FIELD_H);
    let (fc, fg) = widgets::swatch_colors(&doc, &i.fill, 1.0);
    let chip = sub(b, 4.0, 2.0, 17.0, 17.0);
    widgets::paint_chip(ui.painter(), chip, fc, fg);
    ui.painter().rect_stroke(chip, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    let resp = ui.interact(chip, ui.scope_id().with("fillchip"), Sense::click()).on_hover_text(format!(
        "{}: {}",
        crate::i18n::tr(&app.ui.language, "Fill"),
        i.fill
    ));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Fill swatch"));
    swatch_menu(app, &resp, &doc, &i.fill, false, text_frames, false);
    if resp.double_clicked() {
        open_color_picker(app, &doc, &i.fill, false, false);
    }
    crate::rtl::paint(
        ui.painter(),
        b.min + vec2(34.0, FIELD_H / 2.0),
        egui::Align2::LEFT_CENTER,
        crate::i18n::tr(&app.ui.language, "Fill"),
        egui::FontId::proportional(11.5),
        t.text,
    );
    ui.add_space(4.0);
    // Stroke.
    let b = block(ui, FIELD_H);
    let (sc, _) = widgets::swatch_colors(&doc, &i.stroke, 1.0);
    let chip = sub(b, 4.0, 2.0, 17.0, 17.0);
    widgets::paint_stroke_chip(ui.painter(), chip, sc, egui::Color32::from_gray(20));
    ui.painter().rect_stroke(chip, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    let resp = ui.interact(chip, ui.scope_id().with("strokechip"), Sense::click()).on_hover_text(format!(
        "{}: {}",
        crate::i18n::tr(&app.ui.language, "Stroke"),
        i.stroke
    ));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Stroke swatch"));
    swatch_menu(app, &resp, &doc, &i.stroke, true, text_frames, false);
    if resp.double_clicked() {
        open_color_picker(app, &doc, &i.stroke, true, false);
    }
    if place(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), |ui| link_label(ui, crate::i18n::tr(&app.ui.language, "Stroke"), 56.0)).clicked() {
        show_panel(app, "stroke");
    }
    if let Some(v) = place(ui, sub(b, 96.0, 0.0, 69.0, FIELD_H), |ui| {
        NumField::points("sw", Some(i.stroke_weight), 3).width(69.0).spinner().presets(STROKE_WEIGHTS).range(0.0, 800.0).show(ui)
    }) {
        let _ = app.run("object.stroke", json!({"weight": v}));
    }
    let kind = i.stroke_kind.clone();
    place(ui, sub(b, 171.0, 0.0, 54.0, FIELD_H), |ui| stroke_type_dropdown(app, ui, &kind, 54.0));
    ui.add_space(4.0);
    // Corner.
    let b = block(ui, FIELD_H);
    icons::paint(ui.painter(), sub(b, 3.0, 1.0, 19.0, 19.0), "corner", t.accent);
    if place(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), |ui| link_label(ui, crate::i18n::tr(&app.ui.language, "Corner"), 56.0)).clicked() {
        // Corner Options, as Object › Corner Options… opens it.
        crate::menus::activate(app, "object.cornerOptions", &Value::Null);
    }
    let (shape, size) = i.corner;
    if let Some(v) = place(ui, sub(b, 96.0, 0.0, 69.0, FIELD_H), |ui| {
        NumField::measure("corner", Some(size), Unit::Points).width(69.0).spinner().range(0.0, 1000.0).show(ui)
    }) {
        let _ = app.run("object.cornerOptions", corner_size_params(shape, v));
    }
    place(ui, sub(b, 171.0, 0.0, 54.0, FIELD_H), |ui| {
        corner_type_dropdown(app, ui, shape, size, 54.0);
    });
    ui.add_space(4.0);
    // Opacity.
    let b = block(ui, FIELD_H);
    icons::paint(ui.painter(), sub(b, 3.0, 1.0, 19.0, 19.0), "opacity", t.icon);
    if place(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), |ui| link_label(ui, crate::i18n::tr(&app.ui.language, "Opacity"), 56.0)).clicked() {
        show_panel(app, "effects");
    }
    let typed = place(ui, sub(b, 96.0, 0.0, 51.0, FIELD_H), |ui| {
        NumField::number("opacity", Some((i.opacity * 100.0).round()), "%", 0).width(51.0).range(0.0, 100.0).show(ui)
    });
    // The field shows whole percents; `object.opacity` takes the ratio.
    if let Some(percent) = typed {
        let _ = app.run("object.opacity", json!({"opacity": (percent / 100.0).clamp(0.0, 1.0)}));
    }
    let more = sub(b, 147.0, 0.0, 18.0, FIELD_H);
    let mresp = ui.interact(more, ui.scope_id().with("opmore"), Sense::click());
    ui.painter().rect(more, 0.0, if mresp.hovered() { t.hover } else { t.input }, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    icons::paint(ui.painter(), more.shrink(4.0), "chevron-right", t.icon);
    mresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, crate::i18n::tr(&app.ui.language, "Opacity")));
    opacity_slider(app, &mresp, i.opacity);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if icon_toggle(ui, "frame-dashed", false, crate::i18n::tr(&app.ui.language, "Frame Fitting")).clicked() {
            show_panel(app, "textWrap");
        }
        if icon_toggle(ui, "fx", i.shadow, crate::i18n::tr(&app.ui.language, "Effects: Drop Shadow")).clicked() {
            let _ = app.run("object.dropShadow", json!({"on": !i.shadow}));
        }
    });
}

/// Whether every selected object is a text frame: the Fill and Stroke popups then offer Apply To Frame or Text, for
/// one frame or several, as InDesign's do.
fn all_text_frames(app: &DesignApp, i: &SelInfo) -> bool {
    i.is_text && app.session.active().is_some_and(|st| st.selection.items.iter().all(|id| st.doc.item(*id).is_some_and(|it| it.is_text_frame())))
}

/// The Color Picker for a Fill or Stroke chip (a double-click on it, as in InDesign), with the
/// chip's colour: the objects', or with `text` the selected text's.
pub(super) fn open_color_picker(app: &mut DesignApp, doc: &designcraft_doc::Document, swatch: &str, stroke: bool, text: bool) {
    let mut fields = crate::panels::swatches::picker_fields(doc.resolve_color(swatch, 1.0), stroke);
    fields["text"] = json!(text);
    app.ui.dialog = Some(crate::dialogs::Dialog::new("colorPicker", fields));
}

/// The Opacity slider that the arrow beside the field opens, as in InDesign: a drag shows the
/// new opacity live and is one undo step when let go.
fn opacity_slider(app: &mut DesignApp, resp: &egui::Response, opacity: f64) {
    let (mut slid, mut held, mut shown) = (None, false, false);
    egui::Popup::menu(resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        shown = true;
        // A thin rail and a round knob, as InDesign draws it.
        ui.spacing_mut().slider_width = 130.0;
        ui.spacing_mut().slider_rail_height = 2.0;
        let mut v = (opacity * 100.0).round();
        let slider = ui.add(
            egui::Slider::new(&mut v, 0.0..=100.0).integer().show_value(false).handle_shape(egui::style::HandleShape::Circle).trailing_fill(false),
        );
        // The slider has no visible caption (the field beside the arrow is the label), so it's
        // named for assistive technology only.
        let label = crate::i18n::tr(&app.ui.language, "Opacity");
        slider.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), v, label));
        // The release frame can move the value too; it stays part of the drag's preview, which
        // the next frame commits as the one undo step.
        let pressed = slider.is_pointer_button_down_on() || slider.dragged() || slider.drag_stopped();
        if slider.changed() {
            slid = Some((v, pressed));
        }
        held = pressed;
    });
    match slid {
        Some((v, true)) => app.live_preview("Opacity", "object.opacity", json!({"opacity": v / 100.0})),
        Some((v, false)) => {
            // The keyboard (arrow keys on the focused slider): applied at once.
            app.end_live_preview();
            let _ = app.run("object.opacity", json!({"opacity": v / 100.0}));
        }
        None if held => app.hold_live_preview(),
        // Shown but let go without a change (a click outside, Escape): the drag's preview is
        // committed now, before whatever that click does, so that keeps its own undo step.
        None if shown => app.end_live_preview(),
        None => {}
    }
}

/// The key under which a Fill (`stroke` false) or Stroke popup remembers that it colours a text frame's text.
fn apply_to_text_key(stroke: bool) -> egui::Id {
    egui::Id::unique(("swatch_apply_to_text", stroke))
}

/// What a Fill (or Stroke) popup shows and changes: the swatch and tint of the frames (`cur` is theirs), or with
/// `to_text` those of their text.
fn swatch_target(app: &mut DesignApp, cur: &str, stroke: bool, to_text: bool) -> (String, f64) {
    let (key, tint_key) = if stroke { ("stroke", "strokeTint") } else { ("fill", "fillTint") };
    if to_text {
        let chars = crate::panels::text_attrs(app).map(|a| a["chars"].clone()).unwrap_or_default();
        (chars[key].as_str().unwrap_or("").to_string(), chars[tint_key].as_f64().unwrap_or(1.0))
    } else {
        (cur.to_string(), f64::from(crate::panels::sel_tint(app, stroke)))
    }
}

/// Applies what a Fill (or Stroke) popup picked: the swatch `swatch`, else the tint `tint`, to the text (`to_text`)
/// or to the frames.
fn apply_swatch(app: &mut DesignApp, stroke: bool, to_text: bool, swatch: Option<String>, tint: Option<f64>) {
    let (key, tint_key) = if stroke { ("stroke", "strokeTint") } else { ("fill", "fillTint") };
    let attrs = match (swatch, tint) {
        (Some(name), _) => json!({ key: name }),
        (None, Some(tint)) => json!({ tint_key: tint }),
        (None, None) => return,
    };
    if to_text {
        let _ = app.run("type.char", json!({ "attrs": attrs }));
    } else {
        let mut p = attrs.as_object().cloned().unwrap_or_default();
        // object.fill / object.stroke name the swatch `swatch` and the tint `tint`.
        if let Some(v) = p.remove(key) {
            p.insert("swatch".into(), v);
        }
        if let Some(v) = p.remove(tint_key) {
            p.insert("tint".into(), v);
        }
        let _ = app.run(if stroke { "object.stroke" } else { "object.fill" }, Value::Object(p));
    }
}

/// The Fill (or Stroke) popup of Properties › Appearance, as InDesign's: Apply To (a text frame's
/// frame or its text), the tint, then the swatches. `cur` is the frame's swatch. With `text_only`
/// (text selected) it colours the selected text.
pub(super) fn swatch_menu(
    app: &mut DesignApp,
    resp: &egui::Response,
    doc: &designcraft_doc::Document,
    cur: &str,
    stroke: bool,
    text_frame: bool,
    text_only: bool,
) {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    let to_text_id = apply_to_text_key(stroke);
    let to_text = text_only || text_frame && resp.ctx.data(|d| d.get_temp::<bool>(to_text_id)).unwrap_or(false);
    let (cur, tint) = swatch_target(app, cur, stroke, to_text);
    let mut picked = None;
    let mut new_tint = None;
    let mut switch = None;
    egui::Popup::menu(resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        if text_frame {
            ui.horizontal(|ui| {
                crate::rtl::label(ui, tr("Apply To:"));
                for (label, text) in [("Frame", false), ("Text", true)] {
                    if ui.selectable_label(to_text == text, crate::rtl::widget(ui, tr(label))).clicked() {
                        switch = Some(text);
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            crate::rtl::label(ui, tr("Tint:"));
            if let Some(v) = NumField::number(if stroke { "stroke_tint" } else { "fill_tint" }, Some((tint * 100.0).round()), "%", 0)
                .width(60.0)
                .range(0.0, 100.0)
                .show(ui)
            {
                new_tint = Some(v / 100.0);
            }
        });
        ui.separator();
        picked = crate::panels::swatch_list(ui, doc, &cur);
    });
    if let Some(t) = switch {
        resp.ctx.data_mut(|d| d.insert_temp(to_text_id, t));
    }
    apply_swatch(app, stroke, to_text, picked, new_tint);
}

/// The Align To button's glyph shows what the objects align to, as InDesign's.
pub(super) fn align_to_icon(to: &str) -> &'static str {
    match to {
        "keyObject" => "align-to-key",
        "margins" => "align-to-margins",
        "page" => "align-to-page",
        "spread" => "align-to-spread",
        _ => "align-to",
    }
}

/// What Align To shows: the key object (a key object switches it to Key Object while several objects are selected),
/// else the panel's target; the key object, if any; and the button's tooltip.
fn align_target(app: &DesignApp) -> (String, Option<designcraft_doc::ItemId>, String) {
    let key = app.session.active().and_then(|st| st.selection.key.filter(|_| st.selection.items.len() > 1));
    let to = if key.is_some() { "keyObject".to_string() } else { app.ui.align_to.clone() };
    let to_label = match to.as_str() {
        "selection" => "Selection",
        "keyObject" => "Key Object",
        "margins" => "Margins",
        "page" => "Page",
        "spread" => "Spread",
        other => other,
    };
    let tip = format!("{}: {}", crate::i18n::tr(&app.ui.language, "Align To"), crate::i18n::tr(&app.ui.language, to_label));
    (to, key, tip)
}

/// Picks the Align To target `id`: choosing another target than Key Object drops the key object.
fn pick_align_to(app: &mut DesignApp, key: Option<designcraft_doc::ItemId>, id: &str) {
    if let Some(k) = key.filter(|_| id != "keyObject") {
        let _ = app.run("selection.key", json!({"id": k.0}));
    }
    app.ui.align_to = id.into();
}

pub(super) fn align_row(app: &mut DesignApp, ui: &mut Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::align_row(app, ui);
    }
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (to, key, tip) = align_target(app);
        let resp = icon_toggle(ui, align_to_icon(&to), false, &tip);
        egui::Popup::menu(&resp).show(|ui| {
            for (label, id) in ALIGN_TO {
                if ui.selectable_label(to == id, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    pick_align_to(app, key, id);
                    ui.close();
                }
            }
        });
        let (r, _) = ui.allocate_exact_size(vec2(1.0, FIELD_H), Sense::hover());
        ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, t.section_divider.gamma_multiply(1.6)));
        for (edge, icon, tip) in ALIGN_EDGES {
            if icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.align", json!({"edge": edge, "to": to}));
            }
        }
    });
}

pub(super) fn wrap_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::wrap_section(app, ui, i);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 7.0;
        for (icon, mode, tip) in WRAP_MODES {
            if icon_toggle(ui, icon, i.wrap == mode, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.textWrap", json!({"mode": mode}));
            }
        }
        ui.add_space(2.0);
        let mut inv = i.wrap_invert;
        let enabled = i.wrap != "none";
        let r = ui.add_enabled_ui(enabled, |ui| widgets::checkbox(ui, &mut inv, crate::i18n::tr(&app.ui.language, "Invert"))).inner;
        if r.changed() {
            // Each object keeps its own mode.
            let _ = app.run("object.textWrap", json!({"mode": null, "invert": inv}));
        }
    });
    wrap_offsets(app, ui, i);
    if more_options(ui, &app.ui.language).clicked() {
        show_panel(app, "textWrap");
    }
}

/// The key under which the wrap offsets' link remembers its state. It belongs to the selection: another selection
/// starts linked again when its four offsets are the same.
fn wrap_link_key(app: &DesignApp) -> egui::Id {
    let selected: Vec<u64> = app.session.active().map(|st| st.selection.items.iter().map(|i| i.0).collect()).unwrap_or_default();
    egui::Id::unique(("prop_wrap_linked", selected))
}

/// The offsets an edit of the offset at `k` sets: linked, all four; else that side alone, each object keeping its
/// other offsets. Offsets are `[top, left, bottom, right]`.
fn wrap_offsets_set(linked: bool, k: usize, v: f64) -> [Option<f64>; 4] {
    if linked {
        [Some(v); 4]
    } else {
        let mut o = [None; 4];
        o[k] = Some(v);
        o
    }
}

/// The wrap offsets under the wrap buttons, as InDesign's Properties panel shows them: Top and
/// Bottom, the link that makes all four the same, then Left and Right. Greyed with no wrap.
fn wrap_offsets(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let u = units(app);
    let enabled = i.wrap != "none";
    let offsets = i.wrap_offsets;
    ui.add_space(4.0);
    let b = block(ui, 2.0 * FIELD_H + 5.0);
    let mut set = None;
    let link_id = wrap_link_key(app);
    ui.add_enabled_ui(enabled, |ui| {
        // Linked while the four are the same, until the link is clicked.
        let same = offsets.iter().all(|v| (v - offsets[0]).abs() < 1e-9);
        let linked = ui.data(|d| d.get_temp::<bool>(link_id)).unwrap_or(same);
        for (k, key, icon, tip, x, y) in WRAP_OFFSETS {
            let ir = sub(b, x, y + 2.0, 17.0, 17.0);
            icons::paint(ui.painter(), ir, icon, if enabled { t.icon } else { t.text_dim });
            ui.interact(ir, ui.scope_id().with(("prop_wrap_off", k)), Sense::hover()).on_hover_ui(|ui| {
                crate::rtl::label(ui, tr(tip));
            });
            let nv = place(ui, sub(b, x + 20.0, y, 76.0, FIELD_H), |ui| {
                NumField::measure(key, Some(offsets[k]), u).width(76.0).spinner().range(-8000.0, 8000.0).enabled(enabled).show(ui)
            });
            if let Some(v) = nv {
                set = Some(wrap_offsets_set(linked, k, v));
            }
        }
        if place(ui, sub(b, 106.0, (FIELD_H + 5.0) / 2.0, 22.0, FIELD_H), |ui| {
            widgets::icon_toggle_sized(ui, if linked { "link" } else { "link-broken" }, linked, &tr("Make all settings the same"), vec2(22.0, 21.0))
                .clicked()
        }) {
            ui.data_mut(|d| d.insert_temp(link_id, !linked));
        }
    });
    if let Some(o) = set {
        // Each object keeps its own mode: only the offsets change.
        let _ = app.run("object.textWrap", json!({"mode": null, "offset": o}));
    }
}

fn arrange_menu(app: &mut DesignApp, resp: &egui::Response) {
    egui::Popup::menu(resp).show(|ui| {
        for (label, cmd) in ARRANGE {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                let _ = app.run(cmd, json!({}));
                ui.close();
            }
        }
    });
}

/// Converts the selection to the shape `to`; a shape that cannot be converted says why in the status bar.
fn convert_shape(app: &mut DesignApp, to: &str) {
    if let Err(e) = app.run("object.convertShape", json!({"to": to})) {
        app.status_error("Convert Shape", e);
    }
}

fn convert_shape_menu(app: &mut DesignApp, resp: &egui::Response) {
    egui::Popup::menu(resp).show(|ui| {
        for (label, to) in CONVERT_SHAPES {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                convert_shape(app, to);
                ui.close();
            }
        }
    });
}

pub(super) fn quick_actions_object(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::quick_actions_object(app, ui, i);
    }
    let fw = full_width(ui);
    let hw = (fw - 10.0) / 2.0;
    if i.is_text {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let r = outline_button(ui, crate::i18n::tr(&app.ui.language, "Convert Shape"), hw);
            convert_shape_menu(app, &r);
            let r = outline_button(ui, crate::i18n::tr(&app.ui.language, "Arrange"), hw);
            arrange_menu(app, &r);
        });
        ui.add_space(4.0);
        if outline_button(ui, crate::i18n::tr(&app.ui.language, "Fill with Placeholder Text"), fw).clicked() {
            let _ = app.run("type.fillWithPlaceholder", json!({}));
        }
        ui.add_space(4.0);
        if outline_button(ui, crate::i18n::tr(&app.ui.language, "Edit in Story Editor"), fw).clicked() {
            let _ = app.run("app.storyEditor", json!({}));
        }
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let r = outline_button(ui, crate::i18n::tr(&app.ui.language, "Arrange"), hw);
        arrange_menu(app, &r);
        if outline_button(ui, crate::i18n::tr(&app.ui.language, group_or_lock(i).0), hw).clicked() {
            let _ = app.run(group_or_lock(i).1, json!({}));
        }
    });
    ui.add_space(4.0);
    if i.kind == "<group>" {
        if outline_button(ui, crate::i18n::tr(&app.ui.language, "Ungroup"), fw).clicked() {
            let _ = app.run("object.ungroup", json!({}));
        }
    } else {
        let r = outline_button(ui, crate::i18n::tr(&app.ui.language, "Convert Shape"), fw);
        convert_shape_menu(app, &r);
    }
}

/// The second quick action of an object selection: Group for several objects, else Lock; label and command.
fn group_or_lock(i: &SelInfo) -> (&'static str, &'static str) {
    if i.count > 1 { ("Group", "object.group") } else { ("Lock", "object.lock") }
}

/// The object sections on the craft UI framework: the same rows, at the same places, over the same engine commands
/// as the native bodies above, every control a framework component.
#[cfg(not(target_arch = "wasm32"))]
pub(super) mod framework {
    use super::*;
    use crate::framework_transform::{Artwork, Length, icon, icon_button, length_field, swatch_icon};
    use craft_behavior::{Anchor9, Mixed, Selection};
    use craft_theme::variant::Tone;
    use craft_ui_widgets::buttons::{Button, ButtonOutput, IconButton, Link};
    use craft_ui_widgets::choice::{Checkbox, Segmented};
    use craft_ui_widgets::contract::{Bind, Entry, PopupKind, Props as _, SelectMode, Show as _, ShowCollection as _, ShowContainer as _, Trigger};
    use craft_ui_widgets::display::{Divider, Icon, Text};
    use craft_ui_widgets::layout_components::Row;
    use craft_ui_widgets::list::{List, ListRow};
    use craft_ui_widgets::menu::{Menu, MenuItem};
    use craft_ui_widgets::pickers::{ReferencePoint, Select};
    use craft_ui_widgets::popover::Popover;
    use craft_ui_widgets::primitives::{ContentPainterCx, Glyph, IconSpec};
    use craft_ui_widgets::runtime::env::{LengthUnits, UnitKind};
    use craft_ui_widgets::style::UiTokens;
    use craft_ui_widgets::text::{NumberField, NumberPick, link_toggle};

    /// A Fill or Stroke chip as the panel has always drawn it: the swatch's colour or gradient, a stroke as a ring
    /// around a dark centre, inside the field edge.
    #[derive(craft_ui::contract::MemoProps)]
    struct Chip {
        color: Option<egui::Color32>,
        gradient: Option<(egui::Color32, egui::Color32)>,
        stroke: bool,
        /// The "T" that marks a chip of the text's own colours.
        text: bool,
    }

    impl Glyph for Chip {
        fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, _color: egui::Color32, _mirrored: bool) {
            if self.stroke {
                widgets::paint_stroke_chip(painter, rect, self.color, egui::Color32::from_gray(20));
            } else {
                widgets::paint_chip(painter, rect, self.color, self.gradient);
            }
            painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Tokens::get(painter.ctx()).field_border), StrokeKind::Inside);
            if self.text && !self.stroke {
                let light = self.color.is_some_and(|c| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b()) > 380);
                let ink = if light { egui::Color32::BLACK } else { egui::Color32::WHITE };
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, "T", crate::theme::semibold(12.0), ink);
            }
        }
    }

    /// What a Fill or Stroke chip colours: the selected frames (Apply To offered when they are all text frames), or
    /// the selected text.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in crate::panels::properties) enum SwatchFor {
        /// The selected objects; `text_frames` when every one is a text frame.
        Frames { text_frames: bool },
        /// The selected text.
        Text,
    }

    /// The id of the control `key` of the section the panel's `ui` shows.
    fn control_id(ui: &Ui, key: impl std::hash::Hash) -> egui::Id {
        ui.id().with(("properties-object", key))
    }

    /// A link to another panel at `at`: whether it was clicked this pass.
    fn link(ui: &mut Ui, at: Rect, id: egui::Id, label: &str) -> bool {
        Link::new(label.to_owned()).id(id).show_at(ui, at).clicked()
    }

    /// The `•••` button at the end of its own row under the section `section`: whether it was clicked this pass.
    fn more_options(app: &DesignApp, ui: &mut Ui, section: &'static str) -> bool {
        let id = control_id(ui, ("more-options", section));
        crate::framework_transform::more_options(ui, &app.ui.language, id)
    }

    /// The reference-point picker at `at` with the id `id` over the point `current` (0..8, row-major), named in
    /// `language`: the point picked this pass. Properties › Transform and the Control bar share it.
    pub(crate) fn reference_point(ui: &mut Ui, at: Rect, id: egui::Id, language: &str, current: u8) -> Option<u8> {
        let held = Anchor9::ALL.get(usize::from(current)).copied();
        let out = ReferencePoint::new(crate::i18n::tr(language, "Reference Point").to_owned(), Bind::given(held)).id(id).show_at(ui, at);
        let picked = out.events.committed().copied().flatten()?;
        Anchor9::ALL.iter().position(|point| *point == picked).and_then(|at| u8::try_from(at).ok())
    }

    pub(super) fn transform_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
        let lang = app.ui.language.clone();
        let rp = app.ui.ref_point;
        let b = block(ui, 2.0 * FIELD_H + 4.0);
        let id = control_id(ui, "reference-point");
        if let Some(p) = reference_point(ui, sub(b, 5.0, 10.0, 26.0, 26.0), id, &lang, rp) {
            app.ui.ref_point = p;
        }
        let constrain = temp_bool(ui, "constrain_wh", false);
        let mut set = crate::framework_transform::rows(app, ui, b, i, rp, constrain);
        let tip = crate::i18n::tr(&lang, "Constrain proportions for width and height").to_owned();
        let toggled = link_toggle(tip.clone(), Bind::given(constrain))
            .id(control_id(ui, "constrain"))
            .tooltip(tip)
            .show_at(ui, sub(b, 200.0, 12.0, 22.0, FIELD_H))
            .events
            .committed()
            .copied();
        if let Some(on) = toggled {
            set_temp_bool(ui, "constrain_wh", on);
        }
        if !set.is_empty() {
            set.insert("ref".into(), json!(rp));
            let _ = app.run("transform.set", Value::Object(set));
        }
        transform_angles(app, ui, rp);
        if more_options(app, ui, "transform") {
            show_panel(app, "info");
        }
    }

    /// The X/Y/W/H captions and fields of an anchor or guide block row: the caption `caption` at `x`, its field after
    /// it.
    fn caption(ui: &mut Ui, b: Rect, x: f32, y: f32, width: f32, text: &'static str) {
        Text::new(text).show_at(ui, sub(b, x, y, width, FIELD_H));
    }

    pub(super) fn anchor_transform_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
        let lang = app.ui.language.clone();
        let u = units(app);
        let rp = app.ui.ref_point;
        let b = block(ui, 2.0 * FIELD_H + 4.0);
        let (x, y) = reference_position(i.page_rect, rp);
        let id = control_id(ui, "reference-point");
        if let Some(p) = reference_point(ui, sub(b, 5.0, 10.0, 26.0, 26.0), id, &lang, rp) {
            app.ui.ref_point = p;
        }
        let mut set = serde_json::Map::new();
        let (single, w, h) = anchor_size(app, i);
        for (row, (lx, vx, kx, lw, vw, kw)) in [("X:", x, "x", "W:", w, "width"), ("Y:", y, "y", "H:", h, "height")].into_iter().enumerate() {
            let yy = row as f32 * 25.0;
            for (caption_x, caption_w, field_x, text, value, key, enabled) in
                [(40.0, 18.0, 59.0, lx, vx, kx, true), (120.0, 19.0, 140.0, lw, vw, kw, !single)]
            {
                caption(ui, b, caption_x, yy, caption_w, text);
                let label = crate::i18n::tr(&lang, text.trim_end_matches(':')).to_owned();
                let id = control_id(ui, ("anchor", key));
                let length = Length { value: Some(value), unit: u, range: None };
                if let (Some(v), _) = length_field(ui, sub(b, field_x, yy, 54.0, FIELD_H), id, &label, length, enabled) {
                    set.insert(key.into(), json!(v));
                }
            }
        }
        if !set.is_empty() {
            let _ = transform_anchors(app, &Value::Object(set));
        }
        if more_options(app, ui, "anchors") {
            show_panel(app, "info");
        }
    }

    pub(super) fn guide_section(app: &mut DesignApp, ui: &mut Ui) {
        let lang = app.ui.language.clone();
        let guide = crate::panels::guide_field(app);
        let b = block(ui, 2.0 * FIELD_H + 4.0);
        for (row, (text, key)) in [("X:", "gx"), ("Y:", "gy")].into_iter().enumerate() {
            let yy = row as f32 * 25.0;
            let on = guide.filter(|g| g.row() == row);
            caption(ui, b, 40.0, yy, 18.0, text);
            let label = crate::i18n::tr(&lang, text.trim_end_matches(':')).to_owned();
            let length = Length { value: on.map(|g| g.value), unit: on.map_or_else(|| units(app), |g| g.unit), range: None };
            let id = control_id(ui, ("guide", key));
            let (typed, _) = length_field(ui, sub(b, 59.0, yy, 135.0, FIELD_H), id, &label, length, on.is_some());
            if let (Some(v), Some(g)) = (typed, on) {
                let _ = app.run("guide.move", g.move_to(v));
            }
        }
    }

    /// Transform's angles and scales, then the quarter turns and flips.
    fn transform_angles(app: &mut DesignApp, ui: &mut Ui, rp: u8) {
        let lang = app.ui.language.clone();
        let tv = app.session.execute("transform.info", &json!({})).ok();
        let tv_get = |k: &str| tv.as_ref().and_then(|v| v[k].as_f64());
        ui.add_space(4.0);
        let b = block(ui, 2.0 * FIELD_H + 4.0);
        let mut set = serde_json::Map::new();
        for (row, cells) in ANGLE_ROWS.into_iter().enumerate() {
            let yy = row as f32 * 25.0;
            for (col, (artwork, tip, key, suffix, _)) in cells.into_iter().enumerate() {
                let x = if col == 0 { 40.0 } else { 120.0 };
                let tip = crate::i18n::tr(&lang, tip);
                icon(ui, sub(b, x, yy + 2.0, 17.0, 17.0), artwork, Some(tip), true);
                let kind = if suffix == "%" { UnitKind::Percent } else { UnitKind::Degrees };
                let out = NumberField::new(tip.to_owned(), Bind::given(tv_get(key)))
                    .id(control_id(ui, ("angle", key)))
                    .unit_kind(kind)
                    .show_at(ui, sub(b, x + 19.0, yy, 54.0, FIELD_H));
                if let Some(v) = out.edit.events.committed().copied().flatten().filter(|v| v.is_finite()) {
                    set.insert(key.into(), json!(v));
                }
            }
        }
        if !set.is_empty() {
            set.insert("ref".into(), json!(rp));
            let _ = app.run("transform.set", Value::Object(set));
        }
        ui.add_space(2.0);
        let b = block(ui, 21.0);
        for (k, (artwork, tip, cmd, params)) in rotate_flip().into_iter().enumerate() {
            let (at, id) = (sub(b, 36.0 + k as f32 * 33.0, 0.0, 23.0, 21.0), control_id(ui, ("rotate-flip", k)));
            if icon_button(ui, at, id, artwork, crate::i18n::tr(&lang, tip), false) {
                let _ = app.run(cmd, params);
            }
        }
    }

    pub(super) fn appearance_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
        let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
        let lang = app.ui.language.clone();
        let tr = |s: &'static str| crate::i18n::tr(&lang, s).to_owned();
        let text_frames = all_text_frames(app, i);
        // Fill.
        let b = block(ui, FIELD_H);
        swatch_chip(app, ui, sub(b, 4.0, 2.0, 17.0, 17.0), &doc, &i.fill, false, SwatchFor::Frames { text_frames });
        Text::new(tr("Fill")).show_at(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H));
        ui.add_space(4.0);
        // Stroke.
        let b = block(ui, FIELD_H);
        swatch_chip(app, ui, sub(b, 4.0, 2.0, 17.0, 17.0), &doc, &i.stroke, true, SwatchFor::Frames { text_frames });
        let id = control_id(ui, "stroke-link");
        if link(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), id, &tr("Stroke")) {
            show_panel(app, "stroke");
        }
        let presets: Vec<Entry<NumberPick>> =
            STROKE_WEIGHTS.iter().map(|&w| Entry::new(NumberPick::Number(w), widgets::NumKind::Points { decimals: 3 }.format(w))).collect();
        let weight =
            NumberField::length(tr("Stroke Weight"), Bind::given(Some(i.stroke_weight)), LengthUnits::new(UnitKind::Points, UnitKind::Points, 72.0))
                .id(control_id(ui, "stroke-weight"))
                .range(0.0..=800.0)
                .presets(&presets)
                .show_at(ui, sub(b, 96.0, 0.0, 69.0, FIELD_H));
        let picked = match weight.preset {
            Some(NumberPick::Number(w)) => Some(w),
            _ => None,
        };
        if let Some(v) = weight.edit.events.committed().copied().flatten().or(picked).filter(|v| v.is_finite()) {
            let _ = app.run("object.stroke", json!({"weight": v}));
        }
        let id = control_id(ui, "stroke-type");
        stroke_type_select(app, ui, sub(b, 171.0, 0.0, 54.0, FIELD_H), id, &i.stroke_kind);
        ui.add_space(4.0);
        // Corner.
        let b = block(ui, FIELD_H);
        Icon::new(IconSpec::glyph(&Artwork("corner"))).tone(Tone::Accent).show_at(ui, sub(b, 3.0, 1.0, 19.0, 19.0));
        let id = control_id(ui, "corner-link");
        if link(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), id, &tr("Corner")) {
            // Corner Options, as Object › Corner Options… opens it.
            crate::menus::activate(app, "object.cornerOptions", &Value::Null);
        }
        let (shape, size) = i.corner;
        let corner = NumberField::length(tr("Corner Size"), Bind::given(Some(size)), LengthUnits::new(UnitKind::Points, UnitKind::Points, 72.0))
            .id(control_id(ui, "corner-size"))
            .range(0.0..=1000.0)
            .show_at(ui, sub(b, 96.0, 0.0, 69.0, FIELD_H));
        if let Some(v) = corner.edit.events.committed().copied().flatten().filter(|v| v.is_finite()) {
            let _ = app.run("object.cornerOptions", corner_size_params(shape, v));
        }
        let id = control_id(ui, "corner-shape");
        corner_shape_select(app, ui, sub(b, 171.0, 0.0, 54.0, FIELD_H), id, shape, size);
        ui.add_space(4.0);
        // Opacity.
        let b = block(ui, FIELD_H);
        icon(ui, sub(b, 3.0, 1.0, 19.0, 19.0), "opacity", None, true);
        let id = control_id(ui, "opacity-link");
        if link(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), id, &tr("Opacity")) {
            show_panel(app, "effects");
        }
        if let Some(v) = crate::framework_opacity::row(app, ui, b, &tr("Opacity"), i.opacity) {
            let _ = app.run("object.opacity", json!({"opacity": v}));
        }
        ui.add_space(6.0);
        // Frame fitting and effects.
        let b = block(ui, FIELD_H);
        let id = control_id(ui, "frame-fitting");
        if icon_button(ui, sub(b, 0.0, 0.0, 23.0, 21.0), id, "frame-dashed", &tr("Frame Fitting"), false) {
            show_panel(app, "textWrap");
        }
        let id = control_id(ui, "effects");
        if icon_button(ui, sub(b, 33.0, 0.0, 23.0, 21.0), id, "fx", &tr("Effects: Drop Shadow"), i.shadow) {
            let _ = app.run("object.dropShadow", json!({"on": !i.shadow}));
        }
    }

    /// The stroke type picker at `at` with the id `id`: the face is the current type's line, the list each type's line
    /// and name. A pick runs `object.stroke`. Properties › Appearance and the Control bar share it.
    pub(crate) fn stroke_type_select(app: &mut DesignApp, ui: &mut Ui, at: Rect, id: egui::Id, current: &str) {
        let lang = app.ui.language.clone();
        let entries: Vec<Entry<&'static str>> =
            STROKE_TYPES.iter().map(|&(label, kind)| Entry::new(kind, crate::i18n::tr(&lang, label).to_owned())).collect();
        let held = STROKE_TYPES.iter().find(|(_, kind)| *kind == current).map_or(Mixed::Mixed, |&(_, kind)| Mixed::Same(kind));
        let picked = Select::new(crate::i18n::tr(&lang, "Stroke Type").to_owned(), Bind::given(held), &entries)
            .id(id)
            .picture(|kind: &&'static str, cx: &ContentPainterCx<'_>, rect: Rect| paint_stroke_type(&cx.painter, rect, kind, cx.ink()))
            .picture_only()
            .show_at(ui, at)
            .events
            .committed()
            .copied();
        if let Some(kind) = picked {
            let _ = app.run("object.stroke", stroke_type_params(kind));
        }
    }

    /// The corner shape picker at `at` with the id `id`: the face is the current shape's glyph, the list each shape's
    /// glyph and name. A pick runs `object.cornerOptions` with the corner `size`. Properties › Appearance and the
    /// Control bar share it.
    pub(crate) fn corner_shape_select(app: &mut DesignApp, ui: &mut Ui, at: Rect, id: egui::Id, shape: CornerShape, size: f64) {
        let lang = app.ui.language.clone();
        let entries: Vec<Entry<CornerShape>> =
            CornerShape::ALL.into_iter().map(|s| Entry::new(s, crate::i18n::tr(&lang, s.label()).to_owned())).collect();
        let picked = Select::new(crate::i18n::tr(&lang, "Corner Shape").to_owned(), Bind::given(shape), &entries)
            .id(id)
            .picture(|shape: &CornerShape, cx: &ContentPainterCx<'_>, rect: Rect| paint_corner_shape(&cx.painter, rect, *shape, cx.ink()))
            .picture_only()
            .show_at(ui, at)
            .events
            .committed()
            .copied();
        if let Some(s) = picked {
            let _ = app.run("object.cornerOptions", corner_shape_params(s, size));
        }
    }

    /// A Fill (`stroke` false) or Stroke chip at `at` of `target`: a click opens and closes its swatch popover, a double
    /// click opens the Color Picker, as InDesign's do.
    pub(in crate::panels::properties) fn swatch_chip(
        app: &mut DesignApp,
        ui: &mut Ui,
        at: Rect,
        doc: &designcraft_doc::Document,
        swatch: &str,
        stroke: bool,
        target: SwatchFor,
    ) {
        let text = target == SwatchFor::Text;
        let (color, gradient) = widgets::swatch_colors(doc, swatch, 1.0);
        let chip = Chip { color, gradient: if stroke { None } else { gradient }, stroke, text };
        let name = crate::i18n::tr(&app.ui.language, if stroke { "Stroke" } else { "Fill" }).to_owned();
        let label = match (text, stroke) {
            (false, false) => "Fill swatch",
            (false, true) => "Stroke swatch",
            (true, false) => "Text fill swatch",
            (true, true) => "Text stroke swatch",
        };
        let id = control_id(ui, ("swatch", stroke, text));
        let out =
            IconButton::new(IconSpec::glyph(&chip)).id(id).label(label).tooltip(format!("{name}: {swatch}")).popup(PopupKind::Dialog).show_at(ui, at);
        // The popover's open state is the chip's: a pick closes it, as a swatch menu does.
        let open_id = id.with("open");
        let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
        if out.clicks >= 2 {
            open = false;
            open_color_picker(app, doc, swatch, stroke, text);
        } else if out.clicked() {
            open = !open;
        }
        if open {
            open = swatch_popover(app, ui, &out, doc, swatch, stroke, target);
        }
        ui.data_mut(|d| d.insert_temp(open_id, open));
    }

    /// The document's swatches, by position, as the swatch popover's list shows them.
    struct SwatchNames<'a>(&'a [String]);

    impl craft_behavior::DisplayOrder<usize> for SwatchNames<'_> {
        fn len(&self) -> usize {
            self.0.len()
        }
        fn key(&self, at: usize) -> usize {
            at
        }
        fn index_of(&self, key: &usize) -> Option<usize> {
            (*key < self.0.len()).then_some(*key)
        }
        fn text(&self, at: usize) -> Option<std::borrow::Cow<'_, str>> {
            self.0.get(at).map(|name| std::borrow::Cow::Borrowed(name.as_str()))
        }
    }

    impl craft_behavior::ItemSource<usize> for SwatchNames<'_> {}

    /// The Fill (or Stroke) popover hung from `opener`, as InDesign's: Apply To (a text frame's frame or its text), the
    /// tint, then the swatches. `cur` is the frames' swatch. Whether it is still open after this pass.
    fn swatch_popover(
        app: &mut DesignApp,
        ui: &mut Ui,
        opener: &ButtonOutput,
        doc: &designcraft_doc::Document,
        cur: &str,
        stroke: bool,
        target: SwatchFor,
    ) -> bool {
        let lang = app.ui.language.clone();
        let tr = |s: &'static str| crate::i18n::tr(&lang, s).to_owned();
        let to_text_id = apply_to_text_key(stroke);
        // Text frames offer Apply To Frame or Text; selected text is always coloured as text.
        let text_frame = target == (SwatchFor::Frames { text_frames: true });
        let to_text = target == SwatchFor::Text || text_frame && ui.data(|d| d.get_temp::<bool>(to_text_id)).unwrap_or(false);
        let (cur, tint) = swatch_target(app, cur, stroke, to_text);
        let names: Vec<String> = doc.swatches.iter().map(|sw| sw.name.clone()).collect();
        let current = names.iter().position(|name| *name == cur);
        let id = control_id(ui, ("swatch-popover", stroke, target == SwatchFor::Text));
        let mut open = true;
        let shown =
            Popover::new(tr(if stroke { "Stroke" } else { "Fill" }), Bind::bound(&mut open)).id(id).opened_by(&opener.response).show(ui, |ui, _| {
                let (mut switch, mut new_tint, mut picked) = (None, None, None);
                if text_frame {
                    Row::new().id(id.with("apply-to")).show(ui, |ui, _| {
                        Text::new(tr("Apply To:")).show(ui);
                        let entries = [Entry::new(false, tr("Frame")), Entry::new(true, tr("Text"))];
                        let out = Segmented::new(tr("Apply To:"), Bind::given(to_text), &entries).id(id.with("apply-to-choice")).show(ui);
                        if let Some(&text) = out.events.committed() {
                            switch = Some(text);
                        }
                    });
                }
                Row::new().id(id.with("tint")).show(ui, |ui, _| {
                    Text::new(tr("Tint:")).show(ui);
                    let out = NumberField::new(tr("Tint:"), Bind::given(Some((tint * 100.0).round())))
                        .id(id.with("tint-field"))
                        .unit_kind(UnitKind::Percent)
                        .range(0.0..=100.0)
                        .step(1.0)
                        .fixed_decimals(0)
                        .commit_decimals(0)
                        .show(ui);
                    if let Some(v) = out.edit.events.committed().copied().flatten().filter(|v| v.is_finite()) {
                        new_tint = Some(v / 100.0);
                    }
                });
                Divider::horizontal().show(ui);
                let source = SwatchNames(&names);
                let mut selection = current.map_or_else(|| Selection::new(Vec::new(), None, None), Selection::single);
                List::<usize>::new(tr("Swatches"))
                    .id(id.with("swatches"))
                    .select(SelectMode::Single)
                    .max_rows(14)
                    .selection(Bind::bound(&mut selection))
                    .show_items(ui, &source, &mut |key, _| ListRow::new(names[*key].clone()).icon(swatch_icon(doc, &names[*key])));
                let now = selection.items.first().copied();
                if now != current {
                    picked = now.and_then(|at| names.get(at).cloned());
                }
                (switch, new_tint, picked)
            });
        // The body's last run is the pass's real one: what it picked is what to apply.
        let (switch, new_tint, picked) = shown.inner.unwrap_or((None, None, None));
        if let Some(text) = switch.filter(|text| *text != to_text) {
            ui.data_mut(|d| d.insert_temp(to_text_id, text));
        }
        if picked.is_some() {
            // A swatch picked closes the popover, as a pick from the native list closes its menu.
            open = false;
        }
        apply_swatch(app, stroke, to_text, picked, new_tint);
        open
    }

    pub(super) fn align_row(app: &mut DesignApp, ui: &mut Ui) {
        let lang = app.ui.language.clone();
        let (to, key, tip) = align_target(app);
        let b = block(ui, FIELD_H);
        let opener = IconButton::new(IconSpec::glyph(&Artwork(align_to_icon(&to))))
            .id(control_id(ui, "align-to"))
            .label(tip.clone())
            .tooltip(tip)
            .popup(PopupKind::Menu)
            .show_at(ui, sub(b, 0.0, 0.0, 23.0, FIELD_H));
        let picked = Menu::new(crate::i18n::tr(&lang, "Align To").to_owned())
            .id(control_id(ui, "align-to-menu"))
            .opened_by(&opener, Trigger::Click)
            .show(ui, |ui, _| {
                let mut picked = None;
                for (label, id) in ALIGN_TO {
                    if MenuItem::new(crate::i18n::tr(&lang, label).to_owned()).id_salt(id).radio_mark(to == id).show(ui).ran {
                        picked = Some(id);
                    }
                }
                picked
            })
            .inner
            .flatten();
        if let Some(id) = picked {
            pick_align_to(app, key, id);
        }
        Divider::vertical().show_at(ui, sub(b, 31.0, 0.0, 1.0, FIELD_H));
        for (k, (edge, artwork, tip)) in ALIGN_EDGES.into_iter().enumerate() {
            let (at, id) = (sub(b, 40.0 + k as f32 * 31.0, 0.0, 23.0, FIELD_H), control_id(ui, ("align", edge)));
            if icon_button(ui, at, id, artwork, crate::i18n::tr(&lang, tip), false) {
                let _ = app.run("object.align", json!({"edge": edge, "to": to}));
            }
        }
    }

    pub(super) fn wrap_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
        let lang = app.ui.language.clone();
        let b = block(ui, FIELD_H);
        for (k, (artwork, mode, tip)) in WRAP_MODES.into_iter().enumerate() {
            let (at, id) = (sub(b, k as f32 * 30.0, 0.0, 23.0, FIELD_H), control_id(ui, ("wrap", mode)));
            if icon_button(ui, at, id, artwork, crate::i18n::tr(&lang, tip), i.wrap == mode) {
                let _ = app.run("object.textWrap", json!({"mode": mode}));
            }
        }
        let invert = Checkbox::new(Bind::given(i.wrap_invert))
            .id(control_id(ui, "wrap-invert"))
            .label(crate::i18n::tr(&lang, "Invert").to_owned())
            .enabled(i.wrap != "none")
            .show_at(ui, Rect::from_min_max(b.min + vec2(152.0, 0.0), b.max));
        if let Some(&inv) = invert.events.committed().filter(|inv| **inv != i.wrap_invert) {
            // Each object keeps its own mode.
            let _ = app.run("object.textWrap", json!({"mode": null, "invert": inv}));
        }
        wrap_offsets(app, ui, i);
        if more_options(app, ui, "wrap") {
            show_panel(app, "textWrap");
        }
    }

    /// The wrap offsets, Top and Bottom, the link that makes all four the same, then Left and Right; disabled with no
    /// wrap.
    fn wrap_offsets(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
        let lang = app.ui.language.clone();
        let u = units(app);
        let enabled = i.wrap != "none";
        let offsets = i.wrap_offsets;
        ui.add_space(4.0);
        let b = block(ui, 2.0 * FIELD_H + 5.0);
        let link_id = wrap_link_key(app);
        // Linked while the four are the same, until the link is clicked.
        let same = offsets.iter().all(|v| (v - offsets[0]).abs() < 1e-9);
        let linked = ui.data(|d| d.get_temp::<bool>(link_id)).unwrap_or(same);
        let mut set = None;
        for (k, key, artwork, tip, x, y) in WRAP_OFFSETS {
            let tip = crate::i18n::tr(&lang, tip);
            icon(ui, sub(b, x, y + 2.0, 17.0, 17.0), artwork, Some(tip), enabled);
            let length = Length { value: Some(offsets[k]), unit: u, range: Some(-8000.0..=8000.0) };
            let id = control_id(ui, ("wrap-offset", key));
            if let (Some(v), _) = length_field(ui, sub(b, x + 20.0, y, 76.0, FIELD_H), id, tip, length, enabled) {
                set = Some(wrap_offsets_set(linked, k, v));
            }
        }
        let tip = crate::i18n::tr(&lang, "Make all settings the same").to_owned();
        let toggled = link_toggle(tip.clone(), Bind::given(linked))
            .id(control_id(ui, "wrap-link"))
            .tooltip(tip)
            .enabled(enabled)
            .show_at(ui, sub(b, 106.0, (FIELD_H + 5.0) / 2.0, 22.0, FIELD_H))
            .events
            .committed()
            .copied();
        if let Some(on) = toggled {
            ui.data_mut(|d| d.insert_temp(link_id, on));
        }
        if let Some(o) = set {
            // Each object keeps its own mode: only the offsets change.
            let _ = app.run("object.textWrap", json!({"mode": null, "offset": o}));
        }
    }

    /// The quick action button `key` at `at`.
    fn action(ui: &mut Ui, at: Rect, key: &'static str, label: &str) -> ButtonOutput {
        let id = control_id(ui, ("action", key));
        Button::new(label.to_owned()).id(id).show_at(ui, at)
    }

    /// The Arrange menu hung from `opener`.
    fn arrange_menu(app: &mut DesignApp, ui: &mut Ui, opener: &ButtonOutput) {
        let lang = app.ui.language.clone();
        let picked = Menu::new(crate::i18n::tr(&lang, "Arrange").to_owned())
            .id(control_id(ui, "arrange-menu"))
            .opened_by(opener, Trigger::Click)
            .show(ui, |ui, _| {
                let mut picked = None;
                for (label, cmd) in ARRANGE {
                    if MenuItem::new(crate::i18n::tr(&lang, label).to_owned()).id_salt(cmd).show(ui).ran {
                        picked = Some(cmd);
                    }
                }
                picked
            })
            .inner
            .flatten();
        if let Some(cmd) = picked {
            let _ = app.run(cmd, json!({}));
        }
    }

    /// The Convert Shape menu hung from `opener`.
    fn convert_shape_menu(app: &mut DesignApp, ui: &mut Ui, opener: &ButtonOutput) {
        let lang = app.ui.language.clone();
        let picked = Menu::new(crate::i18n::tr(&lang, "Convert Shape").to_owned())
            .id(control_id(ui, "convert-shape-menu"))
            .opened_by(opener, Trigger::Click)
            .show(ui, |ui, _| {
                let mut picked = None;
                for (label, to) in CONVERT_SHAPES {
                    if MenuItem::new(crate::i18n::tr(&lang, label).to_owned()).id_salt(to).show(ui).ran {
                        picked = Some(to);
                    }
                }
                picked
            })
            .inner
            .flatten();
        if let Some(to) = picked {
            convert_shape(app, to);
        }
    }

    pub(super) fn quick_actions_object(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
        let lang = app.ui.language.clone();
        let tr = |s: &'static str| crate::i18n::tr(&lang, s).to_owned();
        let full = full_width(ui);
        let half = (full - 10.0) / 2.0;
        let h = widgets::BUTTON_H;
        if i.is_text {
            let b = block(ui, h);
            let convert = action(ui, sub(b, 0.0, 0.0, half, h), "convert-shape", &tr("Convert Shape"));
            convert_shape_menu(app, ui, &convert);
            let arrange = action(ui, sub(b, half + 10.0, 0.0, half, h), "arrange", &tr("Arrange"));
            arrange_menu(app, ui, &arrange);
            ui.add_space(4.0);
            let b = block(ui, h);
            if action(ui, sub(b, 0.0, 0.0, full, h), "placeholder", &tr("Fill with Placeholder Text")).clicked() {
                let _ = app.run("type.fillWithPlaceholder", json!({}));
            }
            ui.add_space(4.0);
            let b = block(ui, h);
            if action(ui, sub(b, 0.0, 0.0, full, h), "story-editor", &tr("Edit in Story Editor")).clicked() {
                let _ = app.run("app.storyEditor", json!({}));
            }
            return;
        }
        let b = block(ui, h);
        let arrange = action(ui, sub(b, 0.0, 0.0, half, h), "arrange", &tr("Arrange"));
        arrange_menu(app, ui, &arrange);
        let (label, cmd) = group_or_lock(i);
        if action(ui, sub(b, half + 10.0, 0.0, half, h), "group-or-lock", &tr(label)).clicked() {
            let _ = app.run(cmd, json!({}));
        }
        ui.add_space(4.0);
        let b = block(ui, h);
        if i.kind == "<group>" {
            if action(ui, sub(b, 0.0, 0.0, full, h), "ungroup", &tr("Ungroup")).clicked() {
                let _ = app.run("object.ungroup", json!({}));
            }
        } else {
            let convert = action(ui, sub(b, 0.0, 0.0, full, h), "convert-shape", &tr("Convert Shape"));
            convert_shape_menu(app, ui, &convert);
        }
    }
}
