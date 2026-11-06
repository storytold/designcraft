use super::object::preview_dropdown;
use super::{
    DesignApp, FIELD_H, NumField, Rect, Sense, Stroke, StrokeKind, Tokens, Ui, Value, block, icons, json, place, section, set_temp_bool, sub,
    temp_bool, units, vec2, widgets,
};

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

/// The chosen (column, row) of the Flex Layout alignment grid, and whether a space justification
/// leaves only one axis (`Some(true)` for a row layout).
fn flex_grid_choice(f: Option<&designcraft_doc::FlexLayout>) -> ((usize, usize), Option<bool>) {
    use designcraft_doc::{FlexAlign, FlexJustify};
    let pos = |a: FlexAlign| match a {
        FlexAlign::Start => 0,
        FlexAlign::Center => 1,
        FlexAlign::End => 2,
    };
    match f {
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
    }
}

/// The alignment grid: 3 × 3 cells (one column or row of three when a space justification spreads
/// the objects along the main axis); the chosen cell shows the direction's arrow.
fn flex_grid(ui: &mut Ui, r: Rect, f: Option<&designcraft_doc::FlexLayout>) -> Option<u8> {
    let t = Tokens::get(ui.ctx());
    let resp = ui.interact(r, ui.scope_id().with("flex_grid"), Sense::click());
    ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    let cell = r.width() / 3.0;
    let (chosen, spread_row) = flex_grid_choice(f);
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
pub(super) fn flex_section(app: &mut DesignApp, ui: &mut Ui) {
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
            preview_dropdown(ui, 58.0, &tr("Justification"), |p, r| icons::paint(p, r.shrink2(vec2(6.0, 2.0)), icon, egui::Color32::from_gray(60)))
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
            ui.interact(ir, ui.scope_id().with(("flex_pad", k)), Sense::hover()).on_hover_ui(|ui| {
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
            ui.interact(ir, ui.scope_id().with(("flex_gap", k)), Sense::hover()).on_hover_ui(|ui| {
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
