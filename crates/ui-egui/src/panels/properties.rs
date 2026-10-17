//! The Properties panel (context-sensitive, laid out as InDesign 2026's: selection-type band,
//! sections per selection state — `plan/indesign/11-observed-ui.md` §4.1) and small
//! single-purpose panels (Stroke, Character, Paragraph, Text Wrap, Align, Links, Info).

use designcraft_doc::{Align, Arrowhead, Cap, Join, StrokeAlign, StrokeType};
use designcraft_geom::corners::CornerShape;
use designcraft_geom::{Unit, units};
use egui::{Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use super::{SelInfo, sel_info, text_attrs};
use crate::theme::Tokens;
use crate::widgets::{
    self, FIELD_H, NumField, button_pair, caption, divider, dropdown, full_width, icon_toggle, link_label, measure, more_options, number,
    outline_button, rule, section, section_row, selection_band,
};
use crate::{DesignApp, icons};

/// A link to another panel. `window.panel` shows it in the docked layout or as a flyout; it
/// toggles the flyout, so a flyout that is already open is left as it is.
fn show_panel(app: &mut DesignApp, panel: &str) {
    if app.ui.open_panel.as_deref() != Some(panel) {
        let _ = app.run("window.panel", json!({"panel": panel}));
    }
}

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
            "<group>" => crate::i18n::tr_context(&app.ui.language, "Group", "selection").into(),
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
    if !(info.is_none() && !has_text && app.session.tool_id() == "page") {
        selection_band(ui, &selection_title(app, &info));
    }
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
    // The Page tool with no objects selected: the page's own settings, as InDesign's.
    if info.is_none() && app.session.tool_id() == "page" {
        let page = app.session.active().and_then(|st| st.selection.page).or_else(|| crate::canvas::current_page(app));
        if let Some(page) = page {
            page_sections(app, ui, page);
            return;
        }
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
    section(ui, crate::i18n::tr(&app.ui.language, "Flex Layout"), true);
    flex_section(app, ui);
    divider(ui);
    if i.count > 1 {
        section(ui, crate::i18n::tr(&app.ui.language, "Pathfinder"), true);
        pathfinder_row(app, ui);
        divider(ui);
    }
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

/// Properties with the Page tool (Properties › Edit Page), as InDesign's: a "Page" band with Back,
/// Page Size for this page alone (which page, preset, orientation, W/H), Margins and Columns and
/// Quick Actions. Back returns to the Selection tool and the document's properties.
fn page_sections(app: &mut DesignApp, ui: &mut Ui, page: usize) {
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
    let landscape = w > h;
    let b = block(ui, FIELD_H);
    let mut size = None;
    place(ui, sub(b, 0.0, 0.0, 93.0, FIELD_H), |ui| {
        let resp = dropdown(ui, crate::i18n::tr(&lang, preset_name(w, h)), 93.0);
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(140.0);
            for p in PAGE_PRESETS {
                if ui.selectable_label(preset_name(w, h) == p.0, crate::i18n::tr(&lang, p.0)).clicked() {
                    size = Some(if landscape { (p.2, p.1) } else { (p.1, p.2) });
                    ui.close();
                }
            }
        });
    });
    place(ui, sub(b, 126.0, 0.0, 52.0, FIELD_H), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if icon_toggle(ui, "orient-portrait", !landscape, crate::i18n::tr(&lang, "Portrait")).clicked() && landscape {
            size = Some((h, w));
        }
        if icon_toggle(ui, "orient-landscape", landscape, crate::i18n::tr(&lang, "Landscape")).clicked() && !landscape {
            size = Some((h, w));
        }
    });
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
pub(crate) fn ref_point_proxy(ui: &mut Ui, r: Rect, cur: u8) -> Option<u8> {
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
    transform_angles(app, ui, rp);
    if more_options(ui, &app.ui.language).clicked() {
        show_panel(app, "info");
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
            ui.interact(ir, ui.id().with(id), Sense::hover()).on_hover_text(crate::i18n::tr(&lang, tip));
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
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
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

fn appearance_section(app: &mut DesignApp, ui: &mut Ui, i: &SelInfo) {
    let t = Tokens::get(ui.ctx());
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    // Fill.
    let b = block(ui, FIELD_H);
    let (fc, fg) = widgets::swatch_colors(&doc, &i.fill, 1.0);
    let chip = sub(b, 4.0, 2.0, 17.0, 17.0);
    widgets::paint_chip(ui.painter(), chip, fc, fg);
    ui.painter().rect_stroke(chip, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    let resp = ui.interact(chip, ui.id().with("fillchip"), Sense::click()).on_hover_text(format!(
        "{}: {}",
        crate::i18n::tr(&app.ui.language, "Fill"),
        i.fill
    ));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Fill swatch"));
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
    let resp = ui.interact(chip, ui.id().with("strokechip"), Sense::click()).on_hover_text(format!(
        "{}: {}",
        crate::i18n::tr(&app.ui.language, "Stroke"),
        i.stroke
    ));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Stroke swatch"));
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
    let mresp = ui.interact(more, ui.id().with("opmore"), Sense::click());
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
fn swatch_menu(
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
    let to_text_id = egui::Id::new(("swatch_apply_to_text", stroke));
    let to_text = text_only || text_frame && resp.ctx.data(|d| d.get_temp::<bool>(to_text_id)).unwrap_or(false);
    // What the popup shows and changes: the frame's fill or stroke, or its text's.
    let (key, tint_key) = if stroke { ("stroke", "strokeTint") } else { ("fill", "fillTint") };
    let (cur, tint) = if to_text {
        let chars = super::text_attrs(app).map(|a| a["chars"].clone()).unwrap_or_default();
        (chars[key].as_str().unwrap_or("").to_string(), chars[tint_key].as_f64().unwrap_or(1.0))
    } else {
        (cur.to_string(), f64::from(super::sel_tint(app, stroke)))
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
        picked = super::swatch_list(ui, doc, &cur);
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

fn align_row(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let to = app.ui.align_to.clone();
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
        swatch_menu(app, &resp, &doc, &sw, k != 0, false, true);
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
                show_panel(app, "stroke");
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
    let mut behind = c["strokeBehindFill"].as_bool().unwrap_or(false);
    if ui.checkbox(&mut behind, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Stroke Behind Fill"))).changed() {
        let _ = app.run("type.char", json!({"attrs": {"strokeBehindFill": behind}}));
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
    let fonts = super::fonts(app);
    let menu = super::font_menu(app);
    let resp = widgets::dropdown(ui, "", fw);
    let r = resp.rect;
    icons::paint(ui.painter(), Rect::from_min_size(r.min + vec2(3.0, 3.0), vec2(14.0, 14.0)), "search", t.icon);
    widgets::chevron_down(ui.painter(), r.min + vec2(22.0, 10.5), t.icon);
    ui.painter().text(
        r.min + vec2(30.0, 10.5),
        egui::Align2::LEFT_CENTER,
        if fam.is_empty() { "—".to_string() } else { super::font_label(app, &menu, &fam) },
        egui::FontId::proportional(11.5),
        t.text,
    );
    if let Some(f) = super::font_popup(app, &resp, &menu, &fam) {
        super::apply_font(app, &f);
    }
    ui.add_space(1.0);
    let styles = fonts.styles(&fam);
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
    place(ui, sub(b, 24.0, 26.0, half - 24.0, FIELD_H), |ui| kerning_control(app, ui, "ckern", &c["kerning"], half - 24.0));
    icons::paint(ui.painter(), sub(b, half + 10.0, 27.0, 19.0, 19.0), "tracking", t.icon);
    const TRACK: &[f64] = &[-100.0, -75.0, -50.0, -25.0, -10.0, -5.0, 0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 200.0];
    if let Some(v) = place(ui, sub(b, half + 34.0, 26.0, lw, FIELD_H), |ui| {
        NumField::number("ctrack", c["tracking"].as_f64(), "", 0).width(lw).spinner().presets(TRACK).step(5.0).range(-1000.0, 10000.0).show(ui)
    }) {
        let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
    }
    // Language: hyphenation, spelling and typographer's quotes follow it.
    ui.add_space(1.0);
    let lang = c["language"].as_str().unwrap_or("English: USA").to_string();
    let label = crate::i18n::tr(&app.ui.language, "Language");
    let resp = widgets::dropdown(ui, crate::i18n::tr(&app.ui.language, &lang), fw).on_hover_ui(|ui| {
        crate::rtl::label(ui, label);
    });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, label));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(fw);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| language_menu(app, ui, &lang));
    });
    if more_options(ui, &app.ui.language).clicked() {
        show_panel(app, "character");
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
        show_panel(app, "paragraph");
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
                show_panel(app, "paragraph");
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
pub(crate) const LANGUAGES: &[&str] = &[
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
    "Georgian",
    "Turkish",
    "Japanese",
    "Chinese",
    "Chinese: Simplified",
    "Chinese: Traditional",
    "Korean",
    "Arabic",
    "Hebrew",
];

/// The Language list (Character panel, Properties ▸ Character): picking one sets the language of
/// the selected text.
fn language_menu(app: &mut DesignApp, ui: &mut Ui, cur: &str) {
    for l in LANGUAGES {
        if ui.selectable_label(*l == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
            let _ = app.run("type.char", json!({"attrs": {"language": l}}));
            ui.close();
        }
    }
}

/// Kerning for the text selection, or at a caret for the pair at the caret: the label shows
/// Metrics, Optical, 0 or the manual value; its menu sets one of the first three or a manual value
/// in 1/1000 em (typed or stepped).
fn kerning_control(app: &mut DesignApp, ui: &mut egui::Ui, id: &str, k: &Value, width: f32) {
    let kind = k["kind"].as_str().unwrap_or("metrics");
    let manual = k["value"].as_f64().filter(|v| kind == "manual" && v.is_finite());
    let label = match (kind, manual) {
        ("optical", _) => "Optical".to_string(),
        ("none", _) => "0".to_string(),
        ("manual", Some(v)) => format!("{v}"),
        _ => "Metrics".to_string(),
    };
    let resp = widgets::dropdown(ui, crate::i18n::tr(&app.ui.language, &label), width);
    let name = crate::i18n::tr(&app.ui.language, "Kerning");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, name));
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        for (text, v) in [("Metrics", json!({"kind": "metrics"})), ("Optical", json!({"kind": "optical"})), ("0", json!({"kind": "none"}))] {
            if ui.selectable_label(label == text, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, text))).clicked() {
                let _ = app.run("type.kernPair", json!({"kerning": v}));
                ui.close();
            }
        }
        ui.separator();
        // A manual value in 1/1000 em: after each selected character, or between the two at a caret.
        let field = format!("{id}_manual");
        if let Some(v) = NumField::number(&field, Some(manual.unwrap_or(0.0)), "", 0).width(90.0).spinner().step(5.0).range(-1000.0, 10000.0).show(ui)
        {
            let _ = app.run("type.kernPair", json!({"kerning": {"kind": "manual", "value": v}}));
        }
    });
}

pub fn character_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(a) = text_attrs(app) else {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Select text or a text frame."));
        return;
    };
    let c = a["chars"].clone();
    let fam = c["fontFamily"].as_str().unwrap_or("").to_string();
    let sty = c["fontStyle"].as_str().unwrap_or("").to_string();
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
            .show_ui(ui, |ui| language_menu(app, ui, &lang));
    });
    // Digits (World-Ready): how the figures 0-9 are drawn (Arabic 0123, Hindi ٠١٢٣, Farsi ۰۱۲۳).
    let digits = c["digits"].as_str().unwrap_or("default").to_string();
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Digits"));
        let cur = DIGIT_OPTIONS
            .iter()
            .find(|o| o.0 == digits)
            .map_or_else(|| digit_label(&app.ui.language, DIGIT_OPTIONS[0]), |o| digit_label(&app.ui.language, *o));
        egui::ComboBox::from_id_salt("char_digits").selected_text(crate::rtl::widget(ui, cur)).width(150.0).show_ui(ui, |ui| {
            for o in DIGIT_OPTIONS {
                if ui.selectable_label(o.0 == digits, crate::rtl::widget(ui, digit_label(&app.ui.language, o))).clicked() {
                    let _ = app.run("type.char", json!({"attrs": {"digits": o.0}}));
                }
            }
        });
    });
    // Two columns: Size, Kerning and Baseline on the left, Leading and Tracking on the right. A
    // column's captions share the width of its widest one and each field takes the rest of the
    // column, so they line up.
    let auto = c["leading"]["kind"].as_str() != Some("points");
    let lv = if auto { c["size"].as_f64().map(|s| s * a["para"]["autoLeading"].as_f64().unwrap_or(1.2)) } else { c["leading"]["value"].as_f64() };
    let (left_w, right_w) = (caption_width(app, ui, &["Size", "Kerning", "Baseline"]), caption_width(app, ui, &["Leading", "Tracking"]));
    ui.columns(2, |cols| {
        let [left, right] = cols else { return };
        if let Some(v) = char_field(app, left, "Size", left_w, NumField::number("pcs", c["size"].as_f64(), " pt", 2)) {
            let _ = app.run("type.char", json!({"attrs": {"size": v}}));
        }
        left.horizontal(|ui| {
            caption_cell(ui, crate::i18n::tr(&app.ui.language, "Kerning"), left_w);
            let width = (ui.available_width() - ui.spacing().item_spacing.x).max(40.0);
            kerning_control(app, ui, "pck", &c["kerning"], width);
        });
        if let Some(v) = char_field(app, left, "Baseline", left_w, NumField::number("pcb", c["baselineShift"].as_f64(), " pt", 2)) {
            let _ = app.run("type.char", json!({"attrs": {"baselineShift": v}}));
        }
        // Auto leading is shown in parentheses, as the Paragraph panel shows it.
        if let Some(v) = char_field(app, right, "Leading", right_w, NumField::number("pcl", lv, " pt", 2).parens(auto)) {
            let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
        }
        if let Some(v) = char_field(app, right, "Tracking", right_w, NumField::number("pct", c["tracking"].as_f64(), "", 0)) {
            let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
        }
    });
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Color"));
        super::swatch_picker(app, ui, "tfill", c["fill"].as_str().map(str::to_string), |app, n| {
            let _ = app.run("type.char", json!({"attrs": {"fill": n}}));
        });
        crate::rtl::label(ui, c["fill"].as_str().unwrap_or(""));
    });
    ui.horizontal(|ui| {
        // Bold and Italic pick the family's own styles (style linking).
        let (weight, italic) = super::fonts(app).traits_of(c["fontFamily"].as_str().unwrap_or_default(), c["fontStyle"].as_str().unwrap_or_default());
        let tip = |app: &DesignApp, name: &str, id: &str| {
            let name = crate::i18n::tr(&app.ui.language, name);
            match crate::menus::shortcut_of(app, id) {
                Some(sc) => format!("{name} ({})", crate::menus::shortcut_text(&sc)),
                None => name.to_string(),
            }
        };
        let (bold_tip, italic_tip) = (tip(app, "Bold", "type.bold"), tip(app, "Italic", "type.italic"));
        if ui
            .selectable_label(weight >= 650.0, egui::RichText::new("B").strong())
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, bold_tip);
            })
            .clicked()
        {
            let _ = app.run("type.bold", json!({}));
        }
        if ui
            .selectable_label(italic, egui::RichText::new("I").italics())
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, italic_tip);
            })
            .clicked()
        {
            let _ = app.run("type.italic", json!({}));
        }
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
        if ui
            .button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "OpenType")))
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Show the Typography panel"));
            })
            .clicked()
        {
            show_typography_panel(app);
        }
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

/// Opens the Typography panel, or focuses it when it is already showing. `window.panel` toggles a
/// flyout, so it is skipped while the Typography flyout is open.
pub fn show_typography_panel(app: &mut DesignApp) {
    if app.ui.open_panel.as_deref() == Some("typography") {
        return;
    }
    let _ = app.run("window.panel", json!({"panel": "typography"}));
}

pub fn paragraph_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(a) = text_attrs(app) else {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Select text or a text frame."));
        return;
    };
    let u = units(app);
    let p = a["para"].clone();
    ui.horizontal(|ui| {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Paragraph")).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::menus::menu_button(ui, "☰", |ui| {
                for (label, id) in [
                    ("Paragraph Rules…", "app.paragraphRulesDialog"),
                    ("Justification…", "app.justificationDialog"),
                    ("Paragraph Borders and Shading…", "app.paragraphBordersDialog"),
                    ("Span Columns…", "app.spanColumnsDialog"),
                    ("Bullets and Numbering…", "app.bulletsNumberingDialog"),
                ] {
                    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                        let _ = app.run(id, json!({}));
                        ui.close();
                    }
                }
            });
        });
    });
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
        // Drop cap: number of lines and characters.
        for (label, key, max, id) in [
            ("Drop Lines", "dropCapLines", designcraft_compose::MAX_DROP_CAP_LINES, "ppdl"),
            ("Drop Chars", "dropCapChars", designcraft_compose::MAX_DROP_CAP_CHARS, "ppdc"),
        ] {
            caption(ui, crate::i18n::tr(&app.ui.language, label));
            if let Some(v) = number(ui, id, p[key].as_f64(), "", 64.0, 0) {
                let _ = app.run("type.para", json!({"attrs": {key: (v.max(0.0) as u64).min(max as u64)}}));
            }
        }
        ui.end_row();
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
    let mut optical = p["opticalMargin"].as_bool().unwrap_or(false);
    if ui.checkbox(&mut optical, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Optical Margin Alignment"))).changed() {
        let _ = app.run("type.para", json!({"attrs": {"opticalMargin": optical}}));
    }
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Composer"));
        let (single, [paragraph, single_line]) = composer_choice(p["composer"].as_str());
        if ui.selectable_label(!single, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Paragraph"))).clicked() {
            let _ = app.run("type.para", json!({"attrs": {"composer": paragraph}}));
        }
        if ui.selectable_label(single, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Single-line"))).clicked() {
            let _ = app.run("type.para", json!({"attrs": {"composer": single_line}}));
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
    if super::cell_stroke::in_cells(app) {
        super::cell_stroke::panel(app, ui);
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
    let linked_id = egui::Id::new("wrap_offsets_linked");
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
        egui::ComboBox::from_id_salt(salt).selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &shown))).width(170.0).show_ui(
            ui,
            |ui| {
                for (k, l) in opts {
                    if ui.selectable_label(*k == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        let _ = app.run("object.textWrap", json!({ key: k }));
                    }
                }
            },
        );
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
fn pathfinder_row(app: &mut DesignApp, ui: &mut Ui) {
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

/// Links: status (missing / modified / embedded), page and effective resolution of every placed
/// graphic and linked text file; Relink, Go To Link, Update Link, Embed Link for the chosen row.
pub fn links_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    if let Some(st) = app.session.active() {
        let mut external = st.doc.settings.external_links;
        if ui
            .add_enabled(
                !cfg!(target_arch = "wasm32"),
                egui::Checkbox::new(&mut external, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Keep linked artwork external on save"))),
            )
            .changed()
            && let Err(error) = app.run("links.storage", json!({"mode": if external { "linked" } else { "embedded" }}))
        {
            app.status(error.to_string());
        }
        ui.separator();
    }
    // Linked stories (Place and Link).
    let stories = app.session.execute("story.links", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    if !stories.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Linked Stories")).strong());
        for l in &stories {
            let out = l["outOfDate"].as_bool().unwrap_or(false);
            ui.horizontal(|ui| {
                crate::rtl::label(
                    ui,
                    crate::i18n::tr(&app.ui.language, "Story {story} ← story {parent}")
                        .replace("{story}", &l["story"].to_string())
                        .replace("{parent}", &l["parent"].to_string()),
                );
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
    // Leave room for Status and Page/PPI instead of widening the grid for long source paths.
    let name_width = (ui.available_width() - 160.0).max(50.0);
    egui::Grid::new("links_grid").num_columns(3).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Name")).color(t.text_dim).size(11.0));
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Status")).color(t.text_dim).size(11.0));
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page · PPI")).color(t.text_dim).size(11.0));
        ui.end_row();
        for r in &rows {
            // Asset and story ids come from one counter: a row is chosen by either.
            let aid = r["asset"].as_u64().or_else(|| r["story"].as_u64()).unwrap_or(0);
            let name = r["name"].as_str().unwrap_or("");
            let response = ui
                .vertical(|ui| {
                    ui.set_width(name_width);
                    let name = ui.add(egui::Button::selectable(chosen == Some(aid), name).truncate());
                    match r["path"].as_str().filter(|path| !path.is_empty()) {
                        Some(path) => name.union(
                            ui.add(
                                egui::Label::new(egui::RichText::new(path).color(t.text_dim).size(11.0))
                                    .truncate()
                                    .sense(Sense::click())
                                    .show_tooltip_when_elided(false),
                            )
                            .on_hover_text(path),
                        ),
                        None => name,
                    }
                })
                .inner;
            if response.clicked() {
                chosen = Some(aid);
            }
            let (txt, col) = match r["status"].as_str().unwrap_or("") {
                "missing" => ("\u{26A0} Missing", egui::Color32::from_rgb(0xe5, 0x4b, 0x4b)),
                "modified" => ("\u{25B2} Modified", egui::Color32::from_rgb(0xe8, 0xb3, 0x2c)),
                "embedded" => ("Embedded", t.text_dim),
                "tooLarge" => ("Too large to check", egui::Color32::from_rgb(0xe8, 0xb3, 0x2c)),
                _ => ("OK", t.text_dim),
            };
            let status = crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, txt)).color(col).size(11.0));
            // Why a link isn't followed (a path on another computer, a device).
            if let Some(why) = r["refused"].as_str() {
                status.on_hover_text(why);
            }
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
    if let Some(path) = rows
        .iter()
        .find(|r| r["asset"].as_u64().or_else(|| r["story"].as_u64()) == chosen)
        .and_then(|r| r["path"].as_str())
        .filter(|path| !path.is_empty())
    {
        // A selected link stays distinguishable even if two rows truncate to the same prefix.
        ui.add(egui::Label::new(egui::RichText::new(path).color(t.text_dim).size(11.0)).wrap());
        ui.add_space(6.0);
    }
    ui.horizontal_wrapped(|ui| {
        let Some(aid) = chosen else {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Choose a link.")).color(t.text_dim).size(11.0));
            return;
        };
        // A linked text file is addressed by its story.
        let is_text = rows.iter().any(|r| r["story"].as_u64() == Some(aid));
        let target = if is_text { json!({"story": aid}) } else { json!({"asset": aid}) };
        let with = |k: &str, v: Value| {
            let mut p = target.clone();
            p[k] = v;
            p
        };
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Relink…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|pick| pick("relink"))
        {
            let _ = app.run("links.relink", with("path", json!(path)));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Go To"))).clicked() {
            let _ = app.run("links.goTo", target.clone());
        }
        let edited = rows.iter().any(|r| r["story"].as_u64() == Some(aid) && r["edited"].as_bool() == Some(true));
        let update = ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Update")));
        let update = if edited {
            update.on_hover_ui(|ui| {
                crate::rtl::label(
                    ui,
                    crate::i18n::tr(&app.ui.language, "This story was edited in the layout: you'll be asked before those edits are replaced"),
                );
            })
        } else {
            update
        };
        if update.clicked() {
            update_links(app, target.clone());
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, if is_text { "Unlink" } else { "Embed" }))).clicked() {
            let _ = app.run("links.embed", target.clone());
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
                app.status(crate::i18n::tr(&app.ui.language, "Relinked {count}").replace("{count}", &v["relinked"].to_string()));
            }
        }
    });
}

/// Update Link from the Links panel or the menu (`{asset}`, `{story}`, or `{}` for all): asks
/// first when a linked text story edited in the layout would lose those edits, as InDesign's
/// Links panel does. The `links.update` command itself doesn't ask (as in InDesign scripting).
pub fn update_links(app: &mut DesignApp, target: Value) {
    let rows = app.session.execute("links.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let one = target.get("story").and_then(Value::as_u64);
    let all = one.is_none() && target.get("asset").is_none();
    let edited: Vec<&str> = rows
        .iter()
        .filter(|r| (all || (one.is_some() && r["story"].as_u64() == one)) && r["edited"] == true && r["status"] == "modified")
        .filter_map(|r| r["name"].as_str())
        .collect();
    if edited.is_empty() {
        let _ = run_update_links(app, target);
    } else {
        let files = edited.join(", ");
        app.ui.dialog = Some(crate::dialogs::Dialog::new("updateLink", json!({"target": target, "files": files})));
    }
}

/// Run `links.update`; its first warning goes to the status bar.
pub(crate) fn run_update_links(app: &mut DesignApp, target: Value) -> Result<Value, String> {
    let r = app.run("links.update", target)?;
    if let Some(w) = r["warnings"].as_array().and_then(|w| w.first()).and_then(Value::as_str) {
        app.status(w.to_string());
    }
    Ok(r)
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
                        "{} · {} · {}",
                        crate::i18n::count_label(&app.ui.language, "pages", st.doc.page_count()),
                        crate::i18n::count_label(&app.ui.language, "stories", st.doc.stories.len()),
                        crate::i18n::count_label(&app.ui.language, "items", st.doc.all_items().len())
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
                    "{}, {}",
                    crate::i18n::count_label(&app.ui.language, "errors", errors),
                    crate::i18n::count_label(&app.ui.language, "warnings", issues.len() - errors)
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

/// Variable Font sliders: one per axis, each with a field for a typed value; the style becomes
/// `Named {tag:value,…}`. Dragging a slider previews on the page and is one undo step once let go;
/// a click, an arrow key or a typed value applies at once.
fn variable_font_axes(app: &mut DesignApp, ui: &mut egui::Ui, family: &str, style: &str) {
    if family.is_empty() {
        return;
    }
    let db = super::fonts(app);
    let axes = usable_axes(db.axes(family, style));
    if axes.is_empty() {
        return;
    }
    let base = designcraft_fonts::base_style(style).to_string();
    let face = db.face(family, style);
    let cur = |tag: &str, default: f32| face.coords.iter().find(|(t, _)| t == tag.as_bytes()).map_or(default, |c| c.1);
    let mut values: Vec<(String, f32)> = axes.iter().map(|a| (a.0.clone(), cur(&a.0, a.3))).collect();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Variable Font")).size(11.0));
    // What the sliders did this frame, gathered over every axis before anything is run.
    let (mut held, mut dragged, mut applied) = (false, false, false);
    let names: Vec<&str> = axes.iter().map(|a| a.1.as_str()).collect();
    let label_w = caption_width(app, ui, &names);
    for ((tag, name, min, _, max), value) in axes.iter().zip(values.iter_mut()) {
        // Caption, a slider filling the middle, then the value field: every slider is as long.
        ui.horizontal(|ui| {
            caption_cell(ui, name, label_w);
            ui.spacing_mut().slider_width = (ui.available_width() - PANEL_FIELD_W - ui.spacing().item_spacing.x * 2.0).max(40.0);
            let (step, decimals) = axis_step(*min, *max);
            // The field beside the slider takes typed values, so the slider's own is hidden.
            let slider = ui.add(egui::Slider::new(&mut value.1, *min..=*max).step_by(f64::from(step)).show_value(false));
            // Only the user's own change counts: the slider also reports one when it snaps a value
            // that is off its steps, which it would then do on every frame.
            let pressed = slider.is_pointer_button_down_on() || slider.dragged();
            let changed = slider.changed() && (pressed || slider.has_focus());
            held |= pressed;
            dragged |= changed && pressed;
            applied |= changed && !pressed;
            if let Some(v) = number(ui, &format!("vf{tag}"), Some(f64::from(value.1)), "", PANEL_FIELD_W, decimals).filter(|v| v.is_finite()) {
                value.1 = (v as f32).clamp(*min, *max);
                applied = true;
            }
        });
    }
    let attrs = json!({"attrs": {"fontStyle": variable_font_style(&base, &values)}});
    if applied {
        app.end_live_preview();
        let _ = app.run("type.char", attrs);
    } else if dragged {
        app.live_preview("Variable Font", "type.char", attrs);
    } else if held {
        app.hold_live_preview();
    }
    // Let go, the preview is committed at the start of the next frame (DesignApp::ui).
}

/// The slider step and the field's decimals for an axis: whole units, or hundredths for an axis
/// whose whole range is a few units (Skia's weight runs 0.48–3.2).
/// The axes a slider can show: a malformed font's axis whose minimum lies above its maximum (or
/// isn't a number) is left out, since no value fits its range.
fn usable_axes(axes: Vec<(String, String, f32, f32, f32)>) -> Vec<(String, String, f32, f32, f32)> {
    axes.into_iter().filter(|a| a.2 <= a.4).collect()
}

fn axis_step(min: f32, max: f32) -> (f32, usize) {
    if max - min >= 10.0 { (1.0, 0) } else { (0.01, 2) }
}

/// The style for the axis values: `base {tag:value,…}`, each value to at most two decimals.
fn variable_font_style(base: &str, values: &[(String, f32)]) -> String {
    let spec = values
        .iter()
        .map(|(t, v)| {
            let v = format!("{v:.2}");
            format!("{t}:{}", v.trim_end_matches('0').trim_end_matches('.'))
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{base} {{{spec}}}")
}

/// The width of the axis sliders' value fields, so the sliders end at the same place.
const PANEL_FIELD_W: f32 = 52.0;

/// The caption font of the Character panel's rows.
fn caption_font() -> egui::FontId {
    egui::FontId::proportional(11.5)
}

/// The width of the widest of `labels` in the interface language: a column's caption cells.
fn caption_width(app: &DesignApp, ui: &Ui, labels: &[&str]) -> f32 {
    let t = Tokens::get(ui.ctx());
    labels
        .iter()
        .map(|l| ui.painter().layout_no_wrap(crate::i18n::tr(&app.ui.language, l).to_string(), caption_font(), t.text).size().x)
        .fold(0.0, f32::max)
        .ceil()
}

/// A Character panel column row: a caption cell `label_w` wide, then the number field filling the
/// rest of the column. Returns the value committed.
fn char_field(app: &DesignApp, ui: &mut Ui, label: &str, label_w: f32, field: NumField<'_>) -> Option<f64> {
    ui.horizontal(|ui| {
        caption_cell(ui, crate::i18n::tr(&app.ui.language, label), label_w);
        let width = (ui.available_width() - ui.spacing().item_spacing.x).max(40.0);
        field.width(width).show(ui)
    })
    .inner
}

/// A left-aligned caption in a cell of fixed width, so the field after it lines up.
fn caption_cell(ui: &mut Ui, text: &str, width: f32) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(width, crate::widgets::FIELD_H), Sense::hover());
    crate::rtl::paint(ui.painter(), pos2(r.min.x, r.center().y), egui::Align2::LEFT_CENTER, text, caption_font(), t.text);
}

/// Whether `composer` (a paragraph's `composer` attribute) is a single-line composer, and the
/// paragraph and single-line composers of its kind (Japanese or not), for the composer choice.
pub(crate) fn composer_choice(composer: Option<&str>) -> (bool, [&'static str; 2]) {
    let single = matches!(composer, Some("singleLine" | "japaneseSingleLine"));
    let kind = if composer.is_some_and(|c| c.starts_with("japanese")) { ["japanese", "japaneseSingleLine"] } else { ["paragraph", "singleLine"] };
    (single, kind)
}

/// The Digits choices: (value, caption, sample of 0–3). The values are the file format's:
/// `arabic` is 0123, `hindi` the Arabic-Indic ٠١٢٣ and `farsi` the Persian/Urdu ۰۱۲۳; the captions
/// name them by the script's users.
pub const DIGIT_OPTIONS: [(&str, &str, &str); 5] =
    [("default", "Default", ""), ("arabic", "Arabic", "0123"), ("hindi", "Hindi", "٠١٢٣"), ("farsi", "Farsi", "۰۱۲۳"), ("native", "Native", "")];

/// A Digits choice as shown: its translated caption and a sample of the figures.
pub fn digit_label(lang: &str, o: (&str, &str, &str)) -> String {
    let name = crate::i18n::tr(lang, o.1);
    if o.2.is_empty() { name.to_string() } else { format!("{name}  {}", o.2) }
}

#[cfg(test)]
mod tests {
    use egui_kittest::{Harness, kittest::Queryable};

    use super::*;

    #[test]
    fn links_panel_can_choose_external_storage_and_undo_it() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let mut harness = Harness::builder().with_size(vec2(320.0, 300.0)).build_ui_state(|ui, app: &mut DesignApp| links_panel(app, ui), app);
        harness.get_by_label("Keep linked artwork external on save").click();
        harness.run_steps(2);
        assert!(harness.state().session.doc().unwrap().doc.settings.external_links);
        harness.state_mut().run("edit.undo", json!({})).unwrap();
        harness.run_steps(2);
        assert!(!harness.state().session.doc().unwrap().doc.settings.external_links);
    }

    fn language(app: &mut DesignApp) -> Value {
        text_attrs(app).unwrap()["chars"]["language"].clone()
    }

    fn linked_graphics(paths: &[Option<&str>]) -> (DesignApp, Vec<designcraft_doc::ItemId>, Vec<designcraft_doc::AssetId>) {
        use designcraft_doc::{Asset, AssetId, Content, Graphic, ItemId};
        use std::sync::Arc;
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"title": "Linked graphics"})).unwrap();
        let mut items = Vec::new();
        let mut assets = Vec::new();
        for (index, path) in paths.iter().enumerate() {
            let x = 72 + index * 100;
            let id = ItemId(app.run("frame.create", json!({"rect": [x, 72, x + 72, 144]})).unwrap()["id"].as_u64().unwrap());
            let doc = Arc::make_mut(&mut app.session.doc_mut().unwrap().doc);
            let aid = AssetId(doc.alloc());
            doc.assets.insert(
                aid,
                Arc::new(Asset {
                    id: aid,
                    name: "photo.svg".into(),
                    mime: "image/svg+xml".into(),
                    link: path.map(str::to_owned),
                    data: Arc::new(
                        br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#4488cc"/></svg>"##
                            .to_vec(),
                    ),
                    pixels: Some((32, 32)),
                    ..Default::default()
                }),
            );
            doc.item_mut(id).unwrap().content = Content::Graphic(Graphic {
                asset: aid,
                size: (72.0, 72.0),
                xf: designcraft_geom::Affine::IDENTITY,
                clip: None,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: Default::default(),
            });
            items.push(id);
            assets.push(aid);
        }
        (app, items, assets)
    }

    fn links_harness(app: DesignApp, width: f32) -> Harness<'static, DesignApp> {
        let mut ready = false;
        let mut h = Harness::builder().with_size(vec2(width, 600.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), "");
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                links_panel(app, ui);
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        h.run_steps(3);
        h
    }

    /// As InDesign's: Edit Page takes the Page tool to the page in view, and Properties sets that
    /// page alone; Back returns to the document.
    #[test]
    fn edit_page_shows_the_page_and_sets_its_size_alone() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 3, "facingPages": false})).unwrap();
        app.ui.dock_tab = "properties".into();
        let mut h = crate::test_window::open(app, vec2(1400.0, 900.0));
        crate::canvas::go_to_page(&mut h.state_mut().app, 1);
        h.run_steps(3);
        h.get_by_label("Edit Page").click();
        h.run_steps(3);
        let st = h.state().app.session.active().unwrap();
        assert_eq!((h.state().app.session.tool_id(), st.selection.page), ("page", Some(1)));
        assert!(h.query_by_label("Document Setup").is_none(), "no dialog");
        let _ = h.get_by_label("Page Size");
        let _ = h.get_by_label("Margins and Columns");
        h.get_by_label("Landscape").click();
        h.run_steps(3);
        let doc = &h.state().app.session.active().unwrap().doc;
        let size = |abs: usize| doc.page(abs).map(|p| (p.width, p.height)).unwrap();
        assert_eq!(size(1), (792.0, 612.0), "the page turns");
        assert_eq!((size(0), size(2)), ((612.0, 792.0), (612.0, 792.0)), "the others don't");
        h.get_by_label("Back").click();
        h.run_steps(3);
        let st = h.state().app.session.active().unwrap();
        assert_eq!((h.state().app.session.tool_id(), st.selection.page), ("selection", None));
        let _ = h.get_by_label("Edit Page");
    }

    #[test]
    fn links_panel_distinguishes_same_named_sources_and_keeps_link_actions() {
        let first = "/project/assets/studio-a/photo.svg";
        let second = "/project/assets/studio-b/photo.svg";
        let (app, items, assets) = linked_graphics(&[Some(first), Some(second), None]);
        let mut h = links_harness(app, 320.0);
        assert_eq!(h.get_all_by_label("photo.svg").count(), 3);
        assert!(h.query_by_label(first).is_some(), "the first source path is visible in the row");
        h.get_by_label(second).click();
        h.run_steps(3);
        h.get_by_label("Go To").click();
        h.run_steps(3);
        assert_eq!(h.state().session.active().unwrap().selection.items, vec![items[1]]);
        h.get_by_label("Embed").click();
        h.run_steps(3);
        assert!(h.state().session.active().unwrap().doc.assets[&assets[1]].link.is_none());
        assert_eq!(h.state().session.active().unwrap().doc.assets[&assets[0]].link.as_deref(), Some(first));
        assert!(h.query_by_label(second).is_none(), "an embedded graphic no longer shows a source path");
        assert_eq!(h.get_all_by_label("Embedded").count(), 2);
        assert_eq!(h.get_all_by_label("photo.svg").count(), 3);
        h.get_by_label(first).click();
        h.run_steps(3);
        h.get_by_label("Go To").click();
        h.run_steps(3);
        assert_eq!(h.state().session.active().unwrap().selection.items, vec![items[0]]);
    }

    #[test]
    fn links_panel_keeps_long_source_paths_inside_a_narrow_panel() {
        let path = "/project/assets/studio-a/very-long-folder-name/another-long-folder-name/photo.svg";
        let (app, _, _) = linked_graphics(&[Some(path)]);
        let mut h = links_harness(app, 240.0);
        let source = h.get_by_label(path).rect();
        assert!(source.width() < 150.0, "the source is truncated to its column: {source:?}");
        assert!(source.right() <= 240.0, "the source stays inside the narrow panel: {source:?}");
        assert!(h.get_by_label("Page · PPI").rect().right() <= 240.0);
        h.get_by_label(path).click();
        h.run_steps(3);
        assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(text) if text.galley.job.text == path && !text.galley.elided && text.visual_bounding_rect().right() <= 240.0)), "the selected path is painted in full and wraps inside the panel");
    }

    #[test]
    fn properties_character_section_sets_the_language() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "bunun", "caret": true})).unwrap();
        app.run("edit.selectAll", json!({})).unwrap();
        assert_ne!(language(&mut app), "French");
        let mut h = Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                // The panel's fonts are installed before its first frame.
                let id = egui::Id::new("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        h.get_by_label("Language").click();
        h.run_steps(2);
        h.get_by_label("French").click();
        h.run_steps(2);
        assert_eq!(language(h.state_mut()), "French");
    }

    fn kerning(app: &mut DesignApp) -> Value {
        text_attrs(app).unwrap()["chars"]["kerning"].clone()
    }

    /// A harness drawing `panel` for `app` (fonts installed before the first frame).
    fn panel_harness(app: DesignApp, panel: fn(&mut DesignApp, &mut egui::Ui)) -> Harness<'static, DesignApp> {
        Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                let id = egui::Id::new("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                panel(app, ui);
            },
            app,
        )
    }

    fn text_app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "AVATAR", "caret": true})).unwrap();
        app.run("edit.selectAll", json!({})).unwrap();
        app
    }

    #[test]
    fn properties_kerning_menu_sets_zero_kerning() {
        let mut h = panel_harness(text_app(), show);
        h.run_steps(3);
        assert_eq!(kerning(h.state_mut())["kind"], "metrics");
        h.get_by_label("Kerning").click();
        h.run_steps(2);
        h.get_by_label("0").click();
        h.run_steps(2);
        assert_eq!(kerning(h.state_mut())["kind"], "none");
    }

    fn undo_steps(app: &DesignApp) -> usize {
        app.session.active().unwrap().history.undo.len()
    }

    fn font_style(app: &mut DesignApp) -> String {
        text_attrs(app).unwrap()["chars"]["fontStyle"].as_str().unwrap().to_string()
    }

    /// The Digits menu uses the reference names (documented as Arabic, Hindi and Farsi), which
    /// are translated, with a sample of each script's figures.
    #[test]
    fn digits_choices_use_the_reference_names() {
        let shown: Vec<String> = DIGIT_OPTIONS.iter().map(|o| digit_label("fr", *o)).collect();
        assert_eq!(shown, ["Par défaut", "Arabe  0123", "Hindi  ٠١٢٣", "Persan  ۰۱۲۳", "Natif"].map(String::from));
    }

    #[test]
    fn axes_with_an_inverted_or_missing_range_get_no_slider() {
        let axis = |tag: &str, min: f32, max: f32| (tag.to_string(), tag.to_string(), min, min, max);
        let kept = usable_axes(vec![axis("wght", 100.0, 900.0), axis("wdth", 125.0, 75.0), axis("opsz", f32::NAN, 72.0), axis("slnt", 0.0, 0.0)]);
        let tags: Vec<&str> = kept.iter().map(|a| a.0.as_str()).collect();
        assert_eq!(tags, ["wght", "slnt"]);
    }

    #[test]
    fn variable_font_style_keeps_two_decimals_at_most() {
        let values = [("wght".to_string(), 512.0_f32), ("wdth".to_string(), 1.25), ("opsz".to_string(), 0.4801)];
        assert_eq!(variable_font_style("Regular", &values), "Regular {wght:512,wdth:1.25,opsz:0.48}");
        assert_eq!(axis_step(100.0, 900.0), (1.0, 0));
        assert_eq!(axis_step(0.48, 3.2), (0.01, 2));
    }

    #[test]
    fn a_live_preview_held_over_many_frames_is_one_undo_step_once_let_go() {
        let mut app = text_app();
        let steps = undo_steps(&app);
        let size = |app: &mut DesignApp| text_attrs(app).unwrap()["chars"]["size"].as_f64().unwrap();
        // A drag: a preview on most frames, held without moving on others.
        for v in [20.0, 24.0, 30.0] {
            app.settle_live_preview();
            app.live_preview("Size", "type.char", json!({"attrs": {"size": v}}));
            app.settle_live_preview();
            app.hold_live_preview();
        }
        assert_eq!(size(&mut app), 30.0, "the page shows the last preview");
        assert_eq!(undo_steps(&app), steps, "nothing is undoable while held");
        // Let go: a frame without a hold, then the frame after it commits the preview.
        app.settle_live_preview();
        assert_eq!(undo_steps(&app), steps, "the last held frame keeps it open");
        app.settle_live_preview();
        assert_eq!(undo_steps(&app), steps + 1);
        assert!(app.session.active().unwrap().interaction.is_none());
        app.settle_live_preview();
        assert_eq!(undo_steps(&app), steps + 1, "committed once");
        // Later edits get their own undo steps.
        app.run("type.char", json!({"attrs": {"tracking": 50}})).unwrap();
        assert_eq!(undo_steps(&app), steps + 2);
    }

    /// A system variable font with two or more axes (macOS ships some), loaded into the shared fonts.
    fn system_variable_font() -> Option<&'static str> {
        let candidates = [("/System/Library/Fonts/Supplemental/Skia.ttf", "Skia"), ("/System/Library/Fonts/NewYork.ttf", "New York")];
        candidates.into_iter().find_map(|(path, family)| {
            let bytes = std::fs::read(path).ok()?;
            let db = designcraft_fonts::FontDb::global();
            db.add_font(bytes);
            let style = db.styles(family).first().cloned()?;
            (db.axes(family, &style).len() >= 2).then_some(family)
        })
    }

    #[test]
    fn dragging_one_of_several_axis_sliders_is_one_undo_step() {
        let Some(family) = system_variable_font() else {
            eprintln!("skipped: no variable font with two axes on this system");
            return;
        };
        let mut app = text_app();
        super::super::apply_font(&mut app, &super::super::FontPick { family: family.into(), style: None });
        let before = font_style(&mut app);
        let steps = undo_steps(&app);
        // The app commits a preview no panel held at the start of each frame (DesignApp::ui).
        let mut h = Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::new("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                app.settle_live_preview();
                character_panel(app, ui);
            },
            app,
        );
        h.run_steps(3);
        let sliders = h.get_all_by_role(egui::accesskit::Role::Slider).map(|n| n.rect()).collect::<Vec<_>>();
        assert!(sliders.len() >= 2, "one slider per axis: {sliders:?}");
        let track = sliders[0];
        let y = track.center().y;
        h.hover_at(egui::pos2(track.left() + 2.0, y));
        h.run_steps(1);
        h.drag_at(egui::pos2(track.left() + 2.0, y));
        h.run_steps(1);
        for k in 1..=10 {
            h.hover_at(egui::pos2(track.left() + track.width() * k as f32 / 10.0, y));
            h.run_steps(1);
            assert_eq!(undo_steps(h.state()), steps, "no undo step while dragging (frame {k})");
        }
        let previewed = font_style(h.state_mut());
        assert_ne!(previewed, before, "the drag previews on the page");
        h.drop_at(egui::pos2(track.right(), y));
        h.run_steps(3);
        assert_eq!(undo_steps(h.state()), steps + 1, "the whole drag is one undo step");
        assert_eq!(font_style(h.state_mut()), previewed);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(font_style(h.state_mut()), before);
    }

    #[test]
    fn character_panel_has_kerning_and_takes_a_typed_manual_value() {
        let mut h = panel_harness(text_app(), character_panel);
        h.run_steps(3);
        // The caption and the control share the name: click the control.
        h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Kerning").click();
        h.run_steps(2);
        // The menu's field is drawn after the panel's own fields.
        let field = h.get_all_by_role(egui::accesskit::Role::TextInput).last().unwrap();
        field.focus();
        field.type_text("-40");
        h.run_steps(1);
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        assert_eq!(kerning(h.state_mut()), json!({"kind": "manual", "value": -40.0}));
    }

    #[test]
    fn a_manual_kerning_value_at_a_caret_kerns_the_pair() {
        let mut app = text_app();
        let story = app.session.active().unwrap().selection.text.unwrap().story;
        app.run("text.select", json!({"story": story.0, "anchor": 1, "focus": 1})).unwrap();
        let mut h = panel_harness(app, character_panel);
        h.run_steps(3);
        h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Kerning").click();
        h.run_steps(2);
        let field = h.get_all_by_role(egui::accesskit::Role::TextInput).last().unwrap();
        field.focus();
        field.type_text("-80");
        h.run_steps(1);
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        let st = h.state_mut().session.active().unwrap().doc.story(story).unwrap().format_after(0).over.kerning;
        assert_eq!(st, Some(designcraft_doc::Kerning::Manual(-80.0)), "A of the A|V pair");
        assert_eq!(kerning(h.state_mut()), json!({"kind": "manual", "value": -80.0}));
    }

    #[test]
    fn clicking_a_swatch_chip_in_the_appearance_fill_menu_applies_it() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        // A graphic frame: the Appearance section shows a single Fill chip.
        let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200]})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        let before = app.session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.swatch.clone();
        assert_ne!(before, "[Paper]");
        let mut h = Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::new("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        // Open the Fill menu from the Appearance chip.
        h.get_by_label("Fill swatch").click();
        h.run_steps(3);
        // Click the *chip* (the row's left edge), not the name.
        let name = "[Paper]".to_string();
        let row = h.get_by_label(&name).rect();
        // The row spans the menu (chip + name), not just the name text.
        assert!(row.width() > 150.0, "the row is the full menu width, chip included: {row:?}");
        let p = row.min + vec2(8.0, row.height() / 2.0);
        h.hover_at(p);
        h.drag_at(p);
        h.drop_at(p);
        h.run_steps(4);
        let after = h.state().session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.swatch.clone();
        assert_eq!(after, name, "clicking the swatch chip applies it");
    }

    fn panel(app: DesignApp) -> Harness<'static, DesignApp> {
        let mut h = Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::new("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        h
    }

    #[test]
    fn several_shapes_show_pathfinder_and_intersect_combines_them() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [100, 100, 300, 300], "shape": "ellipse"})).unwrap()["id"].clone();
        let b = app.run("frame.create", json!({"rect": [200, 100, 300, 200]})).unwrap()["id"].clone();
        // One shape: no Pathfinder section.
        app.run("selection.set", json!({"ids": [a]})).unwrap();
        let mut h = panel(app);
        assert!(h.query_by_label("Pathfinder").is_none());
        // Two: the section and its five buttons; Intersect leaves one shape, the ellipse's quarter.
        h.state_mut().run("selection.set", json!({"ids": [a, b]})).unwrap();
        h.run_steps(3);
        assert!(h.query_by_label("Pathfinder").is_some());
        for op in ["Add", "Subtract", "Intersect", "Exclude Overlap", "Minus Back"] {
            assert!(h.query_by_label(op).is_some(), "{op}");
        }
        h.get_by_label("Intersect").click();
        h.run_steps(3);
        let doc = &h.state().session.active().unwrap().doc;
        let items = &doc.spreads[0].items;
        assert_eq!(items.len(), 1, "one shape left");
        let r = items[0].bounds();
        assert!((r.x0 - 200.0).abs() < 0.5 && (r.y1 - 200.0).abs() < 0.5 && (r.x1 - 300.0).abs() < 0.5, "{r:?}");
    }

    const CYAN: &str = "C=100 M=0 Y=0 K=0";

    #[test]
    fn the_fill_menu_sets_a_tint() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200]})).unwrap()["id"].as_u64().unwrap();
        app.run("object.fill", json!({"swatch": CYAN, "ids": [id]})).unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        let mut h = panel(app);
        h.get_by_label("Fill swatch").click();
        h.run_steps(3);
        // A frame that isn't a text frame: no Apply To.
        assert!(h.query_by_label("Apply To:").is_none());
        // The field on the Tint row (Opacity also reads 100%).
        let row = h.get_by_label("Tint:").rect();
        h.query_all_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("100%"))
            .find(|n| (n.rect().center().y - row.center().y).abs() < 8.0)
            .expect("the Tint field")
            .click();
        h.run_steps(2);
        h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.event(egui::Event::Text("25".into()));
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        let fill = h.state().session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.clone();
        assert_eq!((fill.swatch.as_str(), fill.tint), (CYAN, 0.25), "the tint, on the same swatch");
    }

    #[test]
    fn a_tint_on_several_objects_keeps_each_ones_swatch() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        let b = app.run("frame.create", json!({"rect": [220, 72, 300, 200]})).unwrap()["id"].as_u64().unwrap();
        app.run("object.fill", json!({"swatch": CYAN, "ids": [a]})).unwrap();
        app.run("object.fill", json!({"swatch": "[Black]", "ids": [b]})).unwrap();
        app.run("selection.set", json!({"ids": [a, b]})).unwrap();
        let mut h = panel(app);
        h.get_by_label("Fill swatch").click();
        h.run_steps(3);
        let row = h.get_by_label("Tint:").rect();
        h.query_all_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("100%"))
            .find(|n| (n.rect().center().y - row.center().y).abs() < 8.0)
            .expect("the Tint field")
            .click();
        h.run_steps(2);
        h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.event(egui::Event::Text("25".into()));
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        let doc = &h.state().session.active().unwrap().doc;
        let fill = |id: u64| doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.clone();
        assert_eq!((fill(a).swatch.as_str(), fill(a).tint), (CYAN, 0.25));
        assert_eq!((fill(b).swatch.as_str(), fill(b).tint), ("[Black]", 0.25), "the second keeps its own swatch");
    }

    #[test]
    fn apply_to_text_colours_a_text_frames_text_not_the_frame() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Heading"})).unwrap();
        let (id, story) = (r["id"].as_u64().unwrap(), designcraft_doc::StoryId(r["story"].as_u64().unwrap()));
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        let mut h = panel(app);
        h.get_by_label("Fill swatch").click();
        h.run_steps(3);
        h.get_by_label("Text").click();
        h.run_steps(3);
        h.get_by_label(CYAN).click();
        h.run_steps(3);
        let doc = &h.state().session.active().unwrap().doc;
        assert_eq!(doc.story(story).unwrap().char_format_at(0).over.fill.as_deref(), Some(CYAN), "the text is cyan");
        assert_eq!(doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.swatch, "[None]", "the frame isn't");
    }

    #[test]
    fn transform_flips_and_rotates_from_properties() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200]})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        let mut h = panel(app);
        let xf = |h: &Harness<'_, DesignApp>| h.state().session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().xf;
        // Flip Horizontal mirrors the frame (a negative determinant); Flip Vertical as well brings it
        // back to a half turn, as in the tutorial's petal.
        h.get_by_label("Flip Horizontal").click();
        h.run_steps(3);
        assert!(xf(&h).determinant() < 0.0, "{:?}", xf(&h));
        h.get_by_label("Flip Vertical").click();
        h.run_steps(3);
        let c = xf(&h).as_coeffs();
        assert!((c[0] + 1.0).abs() < 1e-9 && (c[3] + 1.0).abs() < 1e-9, "a half turn: {c:?}");
        let info = h.state_mut().session.execute("transform.info", &json!({})).unwrap();
        assert!((info["rotation"].as_f64().unwrap().abs() - 180.0).abs() < 1e-6, "{info}");
        // A quarter turn clockwise from there.
        h.get_by_label("Rotate 90° Clockwise").click();
        h.run_steps(3);
        let info = h.state_mut().session.execute("transform.info", &json!({})).unwrap();
        assert!((info["rotation"].as_f64().unwrap() - 90.0).abs() < 1e-6, "{info}");
        // The angle and scale fields are there.
        assert!(h.query_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("90°")).is_some());
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

// ---------------------------------------------------------------- Flex Layout

/// (justification, icon, label) in the Justification menu's order.
const FLEX_JUSTIFY: [(&str, &str, &str); 6] = [
    ("flexStart", "flex-start", "Flex Start"),
    ("center", "flex-center", "Center"),
    ("flexEnd", "flex-end", "Flex End"),
    ("spaceBetween", "flex-between", "Space Between"),
    ("spaceAround", "flex-around", "Space Around"),
    ("spaceEvenly", "flex-evenly", "Space Evenly"),
];

/// The selected Flex Layout container (the first), with its objects' bounds.
fn selected_container(app: &DesignApp) -> Option<(designcraft_doc::Item, Vec<designcraft_geom::Rect>)> {
    let st = app.session.active()?;
    let it = st.selection.items.iter().filter_map(|i| st.doc.item(*i)).find(|it| it.flex.is_some())?.clone();
    let kids = match &it.content {
        designcraft_doc::Content::Group { items } => items.iter().map(|c| c.bounds()).collect(),
        _ => Vec::new(),
    };
    Some((it, kids))
}

/// The alignment grid: 3 × 3 cells (one column or row of three when a space justification spreads
/// the objects along the main axis); the chosen cell shows the direction's arrow.
fn flex_grid(ui: &mut Ui, r: Rect, f: Option<&designcraft_doc::FlexLayout>) -> Option<u8> {
    use designcraft_doc::{FlexAlign, FlexJustify};
    let t = Tokens::get(ui.ctx());
    let resp = ui.interact(r, ui.id().with("flex_grid"), Sense::click());
    ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    let cell = r.width() / 3.0;
    let pos = |a: FlexAlign| match a {
        FlexAlign::Start => 0,
        FlexAlign::Center => 1,
        FlexAlign::End => 2,
    };
    // The chosen (column, row), and whether a space justification leaves only one axis.
    let (chosen, spread_row) = match f {
        Some(f) => {
            let main = match f.justify {
                FlexJustify::Center => 1,
                FlexJustify::FlexEnd => 2,
                _ => 0,
            };
            let main = if f.direction.is_reverse() { 2 - main } else { main };
            let cross = pos(f.align);
            (if f.direction.is_row() { (main, cross) } else { (cross, main) }, f.justify.distributes().then_some(f.direction.is_row()))
        }
        None => ((0, 0), None),
    };
    let mut picked = None;
    for row in 0..3usize {
        for col in 0..3usize {
            match spread_row {
                Some(true) if col != 0 => continue,
                Some(false) if row != 0 => continue,
                _ => {}
            }
            let c = Rect::from_min_size(r.min + vec2(col as f32 * cell, row as f32 * cell), vec2(cell, cell));
            let on = match spread_row {
                Some(true) => row == chosen.1,
                Some(false) => col == chosen.0,
                None => (col, row) == chosen,
            };
            if on && f.is_some() {
                ui.painter().rect_filled(c.shrink(2.0), 2.0, t.well);
                let icon = match f.map(|f| f.direction) {
                    Some(designcraft_doc::FlexDirection::RowReverse) => "flex-left",
                    Some(designcraft_doc::FlexDirection::Column) => "flex-down",
                    Some(designcraft_doc::FlexDirection::ColumnReverse) => "flex-up",
                    _ => "flex-right",
                };
                icons::paint(ui.painter(), c.shrink(3.0), icon, t.text_strong);
            } else {
                ui.painter().circle_filled(c.center(), 1.5, t.icon);
            }
            if resp.clicked() && resp.interact_pointer_pos().is_some_and(|p| c.contains(p)) {
                picked = Some((row * 3 + col) as u8);
            }
        }
    }
    picked
}

/// Flex Layout controls, as InDesign's panel: Create Flex Layout; alignment grid, width and height
/// (Auto/Fixed); justification, direction and wrap; padding and spacing.
fn flex_section(app: &mut DesignApp, ui: &mut Ui) {
    use designcraft_doc::{FlexSize, flex};
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let u = units(app);
    let has_sel = app.session.active().is_some_and(|s| !s.selection.items.is_empty());
    let container = selected_container(app);
    let f = container.as_ref().and_then(|(it, _)| it.flex.clone());
    let mut on = f.is_some();
    let resp = ui.add_enabled_ui(has_sel, |ui| widgets::checkbox(ui, &mut on, &tr("Create Flex Layout"))).inner;
    if resp.changed() {
        let _ = app.run(if on { "flex.add" } else { "flex.remove" }, json!({}));
        return;
    }
    ui.add_space(4.0);
    let enabled = f.is_some();
    let rect = container.as_ref().map(|(it, _)| it.inner_bounds());
    let arranged = container.as_ref().and_then(|(it, kids)| it.flex.as_ref().map(|f| flex::arrange(f, it.inner_bounds(), kids)));
    let mut set: Option<Value> = None;
    // Alignment grid, W and H with Auto/Fixed, constrain proportions.
    let b = block(ui, 2.0 * FIELD_H + 5.0);
    ui.add_enabled_ui(enabled, |ui| {
        if let Some(c) = flex_grid(ui, sub(b, 0.0, 0.0, 48.0, 48.0), f.as_ref()) {
            set = Some(json!({"cell": c}));
        }
        let linked = temp_bool(ui, "flex_size_linked", false);
        for (k, (label, key, mode, len)) in [
            ("W:", "width", f.as_ref().map(|f| f.width), rect.map(|r| r.width())),
            ("H:", "height", f.as_ref().map(|f| f.height), rect.map(|r| r.height())),
        ]
        .into_iter()
        .enumerate()
        {
            let y = k as f32 * (FIELD_H + 5.0);
            ui.painter().text(
                sub(b, 56.0, y, 18.0, FIELD_H).right_center(),
                egui::Align2::RIGHT_CENTER,
                tr(label),
                egui::FontId::proportional(11.5),
                t.text,
            );
            let nv = place(ui, sub(b, 78.0, y, 66.0, FIELD_H), |ui| NumField::measure(key, len, u).width(66.0).range(0.0, 100_000.0).show(ui));
            if let Some(v) = nv {
                let mut p = json!({ key: v });
                if linked
                    && let (Some(r), Some(len)) = (rect, len)
                    && len > 1e-9
                {
                    let other = if key == "width" { ("height", r.height()) } else { ("width", r.width()) };
                    p[other.0] = json!(other.1 * v / len);
                }
                set = Some(p);
            }
            let names = [tr("Fixed"), tr("Auto")];
            let cur = if mode == Some(FlexSize::Auto) { 1 } else { 0 };
            if let Some(i) = place(ui, sub(b, 148.0, y, 62.0, FIELD_H), |ui| widgets::dropdown_list(ui, &names[cur], 62.0, &names, Some(cur))) {
                set = Some(json!({ key: if i == 1 { "auto" } else { "fixed" } }));
            }
        }
        let clicked = place(ui, sub(b, 214.0, 12.5, 22.0, FIELD_H), |ui| {
            widgets::icon_toggle_sized(ui, if linked { "link" } else { "link-broken" }, linked, &tr("Constrain proportions"), vec2(22.0, 21.0))
                .clicked()
        });
        if clicked {
            set_temp_bool(ui, "flex_size_linked", !linked);
        }
    });
    ui.add_space(6.0);
    // Justification, direction, wrap.
    let b = block(ui, FIELD_H);
    ui.add_enabled_ui(enabled, |ui| {
        let cur = f.as_ref().map(|f| f.justify).unwrap_or_default();
        let cur = serde_json::to_value(cur).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let icon = FLEX_JUSTIFY.iter().find(|j| j.0 == cur).map(|j| j.1).unwrap_or("flex-start");
        let resp = place(ui, sub(b, 0.0, 0.0, 58.0, FIELD_H), |ui| {
            preview_dropdown(ui, 58.0, |p, r| icons::paint(p, r.shrink2(vec2(6.0, 2.0)), icon, egui::Color32::from_gray(60)))
        });
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(150.0);
            for (id, icon, label) in FLEX_JUSTIFY {
                let r = ui.horizontal(|ui| {
                    let (ir, _) = ui.allocate_exact_size(vec2(22.0, 16.0), Sense::hover());
                    icons::paint(ui.painter(), ir, icon, t.icon);
                    ui.selectable_label(id == cur, crate::rtl::widget(ui, tr(label)))
                });
                if r.inner.clicked() {
                    set = Some(json!({"justify": id}));
                    ui.close();
                }
            }
        });
        let dir = f.as_ref().map(|f| f.direction).unwrap_or_default();
        place(ui, sub(b, 78.0, 0.0, 132.0, FIELD_H), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (d, icon, tip) in [
                (designcraft_doc::FlexDirection::Row, "flex-right", "Row"),
                (designcraft_doc::FlexDirection::RowReverse, "flex-left", "Reverse Row"),
                (designcraft_doc::FlexDirection::Column, "flex-down", "Column"),
                (designcraft_doc::FlexDirection::ColumnReverse, "flex-up", "Reverse Column"),
            ] {
                if widgets::icon_toggle_sized(ui, icon, enabled && dir == d, &tr(tip), vec2(31.0, 21.0)).clicked() {
                    set = Some(json!({"direction": d}));
                }
            }
        });
        let wrap = f.as_ref().is_some_and(|f| f.wrap);
        if place(ui, sub(b, 214.0, 0.0, 22.0, FIELD_H), |ui| {
            widgets::icon_toggle_sized(ui, "flex-wrap", wrap, &tr("Wrap Content"), vec2(22.0, 21.0)).clicked()
        }) {
            set = Some(json!({"wrap": !wrap}));
        }
    });
    // Spacing: padding (linked by default) and the spacing between objects.
    section(ui, &tr("Spacing"), true);
    let b = block(ui, 3.0 * FIELD_H + 10.0);
    ui.add_enabled_ui(enabled, |ui| {
        let pad = f.as_ref().map(|f| f.padding).unwrap_or_default();
        let linked = temp_bool(ui, "flex_padding_linked", true);
        for (k, (icon, tip, x, y)) in [
            ("margin-top", "Top", 0.0, 0.0),
            ("margin-inside", "Left", 112.0, 0.0),
            ("margin-bottom", "Bottom", 0.0, 26.0),
            ("margin-outside", "Right", 112.0, 26.0),
        ]
        .into_iter()
        .enumerate()
        {
            let ir = sub(b, x, y + 2.0, 17.0, 17.0);
            icons::paint(ui.painter(), ir, icon, t.icon);
            ui.interact(ir, ui.id().with(("flex_pad", k)), Sense::hover()).on_hover_ui(|ui| {
                crate::rtl::label(ui, tr(tip));
            });
            let value = f.as_ref().map(|_| pad[k]);
            let nv = place(ui, sub(b, x + 20.0, y, 76.0, FIELD_H), |ui| {
                NumField::measure(tip, value, u).width(76.0).spinner().range(0.0, 8000.0).show(ui)
            });
            if let Some(v) = nv {
                let p = if linked {
                    [v; 4]
                } else {
                    let mut p = pad;
                    p[k] = v;
                    p
                };
                set = Some(json!({"padding": p}));
            }
        }
        if place(ui, sub(b, 214.0, 12.5, 22.0, FIELD_H), |ui| {
            widgets::icon_toggle_sized(ui, if linked { "link" } else { "link-broken" }, linked, &tr("Make all settings the same"), vec2(22.0, 21.0))
                .clicked()
        }) {
            set_temp_bool(ui, "flex_padding_linked", !linked);
        }
        // Spacing between objects: a space justification works the main axis' out (shown greyed);
        // the cross axis' only counts when objects wrap.
        let (row, distributes, wrap) = f.as_ref().map(|f| (f.direction.is_row(), f.justify.distributes(), f.wrap)).unwrap_or((true, false, false));
        let main_gap = arranged.as_ref().map(|a| a.main_gap);
        for (k, (icon, key, tip, x)) in
            [("gap-h", "gapH", "Horizontal Spacing", 0.0), ("gap-v", "gapV", "Vertical Spacing", 112.0)].into_iter().enumerate()
        {
            let main = (k == 0) == row;
            let live = if main { !distributes } else { wrap };
            let value = match &f {
                Some(_) if main && distributes => main_gap,
                Some(f) => Some(if k == 0 { f.gap_h } else { f.gap_v }),
                None => None,
            };
            let y = 2.0 * FIELD_H + 10.0;
            let ir = sub(b, x, y + 2.0, 17.0, 17.0);
            icons::paint(ui.painter(), ir, icon, if live { t.icon } else { t.text_dim });
            ui.interact(ir, ui.id().with(("flex_gap", k)), Sense::hover()).on_hover_ui(|ui| {
                crate::rtl::label(ui, tr(tip));
            });
            let nv = place(ui, sub(b, x + 20.0, y, 76.0, FIELD_H), |ui| {
                NumField::measure(key, value, u).width(76.0).spinner().range(0.0, 8000.0).enabled(live).show(ui)
            });
            if let Some(v) = nv {
                set = Some(json!({ key: v }));
            }
        }
    });
    if let Some(p) = set {
        let _ = app.run("flex.set", p);
    }
}

/// Window › Flex Layout.
pub fn flex_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing = vec2(6.0, 5.0);
    flex_section(app, ui);
}
