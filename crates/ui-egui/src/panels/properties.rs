//! The Properties panel (context-sensitive, laid out as InDesign 2026's: selection-type band,
//! sections per selection state — `plan/indesign/11-observed-ui.md` §4.1) and small
//! single-purpose panels (Stroke, Character, Paragraph, Text Wrap, Align, Links, Info).

use designcraft_doc::{Align, Arrowhead, Cap, Join, StrokeAlign, StrokeType};
use designcraft_geom::Unit;
use designcraft_geom::corners::CornerShape;
use egui::{Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use super::{SelInfo, sel_info, text_attrs};
use crate::theme::Tokens;
use crate::widgets::{
    self, FIELD_H, NumField, button_pair, caption, divider, dropdown, full_width, icon_toggle, link_label, measure, more_options, number,
    outline_button, rule, section, section_row, selection_band,
};
use crate::{DesignApp, icons};

fn units(app: &DesignApp) -> Unit {
    app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas)
}

/// Allocate a full-width block `h` tall.
fn block(ui: &mut Ui, h: f32) -> Rect {
    ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover()).0
}

/// A sub-rect of a block at (`x`, `y`) relative to its top-left.
fn sub(b: Rect, x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(b.min + vec2(x, y), vec2(w, h))
}

/// Run `f` in a child ui occupying `r` (left-to-right, vertically centred). The parent's cursor
/// does not move: callers allocate the whole block first with [`block`].
pub(crate) fn place<R>(ui: &mut Ui, r: Rect, f: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)));
    f(&mut child)
}

fn temp_bool(ui: &Ui, key: &str, default: bool) -> bool {
    ui.data(|d| d.get_temp::<bool>(egui::Id::new(key))).unwrap_or(default)
}

fn set_temp_bool(ui: &Ui, key: &str, v: bool) {
    ui.data_mut(|d| d.insert_temp(egui::Id::new(key), v));
}

/// Label of the current selection for the selection-type band.
fn selection_title(app: &DesignApp, info: &Option<SelInfo>) -> String {
    let Some(st) = app.session.active() else { return String::new() };
    if st.selection.text.is_some() {
        return crate::i18n::tr(&app.ui.language, "Characters").into();
    }
    match info {
        None => crate::i18n::tr(&app.ui.language, "No Selection").into(),
        Some(i) if i.count > 1 => crate::i18n::tr(&app.ui.language, "Multiple Objects").into(),
        Some(i) => match i.kind {
            "<text frame>" => crate::i18n::tr(&app.ui.language, "Text Frame").into(),
            "<group>" => crate::i18n::tr(&app.ui.language, "Group").into(),
            "<image>" => crate::i18n::tr(&app.ui.language, "Image").into(),
            "<rectangle>" => crate::i18n::tr(&app.ui.language, "Rectangle").into(),
            "<ellipse>" => crate::i18n::tr(&app.ui.language, "Ellipse").into(),
            "<polygon>" => crate::i18n::tr(&app.ui.language, "Polygon").into(),
            "<line>" => crate::i18n::tr(&app.ui.language, "Line").into(),
            _ => crate::i18n::tr(&app.ui.language, "Path").into(),
        },
    }
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    if app.session.active().is_none() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No document open")).color(t.text_dim));
        return;
    }
    ui.spacing_mut().item_spacing = vec2(6.0, 5.0);
    let info = sel_info(app);
    let has_text = app.session.active().is_some_and(|s| s.selection.text.is_some());
    selection_band(ui, &selection_title(app, &info));
    if has_text {
        text_style_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Appearance"), true);
        text_appearance_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Character"), true);
        character_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Paragraph"), true);
        paragraph_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Bullets and Numbering"), true);
        bullets_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Quick Actions"), true);
        quick_actions_text(app, ui);
        return;
    }
    let Some(i) = info else {
        document_sections(app, ui);
        return;
    };
    section(ui, crate::i18n::tr(&app.ui.language, "Transform"), true);
    transform_section(app, ui, &i);
    divider(ui);
    if i.is_text && i.count == 1 {
        text_style_section(app, ui);
        divider(ui);
    }
    section(ui, crate::i18n::tr(&app.ui.language, "Appearance"), true);
    appearance_section(app, ui, &i);
    divider(ui);
    if i.is_text && i.count == 1 {
        section(ui, crate::i18n::tr(&app.ui.language, "Character"), true);
        character_section(app, ui);
        divider(ui);
        section(ui, crate::i18n::tr(&app.ui.language, "Paragraph"), true);
        paragraph_section(app, ui);
        divider(ui);
    }
    section(ui, crate::i18n::tr(&app.ui.language, "Align"), true);
    align_row(app, ui);
    divider(ui);
    if i.is_text && i.count == 1 {
        section(ui, crate::i18n::tr(&app.ui.language, "Text Frame"), true);
        text_frame_section(app, ui);
        divider(ui);
    } else {
        if i.is_graphic {
            section(ui, crate::i18n::tr(&app.ui.language, "Frame Fitting"), true);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (icon, mode, tip) in [
                    ("fit-fill", "fillProportionally", "Fill Frame Proportionally"),
                    ("fit-prop", "fitProportionally", "Fit Content Proportionally"),
                    ("fit-content", "fitContentToFrame", "Fit Content to Frame"),
                    ("fit-frame", "fitFrameToContent", "Fit Frame to Content"),
                    ("fit-center", "centerContent", "Center Content"),
                ] {
                    if icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                        let _ = app.run("object.fit", json!({"mode": mode}));
                    }
                }
            });
            divider(ui);
        }
        section(ui, crate::i18n::tr(&app.ui.language, "Text Wrap"), true);
        wrap_section(app, ui, &i);
        divider(ui);
    }
    section(ui, crate::i18n::tr(&app.ui.language, "Quick Actions"), true);
    quick_actions_object(app, ui, &i);
}

// ---------------------------------------------------------------- No Selection

/// Page-size presets for the Document dropdown (portrait, points).
const PAGE_PRESETS: &[(&str, f64, f64)] = &[
    ("Letter", 612.0, 792.0),
    ("Legal", 612.0, 1008.0),
    ("Tabloid", 792.0, 1224.0),
    ("Letter - Half", 396.0, 612.0),
    ("Legal - Half", 504.0, 612.0),
    ("A3", 841.89, 1190.55),
    ("A4", 595.276, 841.89),
    ("A5", 419.528, 595.276),
    ("B5", 498.898, 708.661),
];

fn preset_name(w: f64, h: f64) -> &'static str {
    let (a, b) = (w.min(h), w.max(h));
    PAGE_PRESETS.iter().find(|p| (p.1 - a).abs() < 0.6 && (p.2 - b).abs() < 0.6).map(|p| p.0).unwrap_or("Custom")
}

fn document_sections(app: &mut DesignApp, ui: &mut Ui) {
    let u = units(app);
    let Some(st) = app.session.active() else { return };
    let d = st.doc.clone();
    let (pw, ph) = (d.settings.page_width, d.settings.page_height);
    section(ui, crate::i18n::tr(&app.ui.language, "Document"), true);
    // Preset dropdown + orientation.
    let b = block(ui, FIELD_H);
    let mut preset = None;
    place(ui, sub(b, 0.0, 0.0, 93.0, FIELD_H), |ui| {
        let resp = dropdown(ui, preset_name(pw, ph), 93.0);
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(140.0);
            for p in PAGE_PRESETS {
                if ui.selectable_label(preset_name(pw, ph) == p.0, p.0).clicked() {
                    preset = Some((p.1, p.2));
                    ui.close();
                }
            }
        });
    });
    let landscape = pw > ph;
    let mut orient = None;
    place(ui, sub(b, 126.0, 0.0, 52.0, FIELD_H), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if icon_toggle(ui, "orient-portrait", !landscape, crate::i18n::tr(&app.ui.language, "Portrait")).clicked() && landscape {
            orient = Some(());
        }
        if icon_toggle(ui, "orient-landscape", landscape, crate::i18n::tr(&app.ui.language, "Landscape")).clicked() && !landscape {
            orient = Some(());
        }
    });
    if let Some((w, h)) = preset {
        let (w, h) = if landscape { (h, w) } else { (w, h) };
        let _ = app.run("layout.documentSetup", json!({"width": w, "height": h}));
    }
    if orient.is_some() {
        let _ = app.run("layout.documentSetup", json!({"width": ph, "height": pw}));
    }
    ui.add_space(3.0);
    // W / H, page count, facing pages.
    let b = block(ui, 2.0 * FIELD_H + 5.0);
    for (row, (label, v, key)) in [("W:", pw, "width"), ("H:", ph, "height")].into_iter().enumerate() {
        let y = row as f32 * 26.0;
        place(ui, sub(b, 0.0, y, 20.0, FIELD_H), |ui| widgets::caption_w(ui, label, 20.0));
        let nv =
            place(ui, sub(b, 21.0, y, 72.0, FIELD_H), |ui| NumField::measure(key, Some(v), u).width(72.0).spinner().range(1.0, 15552.0).show(ui));
        if let Some(nv) = nv {
            let _ = app.run("layout.documentSetup", json!({key: nv}));
        }
    }
    let n = d.page_count();
    let t = Tokens::get(ui.ctx());
    icons::paint(ui.painter(), sub(b, 127.0, 2.0, 17.0, 17.0), "page-count", t.icon);
    let nv = place(ui, sub(b, 150.0, 0.0, 68.0, FIELD_H), |ui| number(ui, "pagecount", Some(n as f64), "", 68.0, 0));
    if let Some(nv) = nv {
        let want = (nv.round() as usize).clamp(1, 9999);
        if want > n {
            let _ = app.run("layout.pages.insert", json!({"count": want - n, "after": n - 1}));
        } else if want < n {
            let _ = app.run("layout.pages.delete", json!({"pages": (want..n).collect::<Vec<_>>()}));
        }
    }
    let mut facing = d.settings.facing_pages;
    let changed = place(ui, sub(b, 126.0, 26.0, 100.0, FIELD_H), |ui| {
        widgets::checkbox(ui, &mut facing, crate::i18n::tr(&app.ui.language, "Facing Pages")).changed()
    });
    if changed {
        let _ = app.run("layout.documentSetup", json!({"facingPages": facing}));
    }
    // Margins.
    section(ui, crate::i18n::tr(&app.ui.language, "Margins"), true);
    let cur = crate::canvas::current_page(app).unwrap_or(0);
    if let Some(p) = d.page(cur).cloned() {
        let linked = temp_bool(ui, "margins_linked", true);
        let b = block(ui, 2.0 * FIELD_H + 5.0);
        let mut set: Option<designcraft_doc::Margins> = None;
        let fields = [
            ("margin-top", "Top", p.margins.top, 0.0, 0.0),
            ("margin-bottom", "Bottom", p.margins.bottom, 0.0, 26.0),
            ("margin-outside", if d.settings.facing_pages { "Outside" } else { "Right" }, p.margins.outside, 127.0, 0.0),
            ("margin-inside", if d.settings.facing_pages { "Inside" } else { "Left" }, p.margins.inside, 127.0, 26.0),
        ];
        for (k, (icon, tip, v, x, y)) in fields.into_iter().enumerate() {
            let ir = sub(b, x, y + 2.0, 17.0, 17.0);
            icons::paint(ui.painter(), ir, icon, t.icon);
            ui.interact(ir, ui.id().with(("micon", k)), Sense::hover()).on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, tip));
            });
            let nv = place(ui, sub(b, x + 20.0, y, 73.0, FIELD_H), |ui| {
                NumField::measure(tip, Some(v), u).width(73.0).spinner().range(0.0, 8000.0).show(ui)
            });
            if let Some(nv) = nv {
                let mut m = p.margins;
                if linked {
                    m = designcraft_doc::Margins { top: nv, bottom: nv, inside: nv, outside: nv };
                } else {
                    match k {
                        0 => m.top = nv,
                        1 => m.bottom = nv,
                        2 => m.outside = nv,
                        _ => m.inside = nv,
                    }
                }
                set = Some(m);
            }
        }
        let lr = sub(b, 99.0, 12.5, 22.0, FIELD_H);
        let clicked = place(ui, lr, |ui| {
            widgets::icon_toggle_sized(ui, if linked { "link" } else { "link-broken" }, linked, "Make all settings the same", vec2(22.0, 21.0))
                .clicked()
        });
        if clicked {
            set_temp_bool(ui, "margins_linked", !linked);
        }
        if let Some(m) = set {
            let _ = app.run("layout.marginsAndColumns", json!({"margins": m}));
        }
    }
    ui.add_space(6.0);
    let fw = full_width(ui);
    if outline_button(ui, crate::i18n::tr(&app.ui.language, "Adjust Layout"), fw).clicked() {
        crate::dialogs::open_document_setup(app);
    }
    divider(ui);
    // Page.
    section(ui, crate::i18n::tr(&app.ui.language, "Page"), true);
    let b = block(ui, FIELD_H);
    let names: Vec<String> = (0..d.page_count()).map(|i| app.session.page_label(i)).collect();
    let picked = place(ui, sub(b, 0.0, 0.0, 93.0, FIELD_H), |ui| {
        widgets::dropdown_list(ui, &names[cur.min(names.len().saturating_sub(1))], 93.0, &names, Some(cur))
    });
    if let Some(pg) = picked {
        crate::canvas::go_to_page(app, pg);
    }
    let (add, del) = place(ui, sub(b, 172.0, 0.0, 50.0, FIELD_H), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let a = widgets::icon_toggle_sized(ui, "style-new", false, "Insert Page", vec2(22.0, 21.0)).clicked();
        let d = widgets::icon_toggle_sized(ui, "trash", false, "Delete Page", vec2(22.0, 21.0)).clicked();
        (a, d)
    });
    if add {
        let _ = app.run("layout.pages.insert", json!({"after": cur, "count": 1}));
    }
    if del {
        let _ = app.run("layout.pages.delete", json!({"pages": [cur]}));
    }
    ui.add_space(3.0);
    if outline_button(ui, crate::i18n::tr(&app.ui.language, "Edit Page"), fw).clicked() {
        crate::dialogs::open_document_setup(app);
    }
    divider(ui);
    // Rulers & Grids / Guides: icon toggle rows.
    let (mut rulers, mut bl, mut dg) = (app.ui.rulers, app.ui.baseline_grid, app.ui.document_grid);
    section_row(ui, crate::i18n::tr(&app.ui.language, "Rulers & Grids"), |ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        if icon_toggle(ui, "grid-document", dg, crate::i18n::tr(&app.ui.language, "Show Document Grid")).clicked() {
            dg = !dg;
        }
        if icon_toggle(ui, "grid-baseline", bl, crate::i18n::tr(&app.ui.language, "Show Baseline Grid")).clicked() {
            bl = !bl;
        }
        if icon_toggle(ui, "rulers", rulers, crate::i18n::tr(&app.ui.language, "Show Rulers")).clicked() {
            rulers = !rulers;
        }
    });
    if rulers != app.ui.rulers {
        let _ = app.run("view.rulers", json!({}));
    }
    if bl != app.ui.baseline_grid {
        let _ = app.run("view.baselineGrid", json!({}));
    }
    app.ui.document_grid = dg;
    rule(ui);
    let (mut guides, mut locked, mut smart) = (app.ui.guides, app.ui.guides_locked, app.ui.smart_guides);
    section_row(ui, crate::i18n::tr(&app.ui.language, "Guides"), |ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        if icon_toggle(ui, "guides-smart", smart, crate::i18n::tr(&app.ui.language, "Smart Guides")).clicked() {
            smart = !smart;
        }
        if icon_toggle(ui, "guides-lock", locked, crate::i18n::tr(&app.ui.language, "Lock Guides")).clicked() {
            locked = !locked;
        }
        if icon_toggle(ui, "guides-show", guides, crate::i18n::tr(&app.ui.language, "Show Guides")).clicked() {
            guides = !guides;
        }
    });
    if guides != app.ui.guides {
        let _ = app.run("view.guides", json!({}));
    }
    if smart != app.ui.smart_guides {
        let _ = app.run("view.smartGuides", json!({}));
    }
    app.ui.guides_locked = locked;
    rule(ui);
    section(ui, crate::i18n::tr(&app.ui.language, "Quick Actions"), true);
    let (a, b) = button_pair(ui, crate::i18n::tr(&app.ui.language, "Import File"), crate::i18n::tr(&app.ui.language, "Insert Page"));
    if a {
        let _ = app.run("app.placeDialog", json!({}));
    }
    if b {
        let _ = app.run("layout.pages.insert", json!({"after": cur, "count": 1}));
    }
}

// ---------------------------------------------------------------- Objects

/// The 9-point reference-point proxy; returns a newly picked point (0..8, row-major).
fn ref_point_proxy(ui: &mut Ui, r: Rect, cur: u8) -> Option<u8> {
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    let pitch = r.width() / 3.0;
    for i in 0..9u8 {
        let c = pos2(r.min.x + pitch * (i % 3) as f32 + pitch / 2.0, r.min.y + pitch * (i / 3) as f32 + pitch / 2.0);
        let sq = Rect::from_center_size(c, vec2(6.0, 6.0));
        let resp = ui.interact(sq.expand(1.5), ui.id().with(("refpt", i)), Sense::click());
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

fn transform_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
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
    if more_options(ui, &app.ui.language).clicked() {
        app.ui.open_panel = Some("info".into());
    }
}

/// A stroke-type preview dropdown (light box with the line style).
fn stroke_type_dropdown(app: &mut DesignApp, ui: &mut Ui, cur: &str, w: f32) {
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
fn preview_dropdown(ui: &mut Ui, w: f32, paint: impl FnOnce(&egui::Painter, Rect)) -> egui::Response {
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

fn appearance_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    let t = Tokens::get(ui.ctx());
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    // Fill.
    let b = block(ui, FIELD_H);
    let (fc, fg) = widgets::swatch_colors(&doc, &i.fill, 1.0);
    let chip = sub(b, 4.0, 2.0, 17.0, 17.0);
    widgets::paint_chip(ui.painter(), chip, fc, fg);
    ui.painter().rect_stroke(chip, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    let resp = ui.interact(chip, ui.id().with("fillchip"), Sense::click()).on_hover_text(format!("Fill: {}", i.fill));
    swatch_menu(app, &resp, &doc, &i.fill, |app, n| {
        let _ = app.run("object.fill", json!({"swatch": n}));
    });
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
    let resp = ui.interact(chip, ui.id().with("strokechip"), Sense::click()).on_hover_text(format!("Stroke: {}", i.stroke));
    swatch_menu(app, &resp, &doc, &i.stroke, |app, n| {
        let _ = app.run("object.stroke", json!({"swatch": n}));
    });
    if place(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), |ui| link_label(ui, crate::i18n::tr(&app.ui.language, "Stroke"), 56.0)).clicked() {
        app.ui.open_panel = Some("stroke".into());
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
        let resp = preview_dropdown(ui, 54.0, |p, r| paint_corner_shape(p, r, shape));
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(140.0);
            for s in CornerShape::ALL {
                if ui.selectable_label(s == shape, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, s.label()))).clicked() {
                    let _ = app.run("object.cornerOptions", json!({"shape": s, "size": if size > 0.0 { size } else { 12.0 }}));
                    ui.close();
                }
            }
        });
    });
    ui.add_space(4.0);
    // Opacity.
    let b = block(ui, FIELD_H);
    icons::paint(ui.painter(), sub(b, 3.0, 1.0, 19.0, 19.0), "opacity", t.icon);
    if place(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), |ui| link_label(ui, crate::i18n::tr(&app.ui.language, "Opacity"), 56.0)).clicked() {
        app.ui.open_panel = Some("effects".into());
    }
    if let Some(v) = place(ui, sub(b, 96.0, 0.0, 51.0, FIELD_H), |ui| {
        NumField::number("opacity", Some((i.opacity * 100.0).round()), "%", 0).width(51.0).range(0.0, 100.0).show(ui)
    }) {
        let _ = app.run("object.opacity", json!({"opacity": v / 100.0}));
    }
    let more = sub(b, 147.0, 0.0, 18.0, FIELD_H);
    let mresp = ui.interact(more, ui.id().with("opmore"), Sense::click());
    ui.painter().rect(more, 0.0, if mresp.hovered() { t.hover } else { t.input }, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    icons::paint(ui.painter(), more.shrink(4.0), "chevron-right", t.icon);
    if mresp.clicked() {
        app.ui.open_panel = Some("effects".into());
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if icon_toggle(ui, "frame-dashed", false, crate::i18n::tr(&app.ui.language, "Frame Fitting")).clicked() {
            app.ui.open_panel = Some("textWrap".into());
        }
        if icon_toggle(ui, "fx", i.shadow, crate::i18n::tr(&app.ui.language, "Effects: Drop Shadow")).clicked() {
            let _ = app.run("object.dropShadow", json!({"on": !i.shadow}));
        }
    });
}

/// Swatch list popup attached to `resp`.
fn swatch_menu(app: &mut DesignApp, resp: &egui::Response, doc: &designcraft_doc::Document, cur: &str, on_pick: impl FnOnce(&mut DesignApp, String)) {
    let mut picked = None;
    egui::Popup::menu(resp).show(|ui| {
        ui.set_min_width(200.0);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for sw in &doc.swatches {
                let (c, g) = widgets::swatch_colors(doc, &sw.name, 1.0);
                let row = ui.horizontal(|ui| {
                    let (cr, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    widgets::paint_chip(ui.painter(), cr, c, g);
                    ui.add(egui::Button::new(&sw.name).frame(false).selected(sw.name == cur))
                });
                if row.inner.clicked() {
                    picked = Some(sw.name.clone());
                    ui.close();
                }
            }
        });
    });
    if let Some(p) = picked {
        on_pick(app, p);
    }
}

fn align_row(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let to = app.ui.align_to.clone();
        let resp = icon_toggle(ui, "align-to", false, &format!("Align To: {to}"));
        egui::Popup::menu(&resp).show(|ui| {
            for (label, id) in [
                ("Align to Selection", "selection"),
                ("Align to Key Object", "keyObject"),
                ("Align to Margins", "margins"),
                ("Align to Page", "page"),
                ("Align to Spread", "spread"),
            ] {
                if ui.selectable_label(app.ui.align_to == id, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
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

fn wrap_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
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
        app.ui.open_panel = Some("textWrap".into());
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
                    app.status(format!("Convert Shape: {e}"));
                }
                ui.close();
            }
        }
    });
}

fn quick_actions_object(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
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

// ---------------------------------------------------------------- Text

fn text_style_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    section(ui, crate::i18n::tr(&app.ui.language, "Text Style"), true);
    let Some(a) = text_attrs(app) else { return };
    let tab = app.ui.text_style_tab;
    let fw = full_width(ui);
    if let Some(k) = widgets::segmented(
        ui,
        &[crate::i18n::tr(&app.ui.language, "Paragraph Styles"), crate::i18n::tr(&app.ui.language, "Character Styles")],
        tab as usize,
        fw,
    ) {
        app.ui.text_style_tab = k as u8;
    }
    ui.add_space(5.0);
    let (names, cur): (Vec<String>, String) = {
        let Some(st) = app.session.active() else { return };
        if tab == 0 {
            (
                st.doc.styles.paragraph.iter().map(|p| p.name.clone()).filter(|n| n != designcraft_doc::NO_PARA_STYLE).collect(),
                a["paragraphStyle"].as_str().unwrap_or("").to_string(),
            )
        } else {
            (st.doc.styles.character.iter().map(|p| p.name.clone()).collect(), a["characterStyle"].as_str().unwrap_or("[None]").to_string())
        }
    };
    let ov = if tab == 0 { a["paraOverrides"].as_u64().unwrap_or(0) + a["charOverrides"].as_u64().unwrap_or(0) } else { 0 };
    let shown = crate::i18n::style_name(&app.ui.language, &cur);
    let label = if ov > 0 { format!("{shown}+") } else { shown.to_string() };
    let resp = widgets::dropdown_h(ui, "", fw, 34.0);
    // "Ag" swatch + style name.
    let ag = Rect::from_min_size(resp.rect.min + vec2(6.0, 6.0), vec2(22.0, 22.0));
    ui.painter().rect_filled(ag, 1.0, egui::Color32::WHITE);
    ui.painter().text(ag.center(), egui::Align2::CENTER_CENTER, "Ag", egui::FontId::new(13.0, egui::FontFamily::Proportional), egui::Color32::BLACK);
    crate::rtl::paint(
        &ui.painter().with_clip_rect(resp.rect.shrink2(vec2(20.0, 0.0))),
        pos2(ag.max.x + 8.0, resp.rect.center().y),
        egui::Align2::LEFT_CENTER,
        &label,
        egui::FontId::proportional(11.5),
        t.text,
    );
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(fw);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for n in &names {
                if ui.selectable_label(*n == cur, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                    pick = Some(n.clone());
                    ui.close();
                }
            }
        });
    });
    if let Some(n) = pick {
        if tab == 0 {
            let _ = app.run("style.paragraph.apply", json!({"name": n, "clearOverrides": false}));
        } else {
            let _ = app.run("style.character.apply", json!({"name": n}));
        }
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if icon_toggle(
            ui,
            "pilcrow-menu",
            false,
            crate::i18n::tr(&app.ui.language, if tab == 0 { "Paragraph Styles panel" } else { "Character Styles panel" }),
        )
        .clicked()
        {
            app.ui.open_panel = Some(if tab == 0 { "paragraphStyles" } else { "characterStyles" }.into());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.add_space(4.0);
            if icon_toggle(ui, "style-new", false, crate::i18n::tr(&app.ui.language, crate::i18n::tr(&app.ui.language, "Create new style"))).clicked()
            {
                let cmd = if tab == 0 { "style.paragraph.create" } else { "style.character.create" };
                let _ = app.run(cmd, json!({}));
            }
            if icon_toggle(ui, "style-clear", false, crate::i18n::tr(&app.ui.language, crate::i18n::tr(&app.ui.language, "Clear overrides")))
                .clicked()
                && tab == 0
                && !cur.is_empty()
            {
                let _ = app.run("style.paragraph.apply", json!({"name": cur, "clearOverrides": true}));
            }
            if icon_toggle(ui, "style-load", false, crate::i18n::tr(&app.ui.language, "Styles panel")).clicked() {
                app.ui.open_panel = Some(if tab == 0 { "paragraphStyles" } else { "characterStyles" }.into());
            }
        });
    });
}

fn text_appearance_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(a) = text_attrs(app) else { return };
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let c = &a["chars"];
    for (k, (key, label)) in [("fill", "Fill"), ("stroke", "Stroke")].into_iter().enumerate() {
        let sw = c[key].as_str().unwrap_or(designcraft_color::swatch::NONE).to_string();
        let b = block(ui, FIELD_H);
        let (col, g) = widgets::swatch_colors(&doc, &sw, 1.0);
        let chip = sub(b, 4.0, 2.0, 17.0, 17.0);
        if k == 0 {
            widgets::paint_chip(ui.painter(), chip, col, g);
        } else {
            widgets::paint_stroke_chip(ui.painter(), chip, col, egui::Color32::from_gray(20));
        }
        ui.painter().rect_stroke(chip, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
        // The "T" marks text formatting.
        let tc = if col.is_some_and(|c| c.r() as u32 + c.g() as u32 + c.b() as u32 > 380) { egui::Color32::BLACK } else { egui::Color32::WHITE };
        if k == 0 {
            ui.painter().text(chip.center(), egui::Align2::CENTER_CENTER, "T", crate::theme::semibold(12.0), tc);
        }
        let resp = ui.interact(chip, ui.id().with(("tchip", k)), Sense::click()).on_hover_text(format!("{label}: {sw}"));
        swatch_menu(app, &resp, &doc, &sw, |app, n| {
            let _ = app.run("type.char", json!({"attrs": {key: n}}));
        });
        if k == 0 {
            crate::rtl::paint(
                ui.painter(),
                b.min + vec2(34.0, FIELD_H / 2.0),
                egui::Align2::LEFT_CENTER,
                crate::i18n::tr(&app.ui.language, label),
                egui::FontId::proportional(11.5),
                t.text,
            );
        } else {
            if place(ui, sub(b, 34.0, 0.0, 56.0, FIELD_H), |ui| link_label(ui, crate::i18n::tr(&app.ui.language, label), 56.0)).clicked() {
                app.ui.open_panel = Some("stroke".into());
            }
            const WEIGHTS: &[f64] = &[0.0, 0.25, 0.5, 0.75, 1.0, 2.0, 3.0, 4.0, 5.0, 10.0];
            if let Some(v) = place(ui, sub(b, 96.0, 0.0, 69.0, FIELD_H), |ui| {
                NumField::number("tsw", c["strokeWeight"].as_f64(), " pt", 3).width(69.0).spinner().presets(WEIGHTS).range(0.0, 100.0).show(ui)
            }) {
                let _ = app.run("type.char", json!({"attrs": {"strokeWeight": v}}));
            }
        }
        ui.add_space(4.0);
    }
}

/// Character section: font family (with search), style, size/leading, kerning/tracking, •••.
fn character_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(a) = text_attrs(app) else { return };
    let c = a["chars"].clone();
    let fam = c["fontFamily"].as_str().unwrap_or("").to_string();
    let sty = c["fontStyle"].as_str().unwrap_or("").to_string();
    let fw = full_width(ui);
    // Family dropdown with a search segment on the left.
    let fams = designcraft_fonts::FontDb::global().families();
    let resp = widgets::dropdown(ui, "", fw);
    let r = resp.rect;
    icons::paint(ui.painter(), Rect::from_min_size(r.min + vec2(3.0, 3.0), vec2(14.0, 14.0)), "search", t.icon);
    widgets::chevron_down(ui.painter(), r.min + vec2(22.0, 10.5), t.icon);
    ui.painter().text(
        r.min + vec2(30.0, 10.5),
        egui::Align2::LEFT_CENTER,
        if fam.is_empty() { "—" } else { &fam },
        egui::FontId::proportional(11.5),
        t.text,
    );
    let mut pick_fam = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(fw);
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for f in &fams {
                if ui.selectable_label(*f == fam, f).clicked() {
                    pick_fam = Some(f.clone());
                    ui.close();
                }
            }
        });
    });
    if let Some(f) = pick_fam {
        let styles = designcraft_fonts::FontDb::global().styles(&f);
        let style = if styles.iter().any(|s| s == "Regular") { "Regular".to_string() } else { styles.first().cloned().unwrap_or_default() };
        let _ = app.run("type.char", json!({"attrs": {"fontFamily": f, "fontStyle": style}}));
    }
    ui.add_space(1.0);
    let styles = designcraft_fonts::FontDb::global().styles(&fam);
    let cur = styles.iter().position(|s| *s == sty);
    if let Some(k) = widgets::dropdown_list(ui, if sty.is_empty() { "—" } else { &sty }, fw, &styles, cur) {
        let _ = app.run("type.char", json!({"attrs": {"fontStyle": styles[k]}}));
    }
    ui.add_space(1.0);
    const SIZES: &[f64] = &[6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 21.0, 24.0, 30.0, 36.0, 48.0, 60.0, 72.0];
    let half = (fw - 10.0) / 2.0;
    let b = block(ui, 2.0 * FIELD_H + 5.0);
    let size = c["size"].as_f64();
    let auto = c["leading"]["kind"].as_str() != Some("points");
    let lv = if auto { size.map(|s| s * a["para"]["autoLeading"].as_f64().unwrap_or(1.2)) } else { c["leading"]["value"].as_f64() };
    icons::paint(ui.painter(), sub(b, 0.0, 1.0, 19.0, 19.0), "font-size", t.icon);
    if let Some(v) = place(ui, sub(b, 24.0, 0.0, half - 24.0, FIELD_H), |ui| {
        NumField::number("csize", size, " pt", 2).width(half - 24.0).spinner().presets(SIZES).range(0.1, 1296.0).show(ui)
    }) {
        let _ = app.run("type.char", json!({"attrs": {"size": v}}));
    }
    icons::paint(ui.painter(), sub(b, half + 10.0, 1.0, 19.0, 19.0), "leading", t.icon);
    let lw = half - 24.0;
    let lshow = lv.map(|v| (v * 100.0).round() / 100.0);
    if let Some(v) = place(ui, sub(b, half + 34.0, 0.0, lw, FIELD_H), |ui| {
        let key = if auto { "cleadauto" } else { "clead" };
        // Auto leading is shown in parentheses, like "(14.4 pt)".
        NumField::number(key, lshow, " pt", 2).width(lw).spinner().presets(SIZES).range(0.0, 5000.0).parens(auto).show(ui)
    }) {
        let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
    }
    // Kerning / tracking.
    icons::paint(ui.painter(), sub(b, 0.0, 27.0, 19.0, 19.0), "kerning", t.icon);
    let kern = c["kerning"]["kind"].as_str().unwrap_or("metrics").to_string();
    let kern_label = match kern.as_str() {
        "optical" => "Optical".to_string(),
        "none" => "0".to_string(),
        "manual" => format!("{}", c["kerning"]["value"].as_f64().unwrap_or(0.0)),
        _ => "Metrics".to_string(),
    };
    place(ui, sub(b, 24.0, 26.0, half - 24.0, FIELD_H), |ui| {
        let resp = widgets::dropdown(ui, crate::i18n::tr(&app.ui.language, &kern_label), half - 24.0);
        egui::Popup::menu(&resp).show(|ui| {
            for (label, v) in [("Metrics", json!({"kind": "metrics"})), ("Optical", json!({"kind": "optical"})), ("0", json!({"kind": "none"}))] {
                if ui.selectable_label(kern_label == label, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    let _ = app.run("type.char", json!({"attrs": {"kerning": v}}));
                    ui.close();
                }
            }
        });
    });
    icons::paint(ui.painter(), sub(b, half + 10.0, 27.0, 19.0, 19.0), "tracking", t.icon);
    const TRACK: &[f64] = &[-100.0, -75.0, -50.0, -25.0, -10.0, -5.0, 0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 200.0];
    if let Some(v) = place(ui, sub(b, half + 34.0, 26.0, lw, FIELD_H), |ui| {
        NumField::number("ctrack", c["tracking"].as_f64(), "", 0).width(lw).spinner().presets(TRACK).step(5.0).range(-1000.0, 10000.0).show(ui)
    }) {
        let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
    }
    if more_options(ui, &app.ui.language).clicked() {
        app.ui.open_panel = Some("character".into());
    }
}

/// Paragraph section: the 9 alignment buttons and •••.
fn paragraph_section(app: &mut DesignApp, ui: &mut Ui) {
    let Some(a) = text_attrs(app) else { return };
    let cur: Align = serde_json::from_value(a["para"]["align"].clone()).unwrap_or_default();
    let fw = full_width(ui);
    let pitch = fw / 9.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (al, icon) in [
            (Align::Left, "palign-left"),
            (Align::Center, "palign-center"),
            (Align::Right, "palign-right"),
            (Align::LeftJustified, "palign-justify-left"),
            (Align::CenterJustified, "palign-justify-center"),
            (Align::RightJustified, "palign-justify-right"),
            (Align::FullyJustified, "palign-justify-all"),
            (Align::TowardsSpine, "palign-spine-towards"),
            (Align::AwayFromSpine, "palign-spine-away"),
        ] {
            if widgets::icon_toggle_sized(ui, icon, cur == al, al.label(), vec2(pitch.min(23.0), 21.0)).clicked() {
                let _ = app.run("type.para", json!({"attrs": {"align": al}}));
            }
            if pitch > 23.0 {
                ui.add_space(pitch - 23.0);
            }
        }
    });
    if more_options(ui, &app.ui.language).clicked() {
        app.ui.open_panel = Some("paragraph".into());
    }
}

fn bullets_section(app: &mut DesignApp, ui: &mut Ui) {
    let Some(a) = text_attrs(app) else { return };
    let lt = a["para"]["listType"].as_str().unwrap_or("none").to_string();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for (icon, kind, tip) in [("bullets", "bullets", "Bulleted List"), ("numbering", "numbers", "Numbered List")] {
            let on = lt == kind;
            if icon_toggle(ui, icon, on, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("type.para", json!({"attrs": {"listType": if on { "none" } else { kind }}}));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            if outline_button(ui, crate::i18n::tr(&app.ui.language, "Options"), 66.0).clicked() {
                app.ui.open_panel = Some("paragraph".into());
            }
        });
    });
}

fn quick_actions_text(app: &mut DesignApp, ui: &mut Ui) {
    let fw = full_width(ui);
    let hw = (fw - 10.0) / 2.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let r = outline_button(ui, crate::i18n::tr(&app.ui.language, "Change Case"), hw);
        egui::Popup::menu(&r).show(|ui| {
            for (label, v) in [("UPPERCASE", "allCaps"), ("Small Caps", "smallCaps"), ("Normal", "normal")] {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    let _ = app.run("type.char", json!({"attrs": {"capitalization": v}}));
                    ui.close();
                }
            }
        });
        if outline_button(ui, crate::i18n::tr(&app.ui.language, "Story Editor"), hw).clicked() {
            let _ = app.run("app.storyEditor", json!({}));
        }
    });
    ui.add_space(4.0);
    if outline_button(ui, crate::i18n::tr(&app.ui.language, "Fill with Placeholder Text"), fw).clicked() {
        let _ = app.run("type.fillWithPlaceholder", json!({}));
    }
}

/// Text Frame section: columns, gutter, [Options].
fn text_frame_section(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let u = units(app);
    let Some(st) = app.session.active() else { return };
    let fid = st.selection.items.first().copied().or_else(|| st.selection.text.and_then(|t| t.frame));
    let Some(opts) = fid.and_then(|f| st.doc.item(f)).and_then(|i| i.text_frame()).map(|t| t.options.clone()) else { return };
    let b = block(ui, FIELD_H);
    icons::paint(ui.painter(), sub(b, 0.0, 1.0, 19.0, 19.0), "text-columns", t.icon);
    if let Some(v) = place(ui, sub(b, 22.0, 0.0, 54.0, FIELD_H), |ui| {
        NumField::number("tfcols", Some(opts.columns as f64), "", 0).width(54.0).spinner().range(1.0, 40.0).show(ui)
    }) {
        let _ = app.run("object.textFrameOptions", json!({"columns": v.max(1.0) as u64}));
    }
    icons::paint(ui.painter(), sub(b, 83.0, 1.0, 19.0, 19.0), "text-gutter", t.icon);
    if let Some(v) = place(ui, sub(b, 105.0, 0.0, 54.0, FIELD_H), |ui| {
        NumField::measure("tfgut", Some(opts.gutter), u).width(54.0).spinner().range(0.0, 1440.0).show(ui)
    }) {
        let _ = app.run("object.textFrameOptions", json!({"gutter": v}));
    }
    let ow = (b.max.x - 8.0) - (b.min.x + 164.0);
    if place(ui, sub(b, 164.0, 0.5, ow, 20.0), |ui| outline_button(ui, crate::i18n::tr(&app.ui.language, "Options"), ow)).clicked() {
        app.ui.dialog = Some(crate::dialogs::Dialog::new("textFrameOptions", json!({})));
    }
}

/// Languages offered for text (InDesign-style names).
const LANGUAGES: &[&str] = &[
    "[No Language]",
    "English: USA",
    "English: UK",
    "English: Canadian",
    "German: 2006 Reform",
    "German: Swiss 2006 Reform",
    "French",
    "French: Canadian",
    "Spanish",
    "Italian",
    "Portuguese",
    "Portuguese: Brazilian",
    "Dutch: 2005 Reform",
    "Danish",
    "Swedish",
    "Norwegian: Bokmål",
    "Finnish",
    "Polish",
    "Czech",
    "Hungarian",
    "Greek",
    "Russian",
    "Ukrainian",
    "Turkish",
    "Japanese",
    "Chinese",
    "Korean",
    "Arabic",
    "Hebrew",
];

pub fn character_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(a) = text_attrs(app) else {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Select text or a text frame."));
        return;
    };
    let c = a["chars"].clone();
    let fam = c["fontFamily"].as_str().unwrap_or("").to_string();
    let sty = c["fontStyle"].as_str().unwrap_or("").to_string();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Character")).strong());
    super::font_family_picker(app, ui, &fam, 220.0);
    super::font_style_picker(app, ui, &fam, &sty, 220.0);
    variable_font_axes(app, ui, &fam, &sty);
    // Language: hyphenation, spelling and typographer's quotes follow it.
    let lang = c["language"].as_str().unwrap_or("English: USA").to_string();
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Language"));
        egui::ComboBox::from_id_salt("char_language")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &lang)))
            .width(170.0)
            .show_ui(ui, |ui| {
                for l in LANGUAGES {
                    if ui.selectable_label(*l == lang, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        let _ = app.run("type.char", json!({"attrs": {"language": l}}));
                    }
                }
            });
    });
    // Digits (World-Ready): shown for right-to-left paragraphs or once set.
    let digits = c["digits"].as_str().unwrap_or("default").to_string();
    if digits != "default" || a["para"]["direction"].as_str() == Some("rightToLeft") {
        ui.horizontal(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Digits"));
            let opts = [("default", "Default"), ("arabic", "Arabic"), ("hindi", "Hindi"), ("farsi", "Farsi"), ("native", "Native")];
            let cur = opts.iter().find(|o| o.0 == digits).map_or("Default", |o| o.1);
            egui::ComboBox::from_id_salt("char_digits")
                .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, cur)))
                .width(120.0)
                .show_ui(ui, |ui| {
                    for (k, l) in opts {
                        if ui.selectable_label(k == digits, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                            let _ = app.run("type.char", json!({"attrs": {"digits": k}}));
                        }
                    }
                });
        });
    }
    egui::Grid::new("chargrid").num_columns(4).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Size"));
        if let Some(v) = number(ui, "pcs", c["size"].as_f64(), " pt", 60.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"size": v}}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "Leading"));
        let auto = c["leading"]["kind"].as_str() != Some("points");
        let lv = if auto { c["size"].as_f64().map(|s| s * a["para"]["autoLeading"].as_f64().unwrap_or(1.2)) } else { c["leading"]["value"].as_f64() };
        if let Some(v) = number(ui, "pcl", lv, if auto { crate::i18n::tr(&app.ui.language, " (auto)") } else { " pt" }, 70.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
        }
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Tracking"));
        if let Some(v) = number(ui, "pct", c["tracking"].as_f64(), "", 60.0, 0) {
            let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "Baseline"));
        if let Some(v) = number(ui, "pcb", c["baselineShift"].as_f64(), " pt", 70.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"baselineShift": v}}));
        }
        ui.end_row();
    });
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Color"));
        super::swatch_picker(app, ui, "tfill", c["fill"].as_str().map(str::to_string), |app, n| {
            let _ = app.run("type.char", json!({"attrs": {"fill": n}}));
        });
        crate::rtl::label(ui, c["fill"].as_str().unwrap_or(""));
    });
    ui.horizontal(|ui| {
        let caps = c["capitalization"].as_str() == Some("allCaps");
        if ui
            .selectable_label(caps, "TT")
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "All Caps"));
            })
            .clicked()
        {
            let _ = app.run("type.char", json!({"attrs": {"capitalization": if caps { "normal" } else { "allCaps" }}}));
        }
        let ul = c["underline"].as_bool().unwrap_or(false);
        if ui
            .selectable_label(ul, "U")
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Underline"));
            })
            .clicked()
        {
            let _ = app.run("type.char", json!({"attrs": {"underline": !ul}}));
        }
        let st = c["strikethrough"].as_bool().unwrap_or(false);
        if ui
            .selectable_label(st, "S")
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Strikethrough"));
            })
            .clicked()
        {
            let _ = app.run("type.char", json!({"attrs": {"strikethrough": !st}}));
        }
        let sup = c["position"].as_str() == Some("superscript");
        if ui
            .selectable_label(sup, "T¹")
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Superscript"));
            })
            .clicked()
        {
            let _ = app.run("type.char", json!({"attrs": {"position": if sup { "normal" } else { "superscript" }}}));
        }
        let features: Vec<String> =
            c["otfFeatures"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        ui.menu_button("OpenType", |ui| open_type_menu(app, ui, &features));
    });
    // Underline / Strikethrough Options.
    for (key, title) in [("underline", "Underline Options"), ("strikethrough", "Strikethrough Options")] {
        if !c[key].as_bool().unwrap_or(false) {
            continue;
        }
        egui::CollapsingHeader::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, title))).id_salt(key).show(ui, |ui| {
            let size = c["size"].as_f64().unwrap_or(12.0);
            let auto_off = if key == "underline" { size * 0.12 } else { size * 0.3 };
            egui::Grid::new((key, "opts")).num_columns(4).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
                caption(ui, crate::i18n::tr(&app.ui.language, "Weight"));
                let w = c[format!("{key}Weight")].as_f64().unwrap_or(size / 14.0);
                if let Some(v) = number(ui, &format!("{key}w"), Some(w), " pt", 60.0, 2) {
                    let _ = app.run("type.char", json!({"attrs": {format!("{key}Weight"): v.max(0.0)}}));
                }
                caption(ui, crate::i18n::tr(&app.ui.language, "Offset"));
                let o = c[format!("{key}Offset")].as_f64().unwrap_or(auto_off);
                if let Some(v) = number(ui, &format!("{key}o"), Some(o), " pt", 60.0, 2) {
                    let _ = app.run("type.char", json!({"attrs": {format!("{key}Offset"): v}}));
                }
                ui.end_row();
            });
            ui.horizontal(|ui| {
                caption(ui, crate::i18n::tr(&app.ui.language, "Color"));
                let cur = c[format!("{key}Color")].as_str().filter(|s| !s.is_empty()).map(str::to_string);
                super::swatch_picker(app, ui, &format!("{key}c"), cur.clone(), |app, n| {
                    let _ = app.run("type.char", json!({"attrs": {format!("{key}Color"): n}}));
                });
                crate::rtl::label(ui, cur.unwrap_or_else(|| crate::i18n::tr(&app.ui.language, "(Text Color)").into()));
            });
        });
    }
}

/// The Character panel's OpenType menu: feature toggles, figure styles and stylistic sets.
pub fn open_type_menu(app: &mut DesignApp, ui: &mut egui::Ui, features: &[String]) {
    use designcraft_doc::otf;
    ui.set_min_width(220.0);
    for (tag, label, _) in otf::TOGGLES {
        let mut on = otf::is_on(features, tag);
        if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
            let _ = app.run("type.openType", json!({"feature": tag, "on": on}));
            ui.close();
        }
    }
    ui.separator();
    let fig = otf::figures(features);
    for (id, label, ..) in otf::FIGURES {
        if ui.radio(fig == *id, *label).clicked() {
            let _ = app.run("type.openType", json!({"figures": id}));
            ui.close();
        }
    }
    ui.separator();
    ui.menu_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Stylistic Sets")), |ui| {
        let mask = otf::stylistic_sets(features);
        for n in 1..=20u32 {
            let mut on = mask & (1 << (n - 1)) != 0;
            if ui.checkbox(&mut on, crate::rtl::widget(ui, format!("{} {n}", crate::i18n::tr(&app.ui.language, "Set")))).clicked() {
                let sets: Vec<u32> = (1..=20).filter(|k| if *k == n { on } else { mask & (1 << (k - 1)) != 0 }).collect();
                let _ = app.run("type.openType", json!({"stylisticSets": sets}));
            }
        }
    });
}

pub fn paragraph_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(a) = text_attrs(app) else {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Select text or a text frame."));
        return;
    };
    let u = units(app);
    let p = a["para"].clone();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Paragraph")).strong());
    let cur: Align = serde_json::from_value(p["align"].clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        for (al, icon) in [
            (Align::Left, "align-left"),
            (Align::Center, "align-center"),
            (Align::Right, "align-right"),
            (Align::LeftJustified, "align-justify"),
            (Align::FullyJustified, "align-justify-all"),
        ] {
            if icons::button(ui, icon, 24.0, cur == al, al.label()).clicked() {
                let _ = app.run("type.para", json!({"attrs": {"align": al}}));
            }
        }
    });
    // Paragraph Direction (World-Ready): switching also mirrors left/right alignment.
    let rtl = p["direction"] == "rightToLeft";
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Direction"));
        for (on, label, dir) in [(!rtl, "Left to Right", "leftToRight"), (rtl, "Right to Left", "rightToLeft")] {
            if ui.selectable_label(on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() && !on {
                let align = match cur {
                    Align::Left => Align::Right,
                    Align::Right => Align::Left,
                    a => a,
                };
                let _ = app.run("type.para", json!({"attrs": {"direction": dir, "align": align}}));
            }
        }
    });
    egui::Grid::new("paragrid").num_columns(4).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
        for (row, ((la, ka), (lb, kb))) in [
            (("Left", "leftIndent"), ("Right", "rightIndent")),
            (("First", "firstLineIndent"), ("Last", "lastLineIndent")),
            (("Before", "spaceBefore"), ("After", "spaceAfter")),
        ]
        .into_iter()
        .enumerate()
        {
            caption(ui, crate::i18n::tr(&app.ui.language, la));
            if let Some(v) = measure(ui, &format!("pp{row}a"), p[ka].as_f64(), u, 64.0) {
                let _ = app.run("type.para", json!({"attrs": {ka: v}}));
            }
            caption(ui, crate::i18n::tr(&app.ui.language, lb));
            if let Some(v) = measure(ui, &format!("pp{row}b"), p[kb].as_f64(), u, 64.0) {
                let _ = app.run("type.para", json!({"attrs": {kb: v}}));
            }
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        let mut h = p["hyphenate"].as_bool().unwrap_or(true);
        if ui.checkbox(&mut h, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Hyphenate"))).changed() {
            let _ = app.run("type.para", json!({"attrs": {"hyphenate": h}}));
        }
        let mut grid = p["gridAlign"].as_str() == Some("allLines");
        if ui.checkbox(&mut grid, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Align to baseline grid"))).changed() {
            let _ = app.run("type.para", json!({"attrs": {"gridAlign": if grid { "allLines" } else { "none" }}}));
        }
    });
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Composer"));
        let single = p["composer"].as_str() == Some("singleLine");
        if ui.selectable_label(!single, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Paragraph"))).clicked() {
            let _ = app.run("type.para", json!({"attrs": {"composer": "paragraph"}}));
        }
        if ui.selectable_label(single, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Single-line"))).clicked() {
            let _ = app.run("type.para", json!({"attrs": {"composer": "singleLine"}}));
        }
    });
    // Paragraph Border and Shading.
    let swatches: Vec<String> =
        app.session.active().map(|d| d.doc.swatches.iter().filter(|w| !w.hidden).map(|w| w.name.clone()).collect()).unwrap_or_default();
    for (on_key, color_key, label) in [("shadingOn", "shadingColor", "Shading"), ("borderOn", "borderColor", "Border")] {
        ui.horizontal(|ui| {
            let mut on = p[on_key].as_bool().unwrap_or(false);
            if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).changed() {
                let _ = app.run("type.para", json!({ on_key: on }));
            }
            if on {
                let cur = p[color_key].as_str().unwrap_or("[Black]").to_string();
                egui::ComboBox::from_id_salt(("pbs", label)).selected_text(&cur).width(110.0).show_ui(ui, |ui| {
                    for w in &swatches {
                        if ui.selectable_label(*w == cur, w).clicked() {
                            let _ = app.run("type.para", json!({ color_key: w }));
                        }
                    }
                });
                if label == "Shading" {
                    if let Some(v) = number(ui, "pshtint", p["shadingTint"].as_f64().map(|t| t * 100.0), "%", 46.0, 0) {
                        let _ = app.run("type.para", json!({"shadingTint": (v / 100.0).clamp(0.0, 1.0)}));
                    }
                } else if let Some(v) = number(ui, "pbw", p["borderWeights"][0].as_f64(), " pt", 50.0, 2) {
                    let w = [v.max(0.0); 4];
                    let _ = app.run("type.para", json!({ "borderWeights": w }));
                }
            }
        });
    }
    if p["shadingOn"].as_bool().unwrap_or(false) || p["borderOn"].as_bool().unwrap_or(false) {
        ui.horizontal(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Offsets"));
            let o = p["borderOffsets"][0].as_f64().or(p["shadingOffsets"][0].as_f64());
            if let Some(v) = number(ui, "pbo", o, " pt", 50.0, 2) {
                let o = [v; 4];
                let _ = app.run("type.para", json!({ "borderOffsets": o, "shadingOffsets": o }));
            }
        });
    }
    let ps = a["paragraphStyle"].as_str().unwrap_or("").to_string();
    let ov = a["paraOverrides"].as_u64().unwrap_or(0) + a["charOverrides"].as_u64().unwrap_or(0);
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Style"));
        super::para_style_picker(app, ui, &ps, ov > 0, 190.0);
    });
}

/// Stroke panel: weight, cap, miter limit, join, alignment, type, arrowheads, gap colour.
pub fn stroke_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
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
        egui::ComboBox::from_id_salt("stype")
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
        ui.end_row();
        for (key, label, cur, flip) in [("start", "Start/End:", st.start, true), ("end", "", st.end, false)] {
            caption(ui, crate::i18n::tr(&app.ui.language, label));
            egui::ComboBox::from_id_salt(key)
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
            ui.end_row();
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "Gap Color:"));
        super::swatch_picker(app, ui, "sgap", Some(st.gap_swatch.clone()), |app, n| {
            let _ = app.run("object.stroke", json!({"gapSwatch": n}));
        });
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Color:"));
        super::swatch_picker(app, ui, "spick", Some(i.stroke.clone()), |app, n| {
            let _ = app.run("object.stroke", json!({"swatch": n}));
        });
        ui.end_row();
    });
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
    ui.horizontal(|ui| {
        for (icon, mode, tip) in [
            ("wrap-none", "none", "No Text Wrap"),
            ("wrap-bbox", "boundingBox", "Wrap Around Bounding Box"),
            ("wrap-contour", "contour", "Wrap Around Object Shape"),
            ("wrap-jump", "jumpObject", "Jump Object"),
            ("wrap-next", "jumpToNextColumn", "Jump to Next Column"),
        ] {
            let on = info.as_ref().is_some_and(|i| i.wrap == mode);
            if icons::button(ui, icon, 26.0, on, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.textWrap", json!({"mode": mode}));
            }
        }
    });
    let u = units(app);
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Offset"));
        if let Some(v) = measure(ui, "wo", Some(0.0), u, 60.0) {
            let _ = app.run("object.textWrap", json!({"mode": info.as_ref().map(|i| i.wrap).unwrap_or("boundingBox"), "offset": v}));
        }
    });
}

/// Pathfinder panel: combine the selected shapes; make or release compound paths.
pub fn pathfinder_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Pathfinder")).strong());
    ui.horizontal_wrapped(|ui| {
        for (label, op) in
            [("Add", "add"), ("Subtract", "subtract"), ("Intersect", "intersect"), ("Exclude Overlap", "exclude"), ("Minus Back", "minusBack")]
        {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                let _ = app.run("object.pathfinder", json!({"op": op}));
            }
        }
    });
    ui.add_space(4.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Paths")).strong());
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Make Compound"))).clicked() {
            let _ = app.run("object.makeCompoundPath", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Release"))).clicked() {
            let _ = app.run("object.releaseCompoundPath", json!({}));
        }
    });
}

pub fn align_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        for (edge, tip) in [
            ("left", "Align left edges"),
            ("hcenter", "Align horizontal centers"),
            ("right", "Align right edges"),
            ("top", "Align top edges"),
            ("vcenter", "Align vertical centers"),
            ("bottom", "Align bottom edges"),
        ] {
            if ui
                .small_button(match edge {
                    "left" => "⇤",
                    "hcenter" => "↔",
                    "right" => "⇥",
                    "top" => "⤒",
                    "vcenter" => "↕",
                    _ => "⤓",
                })
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, tip));
                })
                .clicked()
            {
                let _ = app.run("object.align", json!({"edge": edge}));
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Distribute ↔"))).clicked() {
            let _ = app.run("object.distribute", json!({"axis": "horizontal"}));
        }
        if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Distribute ↕"))).clicked() {
            let _ = app.run("object.distribute", json!({"axis": "vertical"}));
        }
    });
}

/// Links: status (missing / modified / embedded), page and effective resolution of every placed
/// graphic; Relink, Go To Link, Update Link, Embed Link for the chosen row.
pub fn links_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    // Linked stories (Place and Link).
    let stories = app.session.execute("story.links", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    if !stories.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Linked Stories")).strong());
        for l in &stories {
            let out = l["outOfDate"].as_bool().unwrap_or(false);
            ui.horizontal(|ui| {
                crate::rtl::label(ui, format!("Story {} ← story {}", l["story"], l["parent"]));
                if out {
                    crate::rtl::label(
                        ui,
                        crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, "⚠ out of date")).color(egui::Color32::from_rgb(230, 160, 40)),
                        ),
                    );
                    if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Update"))).clicked() {
                        let _ = app.run("story.updateLink", json!({"story": l["story"]}));
                    }
                }
            });
        }
        ui.separator();
    }
    let Some(st) = app.session.active() else { return };
    let t = Tokens::get(ui.ctx());
    // The list checks files on disk: refresh it at most twice a second.
    let key = egui::Id::new("links_panel");
    let now = ui.input(|i| i.time);
    let rev = (st.uid, st.revision);
    let cached: Option<(f64, (u64, u64), Vec<Value>)> = ui.data(|d| d.get_temp(key));
    let rows = match cached {
        Some((at, r, rows)) if r == rev && now - at < 0.5 => rows,
        _ => {
            let rows = app.session.execute("links.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
            ui.data_mut(|d| d.insert_temp(key, (now, rev, rows.clone())));
            rows
        }
    };
    if rows.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No placed graphics.")).color(t.text_dim));
        return;
    }
    let sel_key = egui::Id::new("links_panel_sel");
    let mut chosen: Option<u64> = ui.data(|d| d.get_temp(sel_key));
    egui::Grid::new("links_grid").num_columns(3).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Name")).color(t.text_dim).size(11.0));
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Status")).color(t.text_dim).size(11.0));
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page · PPI")).color(t.text_dim).size(11.0));
        ui.end_row();
        for r in &rows {
            let aid = r["asset"].as_u64().unwrap_or(0);
            let name = r["name"].as_str().unwrap_or("");
            if ui.selectable_label(chosen == Some(aid), name).clicked() {
                chosen = Some(aid);
            }
            let (txt, col) = match r["status"].as_str().unwrap_or("") {
                "missing" => ("\u{26A0} Missing", egui::Color32::from_rgb(0xe5, 0x4b, 0x4b)),
                "modified" => ("\u{25B2} Modified", egui::Color32::from_rgb(0xe8, 0xb3, 0x2c)),
                "embedded" => ("Embedded", t.text_dim),
                _ => ("OK", t.text_dim),
            };
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, txt)).color(col).size(11.0));
            let uses = r["uses"].as_array().cloned().unwrap_or_default();
            let info = match uses.first() {
                Some(u) => {
                    let page = u["page"].as_str().unwrap_or("PB");
                    let ppi = u["ppi"].as_f64().map(|p| format!(" · {p:.0} ppi")).unwrap_or_default();
                    let more = if uses.len() > 1 { format!(" (+{})", uses.len() - 1) } else { String::new() };
                    format!("{page}{ppi}{more}")
                }
                None => crate::i18n::tr(&app.ui.language, "unused").into(),
            };
            // Low effective resolution for print is flagged like the Preflight check.
            let low = uses.first().and_then(|u| u["ppi"].as_f64()).is_some_and(|p| p < 150.0);
            crate::rtl::label(
                ui,
                egui::RichText::new(info).size(11.0).color(if low { egui::Color32::from_rgb(0xe8, 0xb3, 0x2c) } else { t.text_dim }),
            );
            ui.end_row();
        }
    });
    if let Some(c) = chosen {
        ui.data_mut(|d| d.insert_temp(sel_key, c));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let Some(aid) = chosen else {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Choose a link.")).color(t.text_dim).size(11.0));
            return;
        };
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Relink…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|pick| pick("relink"))
        {
            let _ = app.run("links.relink", json!({"asset": aid, "path": path}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Go To"))).clicked() {
            let _ = app.run("links.goTo", json!({"asset": aid}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Update"))).clicked() {
            let _ = app.run("links.update", json!({"asset": aid}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Embed"))).clicked() {
            let _ = app.run("links.embed", json!({"asset": aid}));
        }
    });
    ui.horizontal(|ui| {
        // Choose any file in the folder: missing links are looked up there by name.
        if ui
            .button(crate::i18n::tr(&app.ui.language, "Relink to Folder…"))
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Pick a file in the folder holding the moved graphics"));
            })
            .clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|pick| pick("relink"))
            && let Some(dir) = std::path::Path::new(&path).parent()
        {
            let r = app.run("links.relinkFolder", json!({"dir": dir.to_string_lossy()}));
            if let Ok(v) = r {
                app.status(format!("Relinked {}", v["relinked"]));
            }
        }
    });
}

pub fn info_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let u = units(app);
    let t = Tokens::get(ui.ctx());
    match sel_info(app) {
        Some(i) => {
            let f = |v: f64| designcraft_geom::format_measure(v, u);
            crate::rtl::label(ui, format!("X: {}   Y: {}", f(i.page_rect.x0), f(i.page_rect.y0)));
            crate::rtl::label(ui, format!("W: {}   H: {}", f(i.page_rect.width()), f(i.page_rect.height())));
            crate::rtl::label(
                ui,
                format!("{}: {}   {}: {}", crate::i18n::tr(&app.ui.language, "Fill"), i.fill, crate::i18n::tr(&app.ui.language, "Stroke"), i.stroke),
            );
        }
        None => {
            if let Some(st) = app.session.active() {
                crate::rtl::label(
                    ui,
                    format!(
                        "{} {} · {} {} · {} {}",
                        st.doc.page_count(),
                        crate::i18n::tr(&app.ui.language, "pages"),
                        st.doc.stories.len(),
                        crate::i18n::tr(&app.ui.language, "stories"),
                        st.doc.all_items().len(),
                        crate::i18n::tr(&app.ui.language, "items")
                    ),
                );
            }
        }
    }
    crate::rtl::label(
        ui,
        egui::RichText::new(format!("{} {:.1} ms", crate::i18n::tr(&app.ui.language, "Render"), app.perf.render_ms)).color(t.text_dim),
    );
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
        egui::ComboBox::from_id_salt("blend")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, it.blend.label())))
            .width(130.0)
            .show_ui(ui, |ui| {
                for m in MODES {
                    if ui.selectable_label(m == it.blend, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, m.label()))).clicked() {
                        let _ = app.run("object.opacity", json!({"opacity": it.opacity, "blend": m}));
                    }
                }
            });
        caption(ui, crate::i18n::tr(&app.ui.language, "Opacity"));
        if let Some(v) = number(ui, "eop", Some(it.opacity as f64 * 100.0), "%", 46.0, 0) {
            let _ = app.run("object.opacity", json!({"opacity": (v / 100.0).clamp(0.0, 1.0)}));
        }
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
}

pub fn preflight_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let issues = designcraft_engine::cmd::preflight::check(&app.session, 150.0);
    let errors = issues.iter().filter(|i| i.severity == "error").count();
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(
            r.center(),
            4.5,
            if errors == 0 { egui::Color32::from_rgb(60, 200, 90) } else { egui::Color32::from_rgb(235, 50, 50) },
        );
        crate::rtl::label(
            ui,
            if issues.is_empty() {
                crate::i18n::tr(&app.ui.language, "No errors").to_string()
            } else {
                format!(
                    "{errors} {}, {} {}",
                    crate::i18n::tr(&app.ui.language, "errors"),
                    issues.len() - errors,
                    crate::i18n::tr(&app.ui.language, "warnings")
                )
            },
        );
    });
    divider(ui);
    for i in issues {
        let page = i.page.filter(|_| app.session.active().is_some()).map(|p| app.session.page_label(p));
        let text = format!("{}  {}", if i.severity == "error" { "⛔" } else { "⚠" }, i.message);
        let resp = ui.add(egui::Button::new(egui::RichText::new(text).size(11.0)).frame(false));
        if let Some(pg) = &page {
            crate::rtl::label(ui, egui::RichText::new(format!("   page {pg}")).size(10.0).color(t.text_dim));
        }
        if resp.clicked() {
            if let Some(id) = i.item {
                let _ = app.run("selection.set", json!({"ids": [id]}));
            }
            if let Some(p) = i.page {
                crate::canvas::go_to_page(app, p);
            }
        }
    }
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

/// Variable Font sliders: one per axis; the style becomes `Named {tag:value,…}`.
fn variable_font_axes(app: &mut DesignApp, ui: &mut egui::Ui, family: &str, style: &str) {
    if family.is_empty() {
        return;
    }
    let db = designcraft_fonts::FontDb::global();
    let axes = db.axes(family, style);
    if axes.is_empty() {
        return;
    }
    let base = designcraft_fonts::base_style(style).to_string();
    let face = db.face(family, style);
    let cur = |tag: &str, default: f32| face.coords.iter().find(|(t, _)| t == tag.as_bytes()).map_or(default, |c| c.1);
    let mut values: Vec<(String, f32)> = axes.iter().map(|a| (a.0.clone(), cur(&a.0, a.3))).collect();
    let mut changed = false;
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Variable Font")).size(11.0));
    egui::Grid::new("vf_axes").num_columns(2).spacing(vec2(6.0, 2.0)).show(ui, |ui| {
        for (i, (_, name, min, _, max)) in axes.iter().enumerate() {
            caption(ui, name);
            if ui.add(egui::Slider::new(&mut values[i].1, *min..=*max).step_by(1.0)).drag_stopped() {
                changed = true;
            }
            ui.end_row();
        }
    });
    if changed {
        let spec = values.iter().map(|(t, v)| format!("{t}:{}", v.round())).collect::<Vec<_>>().join(",");
        let _ = app.run("type.char", json!({"attrs": {"fontStyle": format!("{base} {{{spec}}}")}}));
    }
}
