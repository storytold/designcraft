use super::{
    DesignApp, FIELD_H, NumField, Rect, Sense, Tokens, Ui, Value, block, button_pair, divider, dropdown, full_width, icon_toggle, icons, json,
    number, outline_button, place, pos2, rule, section, section_row, set_temp_bool, sub, temp_bool, units, vec2, widgets,
};

// ---------------------------------------------------------------- No Selection

/// Page-size presets for the Document dropdown (portrait, points).
pub(crate) const PAGE_PRESETS: &[(&str, f64, f64)] = &[
    ("Letter", 612.0, 792.0),
    ("Legal", 612.0, 1008.0),
    ("Tabloid", 792.0, 1224.0),
    ("Letter - Half", 396.0, 612.0),
    ("Legal - Half", 504.0, 612.0),
    ("A0", 841.0 * units::PT_PER_MM, 1189.0 * units::PT_PER_MM),
    ("A1", 594.0 * units::PT_PER_MM, 841.0 * units::PT_PER_MM),
    ("A2", 420.0 * units::PT_PER_MM, 594.0 * units::PT_PER_MM),
    ("A3", 841.89, 1190.55),
    ("A4", 595.276, 841.89),
    ("A5", 419.528, 595.276),
    ("A6", 297.638, 419.528),
    ("B5", 498.898, 708.661),
    ("US Business Card", 144.0, 252.0),
];

pub(crate) fn preset_name(w: f64, h: f64) -> &'static str {
    let (a, b) = (w.min(h), w.max(h));
    PAGE_PRESETS.iter().find(|p| (p.1 - a).abs() < 0.6 && (p.2 - b).abs() < 0.6).map(|p| p.0).unwrap_or("Custom")
}

/// The preset dropdown and orientation toggles for one page; the new (width, height) when one changed.
fn page_preset_row(ui: &mut Ui, lang: &str, w: f64, h: f64) -> Option<(f64, f64)> {
    let landscape = w > h;
    let b = block(ui, FIELD_H);
    let mut size = None;
    place(ui, sub(b, 0.0, 0.0, 93.0, FIELD_H), |ui| {
        let resp = dropdown(ui, crate::i18n::tr(lang, preset_name(w, h)), 93.0);
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(140.0);
            for p in PAGE_PRESETS {
                if ui.selectable_label(preset_name(w, h) == p.0, crate::i18n::tr(lang, p.0)).clicked() {
                    size = Some(if landscape { (p.2, p.1) } else { (p.1, p.2) });
                    ui.close();
                }
            }
        });
    });
    place(ui, sub(b, 126.0, 0.0, 52.0, FIELD_H), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if icon_toggle(ui, "orient-portrait", !landscape, crate::i18n::tr(lang, "Portrait")).clicked() && landscape {
            size = Some((h, w));
        }
        if icon_toggle(ui, "orient-landscape", landscape, crate::i18n::tr(lang, "Landscape")).clicked() && !landscape {
            size = Some((h, w));
        }
    });
    size
}

/// Properties with the Page tool (Properties › Edit Page), as InDesign's: a "Page" band with Back,
/// Page Size for this page alone (which page, preset, orientation, W/H), Margins and Columns and
/// Quick Actions. Back returns to the Selection tool and the document's properties.
pub(super) fn page_sections(app: &mut DesignApp, ui: &mut Ui, page: usize) {
    let u = units(app);
    let lang = app.ui.language.clone();
    let Some(st) = app.session.active() else { return };
    let d = st.doc.clone();
    let Some((si, pi)) = d.page_loc(page) else { return };
    let Some(pg) = d.spreads.get(si).and_then(|s| s.pages.get(pi)).cloned() else { return };
    let n = page + 1;
    let t = Tokens::get(ui.ctx());
    // The band: "Page", and Back at its end.
    let band = block(ui, 33.0);
    crate::rtl::paint(
        ui.painter(),
        pos2(band.min.x, band.center().y + 1.0),
        egui::Align2::LEFT_CENTER,
        crate::i18n::tr(&lang, "Page"),
        crate::theme::semibold(11.5),
        t.text_strong,
    );
    let back = place(ui, Rect::from_min_size(pos2(band.max.x - 58.0, band.center().y - 10.0), vec2(54.0, 20.0)), |ui| {
        outline_button(ui, crate::i18n::tr(&lang, "Back"), 54.0).clicked()
    });
    rule(ui);
    if back {
        let _ = app.run("layout.selectPage", json!({"page": null}));
        app.select_tool("selection");
        return;
    }
    section(ui, crate::i18n::tr(&lang, "Page Size"), true);
    // Which page.
    let b = block(ui, FIELD_H);
    let names: Vec<String> = (0..d.page_count()).map(|i| app.session.page_label(i)).collect();
    let picked = place(ui, sub(b, 0.0, 0.0, 93.0, FIELD_H), |ui| {
        widgets::dropdown_list(ui, names.get(page).map_or("", String::as_str), 93.0, &names, Some(page))
    });
    if let Some(p) = picked {
        let _ = app.run("layout.selectPage", json!({"page": p + 1}));
        crate::canvas::go_to_page(app, p);
        return;
    }
    ui.add_space(3.0);
    // Preset and orientation: this page alone.
    let (w, h) = (pg.width, pg.height);
    let mut size = page_preset_row(ui, &lang, w, h);
    ui.add_space(3.0);
    let b = block(ui, 2.0 * FIELD_H + 5.0);
    for (row, (label, v, key)) in [("W:", w, "width"), ("H:", h, "height")].into_iter().enumerate() {
        let y = row as f32 * 26.0;
        place(ui, sub(b, 0.0, y, 20.0, FIELD_H), |ui| widgets::caption_w(ui, label, 20.0));
        let nv = place(ui, sub(b, 21.0, y, 72.0, FIELD_H), |ui| {
            NumField::measure(&format!("page_{key}"), Some(v), u).width(72.0).spinner().range(1.0, 15552.0).show(ui)
        });
        if let Some(nv) = nv {
            size = Some(if key == "width" { (nv, h) } else { (w, nv) });
        }
    }
    if let Some((nw, nh)) = size {
        let _ = app.run("layout.pageSize", json!({"pages": [n], "width": nw, "height": nh, "anchor": app.ui.ref_point.min(8)}));
    }
    ui.add_space(6.0);
    let fw = full_width(ui);
    if outline_button(ui, crate::i18n::tr(&lang, "Margins and Columns"), fw).clicked() {
        crate::canvas::go_to_page(app, page);
        crate::menus::activate(app, "layout.marginsAndColumns", &Value::Null);
    }
    divider(ui);
    section(ui, crate::i18n::tr(&lang, "Quick Actions"), true);
    if outline_button(ui, crate::i18n::tr(&lang, "Import File"), fw).clicked() {
        let _ = app.run("app.placeDialog", json!({}));
    }
}

/// Page count field: add pages after the last one, or delete from the end, to reach `nv` (from `n`).
fn resize_document(app: &mut DesignApp, n: usize, nv: f64) {
    let want = (nv.round() as usize).clamp(1, 9999);
    if want > n {
        let _ = app.run("layout.pages.insert", json!({"count": want - n, "after": n - 1}));
    } else if want < n {
        let _ = app.run("layout.pages.delete", json!({"pages": (want..n).collect::<Vec<_>>()}));
    }
}

/// The margins after field `k` (top, bottom, outside, inside) is set to `nv`; all four when linked.
fn edited_margins(mut m: designcraft_doc::Margins, linked: bool, k: usize, nv: f64) -> designcraft_doc::Margins {
    if linked {
        return designcraft_doc::Margins { top: nv, bottom: nv, inside: nv, outside: nv };
    }
    match k {
        0 => m.top = nv,
        1 => m.bottom = nv,
        2 => m.outside = nv,
        _ => m.inside = nv,
    }
    m
}

/// The page-size preset menu; the width and height of the preset chosen.
fn page_preset_menu(ui: &mut Ui, pw: f64, ph: f64) -> Option<(f64, f64)> {
    let mut preset = None;
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
    preset
}

/// As the status bar's page box: the page, or the parent shown ("A-Parent"); then the parents.
fn page_box_field(app: &mut DesignApp, ui: &mut Ui, b: Rect) {
    let Some(pb) = crate::canvas::page_box(app) else { return };
    let picked = place(ui, sub(b, 0.0, 0.0, 93.0, FIELD_H), |ui| widgets::dropdown_list(ui, pb.current_name(), 93.0, &pb.names, Some(pb.current)));
    if let Some(i) = picked {
        crate::canvas::page_box_go(app, &pb, i);
    }
}

pub(super) fn document_sections(app: &mut DesignApp, ui: &mut Ui) {
    let u = units(app);
    let Some(st) = app.session.active() else { return };
    let d = st.doc.clone();
    let (pw, ph) = (d.settings.page_width, d.settings.page_height);
    section(ui, crate::i18n::tr(&app.ui.language, "Document"), true);
    // Preset dropdown + orientation.
    let b = block(ui, FIELD_H);
    let preset = place(ui, sub(b, 0.0, 0.0, 93.0, FIELD_H), |ui| page_preset_menu(ui, pw, ph));
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
        resize_document(app, n, nv);
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
    // While parent spreads are edited the fields belong to the parent page in view; they used to
    // show (and change) the document pages instead, so nothing changed on the parent.
    let parent_target = crate::canvas::current_parent_page(app);
    let shown = match parent_target {
        Some((pi, qi)) => d.parents.get(pi).and_then(|s| s.pages.get(qi)).cloned(),
        None => d.page(cur).cloned(),
    };
    if let Some(p) = shown {
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
            ui.interact(ir, ui.scope_id().with(("micon", k)), Sense::hover()).on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, tip));
            });
            let nv = place(ui, sub(b, x + 20.0, y, 73.0, FIELD_H), |ui| {
                NumField::measure(tip, Some(v), u).width(73.0).spinner().range(0.0, 8000.0).show(ui)
            });
            if let Some(nv) = nv {
                set = Some(edited_margins(p.margins, linked, k, nv));
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
            let params = match parent_target {
                // Both pages of a facing parent spread, so the margins show wherever the parent is used.
                Some((pi, _)) => {
                    let n = app.session.active().and_then(|st| st.doc.parents.get(pi)).map_or(1, |sp| sp.pages.len());
                    json!({"margins": m, "parent": pi, "pages": (0..n).collect::<Vec<_>>()})
                }
                None => json!({"margins": m}),
            };
            let _ = app.run("layout.marginsAndColumns", params);
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
    page_box_field(app, ui, b);
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
        let _ = crate::dialogs::delete_pages(app, vec![cur]);
    }
    ui.add_space(3.0);
    if outline_button(ui, crate::i18n::tr(&app.ui.language, "Edit Page"), fw).clicked() {
        // As in InDesign: the Page tool on this page; Properties shows the page's own settings.
        app.select_tool("page");
        let _ = app.run("layout.selectPage", json!({"page": cur + 1}));
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
    if dg != app.ui.document_grid {
        let _ = app.run("view.documentGrid", json!({}));
    }
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
    if locked != app.ui.guides_locked {
        let _ = app.run("view.lockGuides", json!({"on": locked}));
    }
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
