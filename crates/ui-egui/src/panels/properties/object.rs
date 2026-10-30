use super::{
    CornerShape, DesignApp, FIELD_H, NumField, Rect, SelInfo, Sense, Stroke, StrokeKind, Tokens, Ui, Unit, Value, block, full_width, icon_toggle,
    icons, json, link_label, measure, more_options, outline_button, place, pos2, set_temp_bool, show_panel, sub, temp_bool, units, vec2, widgets,
};

// ---------------------------------------------------------------- Objects

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

pub(super) fn transform_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    let u = units(app);
    let rp = app.ui.ref_point;
    let b = block(ui, 2.0 * FIELD_H + 4.0);
    // Reference-point position of the bounds.
    let r = i.page_rect;
    let (fx, fy) = ((rp % 3) as f64 / 2.0, (rp / 3) as f64 / 2.0);
    let (x, y) = (r.x0 + r.width() * fx, r.y0 + r.height() * fy);
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

/// Properties › Transform for selected ruler guides: X for one vertical guide, Y for one
/// horizontal guide; with several guides both fields are empty.
pub(super) fn guide_section(app: &mut DesignApp, ui: &mut Ui) {
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
    let rows = [
        [("tool-rotate", "Rotation Angle", "rotation", "°", "p_rot"), ("tool-scale", "Scale X Percentage", "scaleX", "%", "p_sx")],
        [("tool-shear", "Shear X Angle", "shear", "°", "p_shear"), ("tool-scale", "Scale Y Percentage", "scaleY", "%", "p_sy")],
    ];
    for (row, cells) in rows.into_iter().enumerate() {
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
        for (icon, tip, cmd, params) in [
            ("rotate-clockwise", "Rotate 90° Clockwise", "transform.rotate", json!({"angle": -90})),
            ("tool-rotate", "Rotate 90° Counterclockwise", "transform.rotate", json!({"angle": 90})),
            ("flip-horizontal", "Flip Horizontal", "transform.flip", json!({"axis": "horizontal"})),
            ("flip-vertical", "Flip Vertical", "transform.flip", json!({"axis": "vertical"})),
        ] {
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
    const TYPES: &[(&str, &str)] = &[
        ("Solid", "solid"),
        ("Dashed", "dashed"),
        ("Dotted", "dotted"),
        ("Thick - Thin", "thickThin"),
        ("Thin - Thick", "thinThick"),
        ("Thin - Thin", "thinThin"),
        ("Thick - Thick", "thickThick"),
    ];
    let resp = preview_dropdown(ui, w, |p, r| paint_stroke_type(p, r, cur));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(140.0);
        for (label, kind) in TYPES {
            if ui.selectable_label(*kind == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                let ty = if *kind == "dashed" { json!({"kind": "dashed", "pattern": [12.0, 4.0]}) } else { json!({"kind": kind}) };
                let _ = app.run("object.stroke", json!({"type": ty}));
                ui.close();
            }
        }
    });
}

fn paint_stroke_type(p: &egui::Painter, r: Rect, kind: &str) {
    let c = egui::Color32::BLACK;
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

/// A dropdown whose face is a light preview box (stroke type, corner shape).
pub(super) fn preview_dropdown(ui: &mut Ui, w: f32, paint: impl FnOnce(&egui::Painter, Rect)) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(w, FIELD_H), Sense::click());
    let face = Rect::from_min_max(r.min, pos2(r.max.x - widgets::SEG_W, r.max.y));
    ui.painter().rect_filled(face, 0.0, egui::Color32::from_gray(0xdf));
    paint(ui.painter(), face);
    let seg = Rect::from_min_max(pos2(face.max.x, r.min.y), r.max);
    ui.painter().rect_filled(seg, 0.0, if resp.hovered() { t.hover } else { t.input });
    widgets::chevron_down(ui.painter(), seg.center(), t.icon);
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    resp
}

fn paint_corner_shape(p: &egui::Painter, r: Rect, shape: CornerShape) {
    let c = egui::Color32::BLACK;
    let s = Stroke::new(1.5, c);
    let (x0, y0, x1, y1) = (r.min.x + 4.0, r.min.y + 4.0, r.max.x - 9.0, r.max.y - 3.0);
    let k = 8.0;
    let pts: Vec<egui::Pos2> = match shape {
        CornerShape::None => vec![pos2(x0, y0), pos2(x1, y0), pos2(x1, y1)],
        CornerShape::Bevel => vec![pos2(x0, y0), pos2(x1 - k, y0), pos2(x1, y0 + k), pos2(x1, y1)],
        CornerShape::Inset => vec![pos2(x0, y0), pos2(x1 - k, y0), pos2(x1 - k, y0 + k), pos2(x1, y0 + k), pos2(x1, y1)],
        CornerShape::Rounded | CornerShape::InverseRounded | CornerShape::Fancy => {
            let mut v = vec![pos2(x0, y0)];
            for i in 0..=8 {
                let a = i as f32 / 8.0 * std::f32::consts::FRAC_PI_2;
                v.push(if shape == CornerShape::InverseRounded {
                    pos2(x1 - k * a.cos(), y0 + k * a.sin())
                } else {
                    pos2(x1 - k + k * a.sin(), y0 + k - k * a.cos())
                });
            }
            v.push(pos2(x1, y1));
            v
        }
    };
    p.add(egui::Shape::line(pts, s));
}

pub(crate) fn corner_type_dropdown(app: &mut DesignApp, ui: &mut Ui, shape: CornerShape, size: f64, width: f32) {
    let resp = preview_dropdown(ui, width, |p, r| paint_corner_shape(p, r, shape));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(140.0);
        for s in CornerShape::ALL {
            if ui.selectable_label(s == shape, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, s.label()))).clicked() {
                let _ = app.run("object.cornerOptions", json!({"shape": s, "size": if size > 0.0 { size } else { 12.0 }}));
                ui.close();
            }
        }
    });
}

pub(super) fn appearance_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    let t = Tokens::get(ui.ctx());
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
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
    swatch_menu(app, &resp, &doc, &i.fill, false, i.is_text && i.count == 1, false);
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
    swatch_menu(app, &resp, &doc, &i.stroke, true, i.is_text && i.count == 1, false);
    if place(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), |ui| link_label(ui, crate::i18n::tr(&app.ui.language, "Stroke"), 56.0)).clicked() {
        show_panel(app, "stroke");
    }
    const WEIGHTS: &[f64] = &[0.0, 0.25, 0.5, 0.75, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 20.0, 30.0, 40.0, 50.0, 100.0];
    if let Some(v) = place(ui, sub(b, 96.0, 0.0, 69.0, FIELD_H), |ui| {
        NumField::number("sw", Some(i.stroke_weight), " pt", 3).width(69.0).spinner().presets(WEIGHTS).range(0.0, 800.0).show(ui)
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
        app.ui.dialog = Some(crate::dialogs::Dialog::new("frameSize", json!({})));
    }
    let (shape, size) = i.corner;
    if let Some(v) = place(ui, sub(b, 96.0, 0.0, 69.0, FIELD_H), |ui| {
        NumField::measure("corner", Some(size), Unit::Points).width(69.0).spinner().range(0.0, 1000.0).show(ui)
    }) {
        let sh = if shape == CornerShape::None { CornerShape::Rounded } else { shape };
        let _ = app.run("object.cornerOptions", json!({"shape": sh, "size": v}));
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
    if let Some(v) = place(ui, sub(b, 96.0, 0.0, 51.0, FIELD_H), |ui| {
        NumField::number("opacity", Some((i.opacity * 100.0).round()), "%", 0).width(51.0).range(0.0, 100.0).show(ui)
    }) {
        let _ = app.run("object.opacity", json!({"opacity": v / 100.0}));
    }
    let more = sub(b, 147.0, 0.0, 18.0, FIELD_H);
    let mresp = ui.interact(more, ui.scope_id().with("opmore"), Sense::click());
    ui.painter().rect(more, 0.0, if mresp.hovered() { t.hover } else { t.input }, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    icons::paint(ui.painter(), more.shrink(4.0), "chevron-right", t.icon);
    if mresp.clicked() {
        show_panel(app, "effects");
    }
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
    let to_text_id = egui::Id::unique(("swatch_apply_to_text", stroke));
    let to_text = text_only || text_frame && resp.ctx.data(|d| d.get_temp::<bool>(to_text_id)).unwrap_or(false);
    // What the popup shows and changes: the frame's fill or stroke, or its text's.
    let (key, tint_key) = if stroke { ("stroke", "strokeTint") } else { ("fill", "fillTint") };
    let (cur, tint) = if to_text {
        let chars = crate::panels::text_attrs(app).map(|a| a["chars"].clone()).unwrap_or_default();
        (chars[key].as_str().unwrap_or("").to_string(), chars[tint_key].as_f64().unwrap_or(1.0))
    } else {
        (cur.to_string(), f64::from(crate::panels::sel_tint(app, stroke)))
    };
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
    let cmd = |app: &mut DesignApp, attrs: Value| {
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
    };
    if let Some(n) = picked {
        cmd(app, json!({ key: n }));
    } else if let Some(t) = new_tint {
        cmd(app, json!({ tint_key: t }));
    }
}

pub(super) fn align_row(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        // A key object switches Align To to Key Object; choosing another target drops the key.
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
        let resp = icon_toggle(ui, "align-to", false, &tip);
        egui::Popup::menu(&resp).show(|ui| {
            for (label, id) in [
                ("Align to Selection", "selection"),
                ("Align to Key Object", "keyObject"),
                ("Align to Margins", "margins"),
                ("Align to Page", "page"),
                ("Align to Spread", "spread"),
            ] {
                if ui.selectable_label(to == id, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    if let Some(k) = key.filter(|_| id != "keyObject") {
                        let _ = app.run("selection.key", json!({"id": k.0}));
                    }
                    app.ui.align_to = id.into();
                    ui.close();
                }
            }
        });
        let (r, _) = ui.allocate_exact_size(vec2(1.0, FIELD_H), Sense::hover());
        ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, t.section_divider.gamma_multiply(1.6)));
        for (edge, icon, tip) in [
            ("left", "objalign-left", "Align left edges"),
            ("hcenter", "objalign-hcenter", "Align horizontal centers"),
            ("right", "objalign-right", "Align right edges"),
            ("top", "objalign-top", "Align top edges"),
            ("vcenter", "objalign-vcenter", "Align vertical centers"),
            ("bottom", "objalign-bottom", "Align bottom edges"),
        ] {
            if icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.align", json!({"edge": edge, "to": to}));
            }
        }
    });
}

pub(super) fn wrap_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 7.0;
        for (icon, mode, tip) in [
            ("wrap-none", "none", "No Text Wrap"),
            ("wrap-bbox", "boundingBox", "Wrap Around Bounding Box"),
            ("wrap-contour", "contour", "Wrap Around Object Shape"),
            ("wrap-jump", "jumpObject", "Jump Object"),
            ("wrap-next", "jumpToNextColumn", "Jump to Next Column"),
        ] {
            if icon_toggle(ui, icon, i.wrap == mode, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.textWrap", json!({"mode": mode}));
            }
        }
        ui.add_space(2.0);
        let mut inv = i.wrap_invert;
        let enabled = i.wrap != "none";
        let r = ui.add_enabled_ui(enabled, |ui| widgets::checkbox(ui, &mut inv, crate::i18n::tr(&app.ui.language, "Invert"))).inner;
        if r.changed() {
            let _ = app.run("object.textWrap", json!({"mode": i.wrap, "invert": inv}));
        }
    });
    if more_options(ui, &app.ui.language).clicked() {
        show_panel(app, "textWrap");
    }
}

fn arrange_menu(app: &mut DesignApp, resp: &egui::Response) {
    egui::Popup::menu(resp).show(|ui| {
        for (label, cmd) in [
            ("Bring to Front", "object.bringToFront"),
            ("Bring Forward", "object.bringForward"),
            ("Send Backward", "object.sendBackward"),
            ("Send to Back", "object.sendToBack"),
        ] {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                let _ = app.run(cmd, json!({}));
                ui.close();
            }
        }
    });
}

fn convert_shape_menu(app: &mut DesignApp, resp: &egui::Response) {
    egui::Popup::menu(resp).show(|ui| {
        for (label, to) in [
            ("Rectangle", "rectangle"),
            ("Rounded Rectangle", "roundedRectangle"),
            ("Beveled Rectangle", "beveledRectangle"),
            ("Inverse Rounded Rectangle", "inverseRoundedRectangle"),
            ("Ellipse", "ellipse"),
            ("Triangle", "triangle"),
            ("Polygon", "polygon"),
            ("Line", "line"),
            ("Orthogonal Line", "orthogonalLine"),
        ] {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                if let Err(e) = app.run("object.convertShape", json!({"to": to})) {
                    app.status_error("Convert Shape", e);
                }
                ui.close();
            }
        }
    });
}

pub(super) fn quick_actions_object(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
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
    if i.is_graphic && i.count == 1 {
        super::text::image_actions(app, ui, hw);
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let r = outline_button(ui, crate::i18n::tr(&app.ui.language, "Arrange"), hw);
        arrange_menu(app, &r);
        if outline_button(ui, crate::i18n::tr(&app.ui.language, if i.count > 1 { "Group" } else { "Lock" }), hw).clicked() {
            let _ = app.run(if i.count > 1 { "object.group" } else { "object.lock" }, json!({}));
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
