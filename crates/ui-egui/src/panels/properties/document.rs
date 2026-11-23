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

/// The margin fields: icon, name (with facing pages, and without) and where the icon sits in the block; in the order
/// [`edited_margins`] numbers them (top, bottom, outside, inside).
const MARGIN_FIELDS: [(&str, &str, &str, f32, f32); 4] = [
    ("margin-top", "Top", "Top", 0.0, 0.0),
    ("margin-bottom", "Bottom", "Bottom", 0.0, 26.0),
    ("margin-outside", "Outside", "Right", 127.0, 0.0),
    ("margin-inside", "Inside", "Left", 127.0, 26.0),
];

/// The margins `m` in the order of [`MARGIN_FIELDS`].
fn margin_values(m: &designcraft_doc::Margins) -> [f64; 4] {
    [m.top, m.bottom, m.outside, m.inside]
}

/// The page or parent page the Margins fields change: while parent spreads are edited, the parent page in view (they
/// used to show and change the document pages instead, so nothing changed on the parent); else the current page.
fn margin_target(app: &DesignApp, d: &designcraft_doc::Document) -> (Option<(usize, usize)>, Option<designcraft_doc::Page>) {
    let parent_target = crate::canvas::current_parent_page(app);
    let shown = match parent_target {
        Some((pi, qi)) => d.parents.get(pi).and_then(|s| s.pages.get(qi)).cloned(),
        None => d.page(crate::canvas::current_page(app).unwrap_or(0)).cloned(),
    };
    (parent_target, shown)
}

/// Runs `layout.marginsAndColumns` with the margins `m` for the target [`margin_target`] found.
fn set_margins(app: &mut DesignApp, parent_target: Option<(usize, usize)>, m: designcraft_doc::Margins) {
    let params = match parent_target {
        Some((pi, qi)) => json!({"margins": m, "parent": pi, "pages": [qi]}),
        None => json!({"margins": m}),
    };
    let _ = app.run("layout.marginsAndColumns", params);
}

/// Leaves the Page tool's page settings: out of parent editing, no page selected, the Selection tool.
fn leave_page(app: &mut DesignApp, spread: designcraft_doc::SpreadRef) {
    if matches!(spread, designcraft_doc::SpreadRef::Parent(_)) {
        let _ = app.run("layout.parents.edit", json!({"on": false}));
    }
    let _ = app.run("layout.selectPage", json!({"page": null}));
    app.select_tool("selection");
}

/// The page names the Page Size section's page list shows for `spread`, and the one of `page`.
fn page_names(app: &DesignApp, d: &designcraft_doc::Document, spread: designcraft_doc::SpreadRef, page: usize) -> (Vec<String>, usize) {
    match spread {
        designcraft_doc::SpreadRef::Doc(si) => ((0..d.page_count()).map(|i| app.session.page_label(i)).collect(), d.first_page_of_spread(si) + page),
        designcraft_doc::SpreadRef::Parent(_) => {
            let Some(sp) = d.spread(spread) else { return (Vec::new(), page) };
            ((0..sp.pages.len()).map(|i| format!("{} · {}", sp.parent.as_ref().map(|p| p.label()).unwrap_or_default(), i + 1)).collect(), page)
        }
    }
}

/// Shows page `p` of the Page Size section's list for `spread`.
fn pick_page(app: &mut DesignApp, d: &designcraft_doc::Document, spread: designcraft_doc::SpreadRef, p: usize) {
    match spread {
        designcraft_doc::SpreadRef::Doc(_) => {
            let _ = app.run("layout.selectPage", json!({"page": p + 1}));
            crate::canvas::go_to_page(app, p);
        }
        designcraft_doc::SpreadRef::Parent(_) => {
            if let Some(sp) = d.spread(spread) {
                let _ = app.run("layout.selectParentPage", json!({"parent": sp.id.0, "page": p}));
            }
        }
    }
}

/// Opens Margins and Columns for `page` of `spread`, shown first.
fn margins_and_columns(app: &mut DesignApp, d: &designcraft_doc::Document, spread: designcraft_doc::SpreadRef, page: usize) {
    match spread {
        designcraft_doc::SpreadRef::Doc(si) => crate::canvas::go_to_page(app, d.first_page_of_spread(si) + page),
        designcraft_doc::SpreadRef::Parent(_) => crate::canvas::show_spread(app, spread),
    }
    crate::menus::activate(app, "layout.marginsAndColumns", &Value::Null);
}

/// Sets `page` of `spread` to `size`, about the reference point.
fn set_page_size(app: &mut DesignApp, d: &designcraft_doc::Document, spread: designcraft_doc::SpreadRef, page: usize, size: (f64, f64)) {
    let (w, h) = size;
    if let Some((cmd, params)) =
        designcraft_tools::layout::page_size_command(d, spread, page, json!({"width": w, "height": h, "anchor": app.ui.ref_point.min(8)}))
    {
        let _ = app.run(cmd, params);
    }
}

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
/// Quick Actions. Back leaves parent editing and returns to the Selection tool and document properties.
pub(super) fn page_sections(app: &mut DesignApp, ui: &mut Ui, spread: designcraft_doc::SpreadRef, page: usize) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::page_sections(app, ui, spread, page);
    }
    let u = units(app);
    let lang = app.ui.language.clone();
    let Some(st) = app.session.active() else { return };
    let d = st.doc.clone();
    let Some(sp) = d.spread(spread) else { return };
    let Some(pg) = sp.pages.get(page).cloned() else { return };
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
        leave_page(app, spread);
        return;
    }
    section(ui, crate::i18n::tr(&lang, "Page Size"), true);
    // Which page.
    let b = block(ui, FIELD_H);
    let (names, selected) = page_names(app, &d, spread, page);
    let picked = place(ui, sub(b, 0.0, 0.0, 93.0, FIELD_H), |ui| {
        widgets::dropdown_list(ui, names.get(selected).map_or("", String::as_str), 93.0, &names, Some(selected))
    });
    if let Some(p) = picked {
        pick_page(app, &d, spread, p);
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
    if let Some(s) = size {
        set_page_size(app, &d, spread, page, s);
    }
    if matches!(spread, designcraft_doc::SpreadRef::Parent(_)) {
        margin_section(app, ui);
    }
    ui.add_space(6.0);
    let fw = full_width(ui);
    if outline_button(ui, crate::i18n::tr(&lang, "Margins and Columns"), fw).clicked() {
        margins_and_columns(app, &d, spread, page);
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

fn margin_section(app: &mut DesignApp, ui: &mut Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::margin_section(app, ui);
    }
    let u = units(app);
    let Some(st) = app.session.active() else { return };
    let d = st.doc.clone();
    let t = Tokens::get(ui.ctx());
    section(ui, crate::i18n::tr(&app.ui.language, "Margins"), true);
    let (parent_target, shown) = margin_target(app, &d);
    if let Some(p) = shown {
        let linked = temp_bool(ui, "margins_linked", true);
        let b = block(ui, 2.0 * FIELD_H + 5.0);
        let mut set: Option<designcraft_doc::Margins> = None;
        let values = margin_values(&p.margins);
        for (k, (icon, facing, single, x, y)) in MARGIN_FIELDS.into_iter().enumerate() {
            let (tip, v) = (if d.settings.facing_pages { facing } else { single }, values[k]);
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
            set_margins(app, parent_target, m);
        }
    }
}

pub(super) fn document_sections(app: &mut DesignApp, ui: &mut Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::document_sections(app, ui);
    }
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
    margin_section(app, ui);
    let cur = crate::canvas::current_page(app).unwrap_or(0);
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

/// The Document, Page and Margins sections on the craft UI framework: the same rows over the same layout and view
/// commands as the native bodies above, every control a framework component.
#[cfg(not(target_arch = "wasm32"))]
mod framework {
    use super::*;
    use crate::framework_transform::{Number, Quantity, button, caption, check, choice, icon_button, icon_number, number_field};
    use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
    use craft_ui_widgets::text::link_toggle;

    /// The id of the control `key` of the section the `ui` shows.
    fn cid(ui: &Ui, key: impl std::hash::Hash + std::fmt::Debug) -> egui::Id {
        ui.id().with(("properties-document", key))
    }

    /// The page-size preset select at `at` over `w` × `h` (Custom when it is none of them), and the orientation
    /// buttons after it: the new (width, height) of the page when one of them changed it.
    fn preset_row(ui: &mut Ui, lang: &str, key: &str, w: f64, h: f64) -> Option<(f64, f64)> {
        let landscape = w > h;
        let b = block(ui, FIELD_H);
        let choices: Vec<(usize, String)> = PAGE_PRESETS.iter().enumerate().map(|(i, p)| (i, crate::i18n::tr(lang, p.0).to_owned())).collect();
        let current = PAGE_PRESETS.iter().position(|p| p.0 == preset_name(w, h));
        let id = cid(ui, (key, "preset"));
        let custom = crate::i18n::tr(lang, "Custom");
        let mut size = choice(ui, Some(sub(b, 0.0, 0.0, 93.0, FIELD_H)), id, crate::i18n::tr(lang, "Page Size"), current, &choices, Some(custom))
            .map(|i| PAGE_PRESETS[i])
            .map(|p| if landscape { (p.2, p.1) } else { (p.1, p.2) });
        for (k, (artwork, tip, on)) in
            [("orient-portrait", "Portrait", !landscape), ("orient-landscape", "Landscape", landscape)].into_iter().enumerate()
        {
            let (at, id) = (sub(b, 126.0 + k as f32 * 27.0, 0.0, 23.0, FIELD_H), cid(ui, (key, "orientation", k)));
            if icon_button(ui, at, id, artwork, crate::i18n::tr(lang, tip), on) && !on {
                size = Some((h, w));
            }
        }
        size
    }

    /// The W and H fields of a page size in the document's unit, in block `b`: the new (width, height) when one was
    /// committed.
    fn size_fields(ui: &mut Ui, lang: &str, key: &str, b: Rect, size: (f64, f64), u: designcraft_geom::Unit) -> Option<(f64, f64)> {
        let (w, h) = size;
        let mut set = None;
        for (row, (label, name, v)) in [("W:", "Width", w), ("H:", "Height", h)].into_iter().enumerate() {
            let y = row as f32 * 26.0;
            caption(ui, Some(sub(b, 0.0, y, 20.0, FIELD_H)), label);
            let id = cid(ui, (key, name));
            let n = Number::new(crate::i18n::tr(lang, name), Some(v), Quantity::Length(u)).range(1.0, 15552.0);
            if let Some(Some(nv)) = number_field(ui, Some(sub(b, 21.0, y, 72.0, FIELD_H)), id, n) {
                set = Some(if row == 0 { (nv, h) } else { (w, nv) });
            }
        }
        set
    }

    /// A section title with its toggles at the row's end, the first at the far end (Rulers & Grids, Guides): the index
    /// of the one clicked.
    fn title_row(app: &DesignApp, ui: &mut Ui, title: &str, toggles: &[(&'static str, &str, bool)]) -> Option<usize> {
        ui.add_space(8.0);
        let b = block(ui, 23.0);
        caption(ui, Some(sub(b, 0.0, 0.0, 100.0, 23.0)), crate::i18n::tr(&app.ui.language, title));
        let mut clicked = None;
        for (k, (artwork, tip, on)) in toggles.iter().enumerate() {
            let at = sub(b, b.width() - 4.0 - 23.0 - k as f32 * 37.0, 1.0, 23.0, 21.0);
            let id = cid(ui, (title, k));
            if icon_button(ui, at, id, artwork, crate::i18n::tr(&app.ui.language, tip), *on) {
                clicked = Some(k);
            }
        }
        ui.add_space(6.0);
        clicked
    }

    pub(super) fn page_sections(app: &mut DesignApp, ui: &mut Ui, spread: designcraft_doc::SpreadRef, page: usize) {
        let u = units(app);
        let lang = app.ui.language.clone();
        let Some(st) = app.session.active() else { return };
        let d = st.doc.clone();
        let Some(sp) = d.spread(spread) else { return };
        let Some(pg) = sp.pages.get(page).cloned() else { return };
        // The band: "Page", and Back at its end.
        let band = block(ui, 33.0);
        craft_ui_widgets::display::Text::new(crate::i18n::tr(&lang, "Page").to_owned())
            .role(craft_ui_widgets::style::TypeRole::BodyStrong)
            .show_at(ui, Rect::from_min_max(band.min, pos2(band.max.x - 62.0, band.max.y)));
        let id = cid(ui, "back");
        let back = button(
            ui,
            Some(Rect::from_min_size(pos2(band.max.x - 58.0, band.center().y - 10.0), vec2(54.0, 20.0))),
            id,
            crate::i18n::tr(&lang, "Back"),
            true,
        )
        .clicked();
        rule(ui);
        if back {
            leave_page(app, spread);
            return;
        }
        section(ui, crate::i18n::tr(&lang, "Page Size"), true);
        // Which page.
        let b = block(ui, FIELD_H);
        let (names, selected) = page_names(app, &d, spread, page);
        let choices: Vec<(usize, String)> = names.iter().cloned().enumerate().collect();
        let id = cid(ui, "page");
        if let Some(p) = choice(ui, Some(sub(b, 0.0, 0.0, 93.0, FIELD_H)), id, crate::i18n::tr(&lang, "Page"), Some(selected), &choices, None) {
            if p != selected {
                pick_page(app, &d, spread, p);
                return;
            }
        }
        ui.add_space(3.0);
        // Preset and orientation, then W and H: this page alone.
        let mut size = preset_row(ui, &lang, "page", pg.width, pg.height);
        ui.add_space(3.0);
        let b = block(ui, 2.0 * FIELD_H + 5.0);
        if let Some(s) = size_fields(ui, &lang, "page", b, (pg.width, pg.height), u) {
            size = Some(s);
        }
        if let Some(s) = size {
            set_page_size(app, &d, spread, page, s);
        }
        if matches!(spread, designcraft_doc::SpreadRef::Parent(_)) {
            margin_section(app, ui);
        }
        ui.add_space(6.0);
        let fw = full_width(ui);
        let b = block(ui, widgets::BUTTON_H);
        let id = cid(ui, "margins-and-columns");
        if button(ui, Some(sub(b, 0.0, 0.0, fw, widgets::BUTTON_H)), id, crate::i18n::tr(&lang, "Margins and Columns"), true).clicked() {
            margins_and_columns(app, &d, spread, page);
        }
        divider(ui);
        section(ui, crate::i18n::tr(&lang, "Quick Actions"), true);
        let b = block(ui, widgets::BUTTON_H);
        let id = cid(ui, "page-import");
        if button(ui, Some(sub(b, 0.0, 0.0, fw, widgets::BUTTON_H)), id, crate::i18n::tr(&lang, "Import File"), true).clicked() {
            let _ = app.run("app.placeDialog", json!({}));
        }
    }

    pub(super) fn margin_section(app: &mut DesignApp, ui: &mut Ui) {
        let u = units(app);
        let lang = app.ui.language.clone();
        let Some(st) = app.session.active() else { return };
        let d = st.doc.clone();
        section(ui, crate::i18n::tr(&lang, "Margins"), true);
        let (parent_target, shown) = margin_target(app, &d);
        let Some(p) = shown else { return };
        let linked = temp_bool(ui, "margins_linked", true);
        let b = block(ui, 2.0 * FIELD_H + 5.0);
        let values = margin_values(&p.margins);
        let mut set = None;
        for (k, (artwork, facing, single, x, y)) in MARGIN_FIELDS.into_iter().enumerate() {
            let name = crate::i18n::tr(&lang, if d.settings.facing_pages { facing } else { single });
            let id = cid(ui, ("margin", k));
            let n = Number::new(name, Some(values[k]), Quantity::Length(u)).range(0.0, 8000.0);
            if let Some(Some(nv)) = icon_number(ui, sub(b, x, y + 2.0, 17.0, 17.0), sub(b, x + 20.0, y, 73.0, FIELD_H), id, artwork, n) {
                set = Some(edited_margins(p.margins, linked, k, nv));
            }
        }
        let tip = crate::i18n::tr(&lang, "Make all settings the same").to_owned();
        let id = cid(ui, "margins-link");
        let toggled = link_toggle(tip.clone(), Bind::given(linked))
            .id(id)
            .tooltip(tip)
            .show_at(ui, sub(b, 99.0, 12.5, 22.0, FIELD_H))
            .events
            .committed()
            .copied();
        if let Some(on) = toggled {
            set_temp_bool(ui, "margins_linked", on);
        }
        if let Some(m) = set {
            set_margins(app, parent_target, m);
        }
    }

    pub(super) fn document_sections(app: &mut DesignApp, ui: &mut Ui) {
        let u = units(app);
        let lang = app.ui.language.clone();
        let Some(st) = app.session.active() else { return };
        let d = st.doc.clone();
        let (pw, ph) = (d.settings.page_width, d.settings.page_height);
        section(ui, crate::i18n::tr(&lang, "Document"), true);
        if let Some((w, h)) = preset_row(ui, &lang, "document", pw, ph) {
            let _ = app.run("layout.documentSetup", json!({"width": w, "height": h}));
        }
        ui.add_space(3.0);
        // W / H, page count, facing pages.
        let b = block(ui, 2.0 * FIELD_H + 5.0);
        if let Some((w, h)) = size_fields(ui, &lang, "document", b, (pw, ph), u) {
            let params = if (w - pw).abs() > 0.0 { json!({"width": w}) } else { json!({"height": h}) };
            let _ = app.run("layout.documentSetup", params);
        }
        let n = d.page_count();
        let id = cid(ui, "page-count");
        let count = Number::new(crate::i18n::tr(&lang, "Number of Pages"), Some(n as f64), Quantity::Plain { decimals: 0 }).range(1.0, 9999.0);
        if let Some(Some(nv)) = icon_number(ui, sub(b, 127.0, 2.0, 17.0, 17.0), sub(b, 150.0, 0.0, 68.0, FIELD_H), id, "page-count", count) {
            resize_document(app, n, nv);
        }
        let id = cid(ui, "facing-pages");
        if let Some(on) =
            check(ui, Some(sub(b, 126.0, 26.0, 100.0, FIELD_H)), id, crate::i18n::tr(&lang, "Facing Pages"), d.settings.facing_pages, true)
        {
            let _ = app.run("layout.documentSetup", json!({"facingPages": on}));
        }
        margin_section(app, ui);
        let cur = crate::canvas::current_page(app).unwrap_or(0);
        ui.add_space(6.0);
        let fw = full_width(ui);
        let h = widgets::BUTTON_H;
        let b = block(ui, h);
        let id = cid(ui, "adjust-layout");
        if button(ui, Some(sub(b, 0.0, 0.0, fw, h)), id, crate::i18n::tr(&lang, "Adjust Layout"), true).clicked() {
            crate::dialogs::open_document_setup(app);
        }
        divider(ui);
        // Page: the page box, Insert Page and Delete Page, Edit Page.
        section(ui, crate::i18n::tr(&lang, "Page"), true);
        let b = block(ui, FIELD_H);
        if let Some(pb) = crate::canvas::page_box(app) {
            let choices: Vec<(usize, String)> = pb.names.iter().cloned().enumerate().collect();
            let id = cid(ui, "page-box");
            if let Some(i) = choice(ui, Some(sub(b, 0.0, 0.0, 93.0, FIELD_H)), id, crate::i18n::tr(&lang, "Page"), Some(pb.current), &choices, None)
                && i != pb.current
            {
                crate::canvas::page_box_go(app, &pb, i);
            }
        }
        let id = cid(ui, "insert-page");
        let add = icon_button(ui, sub(b, 172.0, 0.0, 22.0, FIELD_H), id, "style-new", crate::i18n::tr(&lang, "Insert Page"), false);
        let id = cid(ui, "delete-page");
        let del = icon_button(ui, sub(b, 198.0, 0.0, 22.0, FIELD_H), id, "trash", crate::i18n::tr(&lang, "Delete Page"), false);
        if add {
            let _ = app.run("layout.pages.insert", json!({"after": cur, "count": 1}));
        }
        if del {
            let _ = crate::dialogs::delete_pages(app, vec![cur]);
        }
        ui.add_space(3.0);
        let b = block(ui, h);
        let id = cid(ui, "edit-page");
        if button(ui, Some(sub(b, 0.0, 0.0, fw, h)), id, crate::i18n::tr(&lang, "Edit Page"), true).clicked() {
            // As in InDesign: the Page tool on this page; Properties shows the page's own settings.
            app.select_tool("page");
            let _ = app.run("layout.selectPage", json!({"page": cur + 1}));
        }
        divider(ui);
        // Rulers & Grids, then Guides: toggles at each row's end.
        let grids = [
            ("grid-document", "Show Document Grid", app.ui.document_grid),
            ("grid-baseline", "Show Baseline Grid", app.ui.baseline_grid),
            ("rulers", "Show Rulers", app.ui.rulers),
        ];
        match title_row(app, ui, "Rulers & Grids", &grids) {
            Some(0) => {
                let _ = app.run("view.documentGrid", json!({}));
            }
            Some(1) => {
                let _ = app.run("view.baselineGrid", json!({}));
            }
            Some(_) => {
                let _ = app.run("view.rulers", json!({}));
            }
            None => {}
        }
        rule(ui);
        let locked = app.ui.guides_locked;
        let guides = [
            ("guides-smart", "Smart Guides", app.ui.smart_guides),
            ("guides-lock", "Lock Guides", locked),
            ("guides-show", "Show Guides", app.ui.guides),
        ];
        match title_row(app, ui, "Guides", &guides) {
            Some(0) => {
                let _ = app.run("view.smartGuides", json!({}));
            }
            Some(1) => {
                let _ = app.run("view.lockGuides", json!({"on": !locked}));
            }
            Some(_) => {
                let _ = app.run("view.guides", json!({}));
            }
            None => {}
        }
        rule(ui);
        section(ui, crate::i18n::tr(&lang, "Quick Actions"), true);
        let half = (fw - 10.0) / 2.0;
        let b = block(ui, h);
        let id = cid(ui, "import-file");
        if button(ui, Some(sub(b, 0.0, 0.0, half, h)), id, crate::i18n::tr(&lang, "Import File"), true).clicked() {
            let _ = app.run("app.placeDialog", json!({}));
        }
        let id = cid(ui, "quick-insert-page");
        if button(ui, Some(sub(b, half + 10.0, 0.0, half, h)), id, crate::i18n::tr(&lang, "Insert Page"), true).clicked() {
            let _ = app.run("layout.pages.insert", json!({"after": cur, "count": 1}));
        }
    }
}

/// The Document, Page and Margins sections on the framework, in the real Properties panel with nothing selected, driven
/// with real pointer and key events.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod framework_tests {
    use super::super::framework_test_kit::*;
    use egui::accesskit::Role;
    use serde_json::json;

    fn properties(app: &mut crate::DesignApp, ui: &mut egui::Ui) {
        crate::panels::properties::show(app, ui);
    }

    fn settings(h: &Window) -> designcraft_doc::DocSettings {
        h.state().session.active().expect("a document").doc.settings.clone()
    }

    #[test]
    fn a_preset_and_the_orientation_set_the_document_size() {
        let mut h = open(document(), properties);
        pick(&mut h, "Page Size", "A4");
        let s = settings(&h);
        assert!((s.page_width - 595.276).abs() < 0.01 && (s.page_height - 841.89).abs() < 0.01, "A4 portrait: {} × {}", s.page_width, s.page_height);
        click(&mut h, Role::Button, "Landscape");
        let s = settings(&h);
        assert!(s.page_width > s.page_height, "landscape swaps the sides: {} × {}", s.page_width, s.page_height);
        assert_eq!(journal(&h, "layout.documentSetup").len(), 2, "one command each");
    }

    #[test]
    fn the_page_count_facing_pages_and_a_margin_run_their_layout_commands() {
        let mut h = open(document(), properties);
        type_into(&mut h, "Number of Pages", "3");
        assert_eq!(h.state().session.active().unwrap().doc.page_count(), 3);
        let facing = settings(&h).facing_pages;
        click(&mut h, Role::CheckBox, "Facing Pages");
        assert_eq!(settings(&h).facing_pages, !facing);
        type_into(&mut h, "Top", "2p0");
        let m = journal(&h, "layout.marginsAndColumns").last().cloned().expect("Margins ran layout.marginsAndColumns");
        // Linked by default: all four sides take 2p0 (24 pt).
        assert_eq!(m["margins"]["top"], json!(24.0));
        assert_eq!(m["margins"]["bottom"], json!(24.0));
    }

    #[test]
    fn the_rulers_and_guides_toggles_run_their_view_commands() {
        let mut h = open(document(), properties);
        let rulers = h.state().ui.rulers;
        click(&mut h, Role::Button, "Show Rulers");
        assert_eq!(h.state().ui.rulers, !rulers);
        let locked = h.state().ui.guides_locked;
        click(&mut h, Role::Button, "Lock Guides");
        assert_eq!(h.state().ui.guides_locked, !locked);
        assert_eq!(journal(&h, "view.lockGuides").last(), Some(&json!({"on": !locked})));
    }

    #[test]
    fn insert_page_adds_a_page_after_the_current_one() {
        let mut h = open(document(), properties);
        let before = h.state().session.active().unwrap().doc.page_count();
        click(&mut h, Role::Button, "Insert Page");
        assert_eq!(h.state().session.active().unwrap().doc.page_count(), before + 1);
    }
}
