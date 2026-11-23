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

/// The direction buttons: direction, icon and name.
const FLEX_DIRECTIONS: [(designcraft_doc::FlexDirection, &str, &str); 4] = [
    (designcraft_doc::FlexDirection::Row, "flex-right", "Row"),
    (designcraft_doc::FlexDirection::RowReverse, "flex-left", "Reverse Row"),
    (designcraft_doc::FlexDirection::Column, "flex-down", "Column"),
    (designcraft_doc::FlexDirection::ColumnReverse, "flex-up", "Reverse Column"),
];

/// The padding fields: icon, name, and where the icon sits in the block; in the order of `FlexLayout::padding`.
const FLEX_PADDING: [(&str, &str, f32, f32); 4] = [
    ("margin-top", "Top", 0.0, 0.0),
    ("margin-inside", "Left", 112.0, 0.0),
    ("margin-bottom", "Bottom", 0.0, 26.0),
    ("margin-outside", "Right", 112.0, 26.0),
];

/// The spacing fields: icon, attribute, name and x.
const FLEX_GAPS: [(&str, &str, &str, f32); 2] = [("gap-h", "gapH", "Horizontal Spacing", 0.0), ("gap-v", "gapV", "Vertical Spacing", 112.0)];

/// The icon of the alignment grid's chosen cell: the direction's arrow.
fn flex_arrow(direction: Option<designcraft_doc::FlexDirection>) -> &'static str {
    match direction {
        Some(designcraft_doc::FlexDirection::RowReverse) => "flex-left",
        Some(designcraft_doc::FlexDirection::Column) => "flex-down",
        Some(designcraft_doc::FlexDirection::ColumnReverse) => "flex-up",
        _ => "flex-right",
    }
}

/// Whether the alignment grid shows the cell (`col`, `row`), and whether it is the chosen one.
fn flex_cell(f: Option<&designcraft_doc::FlexLayout>, col: usize, row: usize) -> Option<bool> {
    let (chosen, spread_row) = flex_grid_choice(f);
    match spread_row {
        Some(true) if col != 0 => None,
        Some(false) if row != 0 => None,
        Some(true) => Some(row == chosen.1),
        Some(false) => Some(col == chosen.0),
        None => Some((col, row) == chosen),
    }
}

/// The `flex.set` an edit of the width or height `key` to `v` makes: with the proportions constrained, the other
/// side follows.
fn flex_size_set(key: &str, v: f64, linked: bool, rect: Option<designcraft_geom::Rect>, len: Option<f64>) -> Value {
    let mut p = json!({ key: v });
    if linked
        && let (Some(r), Some(len)) = (rect, len)
        && len > 1e-9
    {
        let other = if key == "width" { ("height", r.height()) } else { ("width", r.width()) };
        p[other.0] = json!(other.1 * v / len);
    }
    p
}

/// The padding an edit of the side `k` to `v` sets: all four while linked.
fn flex_padding_set(pad: [f64; 4], linked: bool, k: usize, v: f64) -> [f64; 4] {
    if linked {
        [v; 4]
    } else {
        let mut p = pad;
        p[k] = v;
        p
    }
}

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
    let (_, spread_row) = flex_grid_choice(f);
    let mut picked = None;
    for row in 0..3usize {
        for col in 0..3usize {
            match spread_row {
                Some(true) if col != 0 => continue,
                Some(false) if row != 0 => continue,
                _ => {}
            }
            let c = Rect::from_min_size(r.min + vec2(col as f32 * cell, row as f32 * cell), vec2(cell, cell));
            let on = flex_cell(f, col, row).unwrap_or(false);
            if on && f.is_some() {
                ui.painter().rect_filled(c.shrink(2.0), 2.0, t.well);
                let icon = flex_arrow(f.map(|f| f.direction));
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

// FLEX_CREATE_DIRECTION_WRAP_APPLICATION_BEGIN
fn flex_create_choice(app: &DesignApp, ui: &mut Ui, on: &mut bool, label: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
        let owner = app.session.active().map(|st| (st.uid, st.identity_epoch, &st.selection.items));
        let id = ui.make_persistent_id(("flex-create", owner));
        let out = craft_ui_widgets::choice::Checkbox::new(Bind::given(*on)).id(id).label(label.to_owned()).show(ui);
        if let Some(after) = out.events.committed() {
            *on = *after;
            return true;
        }
        return false;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = app;
    widgets::checkbox(ui, on, label).changed()
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(craft_ui::contract::MemoProps)]
struct FlexChoiceGlyph(String);

#[cfg(not(target_arch = "wasm32"))]
impl craft_ui_widgets::primitives::Glyph for FlexChoiceGlyph {
    fn paint(&self, painter: &egui::Painter, _tokens: &craft_ui_widgets::style::UiTokens, rect: egui::Rect, color: egui::Color32, _mirrored: bool) {
        // These are physical layout directions, just as in the original app artwork.
        icons::paint(painter, rect, &self.0, color);
    }
}

fn flex_icon_choice(app: &DesignApp, ui: &mut Ui, key: &str, icon: &str, on: bool, label: &str, size: egui::Vec2) -> Option<bool> {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
        use craft_ui_widgets::primitives::{IconSpec, Mirror};
        // Keep focus through authoritative direction/wrap updates. Actual selection and document
        // epochs retire old input; value/revision changes are not a different physical control.
        let owner = app.session.active().map(|st| (st.uid, st.identity_epoch, &st.selection.items));
        let id = ui.make_persistent_id(("flex-icon-choice", key, owner));
        let glyph = FlexChoiceGlyph(icon.to_owned());
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        return craft_ui_widgets::buttons::ToggleButton::new(label.to_owned(), Bind::given(on))
            .id(id)
            .icon(IconSpec::glyph(&glyph).mirror(Mirror::Never))
            .icon_only()
            .tooltip(label.to_owned())
            .size(craft_theme::ControlSize::Sm)
            .show_at(ui, rect)
            .events
            .committed()
            .copied();
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, key);
    widgets::icon_toggle_sized(ui, icon, on, label, size).clicked().then_some(!on)
}
// FLEX_CREATE_DIRECTION_WRAP_APPLICATION_END

/// Flex Layout controls, as InDesign's panel: Create Flex Layout; alignment grid, width and height
/// (Auto/Fixed); justification, direction and wrap; padding and spacing.
pub(super) fn flex_section(app: &mut DesignApp, ui: &mut Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::flex_section(app, ui);
    }
    use designcraft_doc::{FlexSize, flex};
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let u = units(app);
    let has_sel = app.session.active().is_some_and(|s| !s.selection.items.is_empty());
    let container = selected_container(app);
    let f = container.as_ref().and_then(|(it, _)| it.flex.clone());
    let mut on = f.is_some();
    let changed = ui.add_enabled_ui(has_sel, |ui| flex_create_choice(app, ui, &mut on, &tr("Create Flex Layout"))).inner;
    if changed {
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
                set = Some(flex_size_set(key, v, linked, rect, len));
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
            for (d, icon, tip) in FLEX_DIRECTIONS {
                if let Some(after) = flex_icon_choice(app, ui, tip, icon, enabled && dir == d, &tr(tip), vec2(31.0, 21.0))
                    // The four choices are exclusive. Activating the current one retains its
                    // direction, like the original action, rather than turning the enum off.
                    && (after || dir == d)
                {
                    set = Some(json!({"direction": d}));
                }
            }
        });
        let wrap = f.as_ref().is_some_and(|f| f.wrap);
        if let Some(after) = place(ui, sub(b, 214.0, 0.0, 22.0, FIELD_H), |ui| {
            flex_icon_choice(app, ui, "wrap", "flex-wrap", wrap, &tr("Wrap Content"), vec2(22.0, 21.0))
        }) {
            set = Some(json!({"wrap": after}));
        }
    });
    // Spacing: padding (linked by default) and the spacing between objects.
    section(ui, &tr("Spacing"), true);
    let b = block(ui, 3.0 * FIELD_H + 10.0);
    ui.add_enabled_ui(enabled, |ui| {
        let pad = f.as_ref().map(|f| f.padding).unwrap_or_default();
        let linked = temp_bool(ui, "flex_padding_linked", true);
        for (k, (icon, tip, x, y)) in FLEX_PADDING.into_iter().enumerate() {
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
                set = Some(json!({"padding": flex_padding_set(pad, linked, k, v)}));
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
        for (k, (icon, key, tip, x)) in FLEX_GAPS.into_iter().enumerate() {
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

/// The Flex Layout section on the craft UI framework: the same rows over the same `flex` commands as the native body
/// above, every control a framework component. Create Flex Layout and the direction and wrap buttons were already the
/// framework's (`flex_create_choice`, `flex_icon_choice`) and keep their ids and geometry.
#[cfg(not(target_arch = "wasm32"))]
mod framework {
    use super::*;
    use crate::framework_transform::{Number, Quantity, choice, icon_number, number_field};
    use craft_behavior::Mixed;
    use craft_ui_widgets::buttons::IconButton;
    use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _};
    use craft_ui_widgets::pickers::Select;
    use craft_ui_widgets::primitives::{ContentPainterCx, Glyph, IconSpec};
    use craft_ui_widgets::style::UiTokens;
    use craft_ui_widgets::text::link_toggle;

    /// The alignment grid's cells by row and column.
    const CELLS: [[&str; 3]; 3] =
        [["Top Left", "Top Center", "Top Right"], ["Middle Left", "Center", "Middle Right"], ["Bottom Left", "Bottom Center", "Bottom Right"]];

    /// An alignment grid cell's face: the direction's arrow on the chosen cell, a dot on the others.
    #[derive(craft_ui::contract::MemoProps)]
    struct Cell(Option<&'static str>);

    impl Glyph for Cell {
        fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, color: egui::Color32, _mirrored: bool) {
            match self.0 {
                Some(arrow) => icons::paint(painter, rect, arrow, color),
                None => {
                    painter.circle_filled(rect.center(), 1.5, color);
                }
            }
        }
    }

    /// The id of the control `key` of the section the `ui` shows, for the selected container.
    fn cid(app: &DesignApp, ui: &Ui, key: impl std::hash::Hash) -> egui::Id {
        let owner = app.session.active().map(|st| (st.uid, st.identity_epoch, &st.selection.items));
        ui.make_persistent_id(("flex-section", key, owner))
    }

    /// The alignment grid at `r`: 3 × 3 cell buttons (one column or row of three when a space justification spreads the
    /// objects along the main axis), the chosen cell showing the direction's arrow. The cell picked, `row * 3 + col`.
    fn flex_grid(app: &DesignApp, ui: &mut Ui, r: Rect, f: Option<&designcraft_doc::FlexLayout>) -> Option<u8> {
        let t = Tokens::get(ui.ctx());
        // The 48-point outline, as the native grid draws it.
        ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
        let cell = r.width() / 3.0;
        let arrow = flex_arrow(f.map(|f| f.direction));
        let mut picked = None;
        for row in 0..3usize {
            for col in 0..3usize {
                let Some(on) = flex_cell(f, col, row) else { continue };
                let on = on && f.is_some();
                let at = Rect::from_min_size(r.min + vec2(col as f32 * cell, row as f32 * cell), vec2(cell, cell));
                let glyph = Cell(on.then_some(arrow));
                let label = crate::i18n::tr(&app.ui.language, CELLS[row][col]).to_owned();
                let id = cid(app, ui, ("cell", row, col));
                if IconButton::new(IconSpec::glyph(&glyph)).id(id).label(label.clone()).tooltip(label).selected(on).show_at(ui, at).clicked() {
                    picked = Some((row * 3 + col) as u8);
                }
            }
        }
        picked
    }

    pub(super) fn flex_section(app: &mut DesignApp, ui: &mut Ui) {
        use designcraft_doc::{FlexSize, flex};
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
        let u = units(app);
        let has_sel = app.session.active().is_some_and(|s| !s.selection.items.is_empty());
        let container = selected_container(app);
        let f = container.as_ref().and_then(|(it, _)| it.flex.clone());
        let mut on = f.is_some();
        let changed = ui.add_enabled_ui(has_sel, |ui| flex_create_choice(app, ui, &mut on, &tr("Create Flex Layout"))).inner;
        if changed {
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
            if let Some(c) = flex_grid(app, ui, sub(b, 0.0, 0.0, 48.0, 48.0), f.as_ref()) {
                set = Some(json!({"cell": c}));
            }
            let linked = temp_bool(ui, "flex_size_linked", false);
            for (k, (label, name, key, mode, len)) in [
                ("W:", "Width", "width", f.as_ref().map(|f| f.width), rect.map(|r| r.width())),
                ("H:", "Height", "height", f.as_ref().map(|f| f.height), rect.map(|r| r.height())),
            ]
            .into_iter()
            .enumerate()
            {
                let y = k as f32 * (FIELD_H + 5.0);
                crate::framework_transform::caption(ui, Some(sub(b, 56.0, y, 18.0, FIELD_H)), &tr(label));
                let id = cid(app, ui, key);
                let field_name = tr(name);
                let n = Number::new(&field_name, len, Quantity::Length(u)).range(0.0, 100_000.0).enabled(enabled);
                if let Some(Some(v)) = number_field(ui, Some(sub(b, 78.0, y, 66.0, FIELD_H)), id, n) {
                    set = Some(flex_size_set(key, v, linked, rect, len));
                }
                let sizes = [("fixed", tr("Fixed")), ("auto", tr("Auto"))];
                let cur = if mode == Some(FlexSize::Auto) { "auto" } else { "fixed" };
                let id = cid(app, ui, (key, "size"));
                let size_name = format!("{} {}", tr(name), tr("Size"));
                if let Some(v) = choice(ui, Some(sub(b, 148.0, y, 62.0, FIELD_H)), id, &size_name, Some(cur), &sizes, None)
                    && v != cur
                {
                    set = Some(json!({ key: v }));
                }
            }
            let tip = tr("Constrain proportions");
            let id = cid(app, ui, "size-link");
            let toggled = link_toggle(tip.clone(), Bind::given(linked))
                .id(id)
                .tooltip(tip)
                .show_at(ui, sub(b, 214.0, 12.5, 22.0, FIELD_H))
                .events
                .committed()
                .copied();
            if let Some(on) = toggled {
                set_temp_bool(ui, "flex_size_linked", on);
            }
        });
        ui.add_space(6.0);
        // Justification, direction, wrap.
        let b = block(ui, FIELD_H);
        ui.add_enabled_ui(enabled, |ui| {
            let cur = f.as_ref().map(|f| f.justify).unwrap_or_default();
            let cur = serde_json::to_value(cur).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let entries: Vec<Entry<&'static str>> = FLEX_JUSTIFY.iter().map(|(id, _, label)| Entry::new(*id, tr(label))).collect();
            let held = FLEX_JUSTIFY.iter().find(|j| j.0 == cur).map_or(Mixed::Mixed, |j| Mixed::Same(j.0));
            let id = cid(app, ui, "justification");
            let picked = Select::new(tr("Justification"), Bind::given(held), &entries)
                .id(id)
                .enabled(enabled)
                .picture(|id: &&'static str, cx: &ContentPainterCx<'_>, rect: Rect| {
                    let icon = FLEX_JUSTIFY.iter().find(|j| j.0 == *id).map_or("flex-start", |j| j.1);
                    icons::paint(&cx.painter, rect, icon, cx.ink());
                })
                .picture_only()
                .show_at(ui, sub(b, 0.0, 0.0, 58.0, FIELD_H))
                .events
                .committed()
                .copied();
            if let Some(id) = picked {
                set = Some(json!({"justify": id}));
            }
            let dir = f.as_ref().map(|f| f.direction).unwrap_or_default();
            place(ui, sub(b, 78.0, 0.0, 132.0, FIELD_H), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (d, icon, tip) in FLEX_DIRECTIONS {
                    if let Some(after) = flex_icon_choice(app, ui, tip, icon, enabled && dir == d, &tr(tip), vec2(31.0, 21.0))
                        // The four choices are exclusive. Activating the current one retains its direction.
                        && (after || dir == d)
                    {
                        set = Some(json!({"direction": d}));
                    }
                }
            });
            let wrap = f.as_ref().is_some_and(|f| f.wrap);
            if let Some(after) = place(ui, sub(b, 214.0, 0.0, 22.0, FIELD_H), |ui| {
                flex_icon_choice(app, ui, "wrap", "flex-wrap", wrap, &tr("Wrap Content"), vec2(22.0, 21.0))
            }) {
                set = Some(json!({"wrap": after}));
            }
        });
        // Spacing: padding (linked by default) and the spacing between objects.
        section(ui, &tr("Spacing"), true);
        let b = block(ui, 3.0 * FIELD_H + 10.0);
        ui.add_enabled_ui(enabled, |ui| {
            let pad = f.as_ref().map(|f| f.padding).unwrap_or_default();
            let linked = temp_bool(ui, "flex_padding_linked", true);
            for (k, (artwork, tip, x, y)) in FLEX_PADDING.into_iter().enumerate() {
                let id = cid(app, ui, ("padding", k));
                let name = tr(tip);
                let n = Number::new(&name, f.as_ref().map(|_| pad[k]), Quantity::Length(u)).range(0.0, 8000.0).enabled(enabled);
                if let Some(Some(v)) = icon_number(ui, sub(b, x, y + 2.0, 17.0, 17.0), sub(b, x + 20.0, y, 76.0, FIELD_H), id, artwork, n) {
                    set = Some(json!({"padding": flex_padding_set(pad, linked, k, v)}));
                }
            }
            let tip = tr("Make all settings the same");
            let id = cid(app, ui, "padding-link");
            let toggled = link_toggle(tip.clone(), Bind::given(linked))
                .id(id)
                .tooltip(tip)
                .show_at(ui, sub(b, 214.0, 12.5, 22.0, FIELD_H))
                .events
                .committed()
                .copied();
            if let Some(on) = toggled {
                set_temp_bool(ui, "flex_padding_linked", on);
            }
            // Spacing between objects: a space justification works the main axis' out (shown greyed); the cross axis'
            // only counts when objects wrap.
            let (row, distributes, wrap) =
                f.as_ref().map(|f| (f.direction.is_row(), f.justify.distributes(), f.wrap)).unwrap_or((true, false, false));
            let main_gap = arranged.as_ref().map(|a| a.main_gap);
            for (k, (artwork, key, tip, x)) in FLEX_GAPS.into_iter().enumerate() {
                let main = (k == 0) == row;
                let live = if main { !distributes } else { wrap };
                let value = match &f {
                    Some(_) if main && distributes => main_gap,
                    Some(f) => Some(if k == 0 { f.gap_h } else { f.gap_v }),
                    None => None,
                };
                let y = 2.0 * FIELD_H + 10.0;
                let id = cid(app, ui, key);
                let name = tr(tip);
                let n = Number::new(&name, value, Quantity::Length(u)).range(0.0, 8000.0).enabled(enabled && live);
                if let Some(Some(v)) = icon_number(ui, sub(b, x, y + 2.0, 17.0, 17.0), sub(b, x + 20.0, y, 76.0, FIELD_H), id, artwork, n) {
                    set = Some(json!({ key: v }));
                }
            }
        });
        if let Some(p) = set {
            let _ = app.run("flex.set", p);
        }
    }
}

// FLEX_CREATE_DIRECTION_WRAP_TESTS_BEGIN

#[cfg(all(test, not(target_arch = "wasm32")))]
mod application_flex_create_direction_wrap {
    use super::*;
    use designcraft_engine::{DocState, HistoryEntry};
    use egui::{Event, Key, Modifiers, PointerButton, Pos2};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::sync::Arc;

    const REGION: &str = "actual-flex-layout-workflow";
    const CREATE: &str = "Create Flex Layout";
    const DIRECTIONS: [(&str, &str); 4] = [("Row", "row"), ("Reverse Row", "rowReverse"), ("Column", "column"), ("Reverse Column", "columnReverse")];
    #[derive(Clone, Copy)]
    enum Parent {
        Flex,
        Properties,
    }
    struct State {
        app: DesignApp,
        parent: Parent,
        disabled: bool,
        visible: bool,
        sizing: bool,
        width: f32,
        grid: Option<egui::Rect>,
    }
    #[derive(Clone)]
    struct Made {
        ids: Vec<u64>,
        untouched: u64,
    }
    fn select(app: &mut DesignApp, ids: &[u64]) {
        app.run("selection.set", json!({"ids": ids})).unwrap();
    }
    fn fixture() -> (DesignApp, Made) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width": 800, "height": 600, "margins": 36})).unwrap();
        let mut ids = Vec::new();
        for x in [72, 112, 152] {
            ids.push(app.run("frame.create", json!({"rect": [x, 72, x + 30, 92]})).unwrap()["id"].as_u64().unwrap());
        }
        let untouched = app.run("frame.create", json!({"rect": [300, 180, 370, 220]})).unwrap()["id"].as_u64().unwrap();
        app.run("file.new", json!({})).unwrap();
        app.session.set_active(0);
        select(&mut app, &ids);
        // A genuine pending redo proves an accepted edit clears it normally, not by test reset.
        app.run("object.fill", json!({"swatch": "[Paper]"})).unwrap();
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.active().unwrap().history.redo.len(), 1);
        (app, Made { ids, untouched })
    }
    fn configured_container(app: &mut DesignApp) -> u64 {
        let id = app.run("flex.add", json!({})).unwrap()["id"].as_u64().unwrap();
        app.run("flex.set", json!({"width": 90, "height": 76, "justify": "flexStart", "direction": "row", "wrap": false, "padding": [5, 7, 8, 9], "gapH": 10, "gapV": 6})).unwrap();
        id
    }
    fn harness(mut app: DesignApp, installed: bool, parent: Parent, width: f32) -> Harness<'static, State> {
        let ctx = egui::Context::default();
        if installed {
            app.framework_opacity = true;
            app.install_framework(&ctx);
        }
        assert_eq!(app.framework_installed, installed);
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(width, 1000.0)).build_ui_state(
            move |ui, state: &mut State| {
                state.grid = None;
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                let _ = craft_ui_widgets::runtime::inspect::record(ui, egui::Id::NULL.with(REGION), |ui| {
                    if !state.visible {
                        return;
                    }
                    ui.add_enabled_ui(!state.disabled, |ui| {
                        let builder = egui::UiBuilder::new().id_salt("actual_flex_parent");
                        let builder = if state.sizing { builder.sizing_pass() } else { builder };
                        ui.scope_builder(builder, |ui| {
                            if state.sizing {
                                assert!(ui.is_sizing_pass());
                            }
                            match state.parent {
                                Parent::Flex => crate::panels::properties::flex_panel(&mut state.app, ui),
                                Parent::Properties => crate::panels::properties::show(&mut state.app, ui),
                            }
                            // Read the real native painted grid, without modifying its function or IDs.
                            let grids: Vec<egui::Rect> = ui.ctx().graphics(|layers| {
                                layers
                                    .get(ui.layer_id())
                                    .map(|list| {
                                        list.all_entries()
                                            .filter_map(|shape| match &shape.shape {
                                                egui::Shape::Rect(rect) if rect.rect.size() == vec2(48.0, 48.0) => Some(rect.rect),
                                                _ => None,
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default()
                            });
                            assert!(grids.len() <= 1, "unique real native grid outline, never guessed coordinates");
                            state.grid = grids.first().copied();
                        });
                    });
                });
            },
            State { app, parent, disabled: false, visible: true, sizing: false, width, grid: None },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 3);
        h
    }
    fn frame(h: &mut Harness<'_, State>, event: Option<Event>) {
        let mut raw = std::mem::take(h.input_mut());
        if let Some(event) = event {
            raw.events.push(event);
        }
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, State>, count: usize) {
        for _ in 0..count {
            frame(h, None);
        }
    }
    fn key(h: &mut Harness<'_, State>, key: Key) {
        for pressed in [true, false] {
            frame(h, Some(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }));
        }
        idle(h, 2);
    }
    fn pointer(h: &mut Harness<'_, State>, at: Pos2, pressed: bool) {
        frame(h, Some(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
    }
    fn click_at(h: &mut Harness<'_, State>, at: Pos2) {
        frame(h, Some(Event::PointerMoved(at)));
        pointer(h, at, true);
        pointer(h, at, false);
        idle(h, 2);
    }

    fn translated(h: &Harness<'_, State>, text: &str) -> String {
        crate::i18n::tr(&h.state().app.ui.language, text).to_owned()
    }
    fn role(label: &str) -> egui::accesskit::Role {
        if label == CREATE { egui::accesskit::Role::CheckBox } else { egui::accesskit::Role::Button }
    }
    fn control(h: &Harness<'_, State>, label: &str) -> craft_ui_widgets::runtime::inspect::Element {
        let name = translated(h, label);
        let component = if label == CREATE { "Checkbox" } else { "ToggleButton" };
        let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with(REGION)).unwrap();
        let elements: Vec<_> = record
            .elements
            .iter()
            .filter(|e| e.part.is_none() && e.component == component && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(name.as_str()))
            .collect();
        assert_eq!(elements.len(), 1, "one real {component} with complete translated name {name}");
        assert_eq!(h.get_all_by_role_and_label(role(label), &name).count(), 1);
        let element = elements[0].clone();
        assert!(element.rect.is_finite() && element.rect.is_positive());
        if label != CREATE {
            assert_eq!(
                element.rect.size(),
                vec2(if label == "Wrap Content" { 22.0 } else { 31.0 }, 21.0),
                "the original allocated direction/wrap geometry"
            );
        }
        assert!(
            element.rect.center().y < 990.0 && element.rect.center().x < h.state().width,
            "real caller geometry remains physically reachable, not a clipped/offscreen coordinate"
        );
        element
    }
    fn click_choice(h: &mut Harness<'_, State>, label: &str) {
        let element = control(h, label);
        let name = translated(h, label);
        let node = h.get_by_role_and_label(role(label), &name);
        assert!(node.rect().intersects(element.rect));
        click_at(h, element.rect.center());
    }
    fn tab_to(h: &mut Harness<'_, State>, label: &str) {
        let name = translated(h, label);
        let count = h
            .query_all_by(|n| {
                matches!(
                    n.role(),
                    egui::accesskit::Role::Button
                        | egui::accesskit::Role::ComboBox
                        | egui::accesskit::Role::TextInput
                        | egui::accesskit::Role::CheckBox
                )
            })
            .count();
        for _ in 0..=count {
            let node = h.get_by_role_and_label(role(label), &name);
            if node.is_focused() {
                let id = node.accesskit_node().id();
                let focused: Vec<_> = h.query_all_by(|n| n.is_focused()).map(|n| n.accesskit_node().id()).collect();
                assert_eq!(focused, vec![id], "only the naturally tabbed full actual consumer ID is focused");
                return;
            }
            key(h, Key::Tab);
        }
        panic!("natural Tab did not reach current {label} owner");
    }
    fn selected(app: &DesignApp) -> u64 {
        app.session.active().unwrap().selection.items[0].0
    }
    fn item(app: &DesignApp, id: u64) -> &designcraft_doc::Item {
        app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap()
    }
    fn flex(app: &DesignApp, id: u64) -> &designcraft_doc::FlexLayout {
        item(app, id).flex.as_ref().unwrap()
    }
    fn child_ids(app: &DesignApp, id: u64) -> Vec<u64> {
        let designcraft_doc::Content::Group { items } = &item(app, id).content else { panic!("real nested objects") };
        items.iter().map(|child| child.id.0).collect()
    }
    fn assert_geometry(app: &DesignApp, container: u64, ids: &[u64]) {
        let f = flex(app, container);
        assert_eq!(f.padding, [5.0, 7.0, 8.0, 9.0]);
        assert_eq!((f.gap_h, f.gap_v), (10.0, 6.0));
        let rect = item(app, container).inner_bounds();
        assert!((rect.width() - 90.0).abs() < 1e-9 && (rect.height() - 76.0).abs() < 1e-9);
        let (first, second, third) = if f.direction.is_row() {
            let x0 = if f.direction.is_reverse() { 51.0 } else { 7.0 };
            let step = if f.direction.is_reverse() { -40.0 } else { 40.0 };
            ((x0, 5.0), (x0 + step, 5.0), if f.wrap { (x0, 31.0) } else { (x0 + 2.0 * step, 5.0) })
        } else {
            let y0 = if f.direction.is_reverse() { 48.0 } else { 5.0 };
            let step = if f.direction.is_reverse() { -26.0 } else { 26.0 };
            ((7.0, y0), (7.0, y0 + step), if f.wrap { (47.0, y0) } else { (7.0, y0 + 2.0 * step) })
        };
        for (id, (x, y)) in ids.iter().zip([first, second, third]) {
            let r = item(app, *id).bounds();
            assert!(
                (r.x0 - rect.x0 - x).abs() < 1e-6 && (r.y0 - rect.y0 - y).abs() < 1e-6,
                "physical child {id} at {r:?} in {rect:?}; expected {x},{y}"
            );
            assert!((r.width() - 30.0).abs() < 1e-6 && (r.height() - 20.0).abs() < 1e-6);
        }
        assert_eq!(child_ids(app, container), ids, "original object identity/order, not replacement rectangles");
    }
    #[derive(Clone)]
    struct Quiet {
        docs: Vec<DocState>,
        active: Option<usize>,
        prefs: Value,
        journal: Vec<(String, Value)>,
    }
    fn quiet(app: &DesignApp) -> Quiet {
        Quiet {
            docs: app.session.documents().to_vec(),
            active: app.session.active_index(),
            prefs: serde_json::to_value(&app.session.prefs).unwrap(),
            journal: app.session.journal.clone(),
        }
    }
    fn same_entries(now: &[HistoryEntry], before: &[HistoryEntry]) {
        assert_eq!(now.len(), before.len());
        for (now, before) in now.iter().zip(before) {
            assert_eq!(now.label, before.label);
            assert_eq!(now.selection, before.selection);
            assert_eq!(now.doc.as_ref(), before.doc.as_ref());
            assert!(Arc::ptr_eq(&now.doc, &before.doc));
        }
    }
    fn same_doc(now: &DocState, before: &DocState) {
        assert_eq!(format!("{now:?}"), format!("{before:?}"), "complete authoritative DocState, not just the type");
        assert!(Arc::ptr_eq(&now.doc, &before.doc));
        assert!(Arc::ptr_eq(&now.saved_doc, &before.saved_doc));
        same_entries(&now.history.undo, &before.history.undo);
        same_entries(&now.history.redo, &before.history.redo);
        assert_eq!(now.is_dirty(), before.is_dirty());
    }
    fn assert_quiet(app: &DesignApp, before: &Quiet) {
        assert_eq!(app.session.active_index(), before.active);
        assert_eq!(app.session.documents().len(), before.docs.len());
        for (now, before) in app.session.documents().iter().zip(&before.docs) {
            same_doc(now, before);
        }
        assert_eq!(serde_json::to_value(&app.session.prefs).unwrap(), before.prefs);
        assert_eq!(app.session.journal, before.journal, "no added command journal entries, including after idle");
    }

    fn assert_command(app: &DesignApp, before: &Quiet, command: &str, payload: Value, changed: bool) {
        assert_eq!(app.session.active_index(), before.active);
        assert_eq!(app.session.documents().len(), before.docs.len());
        assert_eq!(serde_json::to_value(&app.session.prefs).unwrap(), before.prefs);
        assert_eq!(&app.session.journal[..before.journal.len()], before.journal.as_slice());
        assert_eq!(&app.session.journal[before.journal.len()..], &[(command.to_owned(), payload.clone())]);
        let active = before.active.unwrap();
        let mut oracle = designcraft_engine::Session::new();
        oracle.add_document(before.docs[active].clone());
        oracle.execute(command, &payload).unwrap();
        let mut expected = oracle.active().unwrap().clone();
        let now = app.session.active().unwrap();
        assert_eq!(expected.doc.as_ref() != before.docs[active].doc.as_ref(), changed, "the fixture must exercise the declared real change/no-op");
        assert_eq!(now.doc.as_ref(), expected.doc.as_ref(), "complete actual domain and geometry equals an independent native command");
        assert!(now.revision > before.docs[active].revision);
        expected.revision = now.revision;
        assert_eq!(format!("{now:?}"), format!("{expected:?}"), "entire native DocState, history/selection/saved state, not a boolean-only check");
        assert!(Arc::ptr_eq(&now.saved_doc, &before.docs[active].saved_doc));
        if changed {
            assert_eq!(now.history.undo.len(), before.docs[active].history.undo.len() + 1);
            let label = match command {
                "flex.add" => "Add to Flex Layout",
                "flex.remove" => "Remove Flex Layout",
                "flex.set" => "Flex Layout Options",
                _ => panic!("bounded native oracle"),
            };
            assert_eq!(now.history.undo.last().unwrap().label, label);
            same_entries(&now.history.undo[..before.docs[active].history.undo.len()], &before.docs[active].history.undo);
            assert!(Arc::ptr_eq(&now.history.undo.last().unwrap().doc, &before.docs[active].doc));
            assert!(now.history.redo.is_empty());
        }
        for (index, state) in app.session.documents().iter().enumerate() {
            if index != active {
                same_doc(state, &before.docs[index]);
            }
        }
    }
    fn undo_redo(h: &mut Harness<'_, State>, before: &Quiet) {
        let active = before.active.unwrap();
        let after = quiet(&h.state().app);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(h, 2);
        let now = h.state().app.session.active().unwrap();
        assert_eq!(now.doc.as_ref(), before.docs[active].doc.as_ref());
        assert!(Arc::ptr_eq(&now.doc, &before.docs[active].doc));
        assert_eq!(now.selection, before.docs[active].selection);
        same_entries(&now.history.undo, &before.docs[active].history.undo);
        assert_eq!(now.history.redo.len(), 1);
        assert!(Arc::ptr_eq(&now.history.redo[0].doc, &after.docs[active].doc));
        assert!(Arc::ptr_eq(&now.saved_doc, &before.docs[active].saved_doc));
        let mut expected_undo = before.docs[active].clone();
        expected_undo.revision = now.revision;
        expected_undo.history.redo = vec![HistoryEntry {
            label: after.docs[active].history.undo.last().unwrap().label.clone(),
            doc: after.docs[active].doc.clone(),
            selection: after.docs[active].selection.clone(),
        }];
        assert_eq!(format!("{now:?}"), format!("{expected_undo:?}"), "complete Undo state, including unrelated metadata and exact redo contents");
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        idle(h, 2);
        let now = h.state().app.session.active().unwrap();
        assert!(Arc::ptr_eq(&now.doc, &after.docs[active].doc));
        assert_eq!(now.doc.as_ref(), after.docs[active].doc.as_ref());
        assert_eq!(now.selection, after.docs[active].selection);
        same_entries(&now.history.undo, &after.docs[active].history.undo);
        assert!(now.history.redo.is_empty());
        let mut expected_redo = after.docs[active].clone();
        expected_redo.revision = now.revision;
        assert_eq!(format!("{now:?}"), format!("{expected_redo:?}"), "complete Redo state and preserved metadata");
        assert_eq!(h.state().app.session.active_index(), before.active);
        assert_eq!(serde_json::to_value(&h.state().app.session.prefs).unwrap(), before.prefs);
        assert_eq!(&h.state().app.session.journal[..after.journal.len()], after.journal.as_slice());
        assert_eq!(
            &h.state().app.session.journal[after.journal.len()..],
            &[("edit.undo".to_owned(), json!({})), ("edit.redo".to_owned(), json!({}))]
        );
        for (index, doc) in h.state().app.session.documents().iter().enumerate() {
            if index != active {
                same_doc(doc, &before.docs[index]);
            }
        }
        let stable = quiet(&h.state().app);
        idle(h, 3);
        assert_quiet(&h.state().app, &stable);
    }

    #[test]
    fn flex_complete_create_detect_four_directions_wrap_remove_and_undo_through_actual_flex_and_properties_parents() {
        for parent in [Parent::Flex, Parent::Properties] {
            let (app, made) = fixture();
            let mut h = harness(app, true, parent, 380.0);
            let untouched = item(&h.state().app, made.untouched).clone();
            let before = quiet(&h.state().app);
            click_choice(&mut h, CREATE);
            assert_command(&h.state().app, &before, "flex.add", json!({}), true);
            let container = selected(&h.state().app);
            assert_eq!(child_ids(&h.state().app, container), made.ids);
            let detected = flex(&h.state().app, container);
            assert_eq!(
                (detected.direction, detected.wrap, detected.width, detected.height, detected.justify),
                (
                    designcraft_doc::FlexDirection::Row,
                    false,
                    designcraft_doc::FlexSize::Auto,
                    designcraft_doc::FlexSize::Auto,
                    designcraft_doc::FlexJustify::SpaceBetween
                )
            );
            assert!((detected.gap_h - 10.0).abs() < 1e-9);
            for (id, x) in made.ids.iter().zip([72.0, 112.0, 152.0]) {
                assert_eq!(
                    item(&h.state().app, *id).bounds(),
                    designcraft_geom::Rect::new(x, 72.0, x + 30.0, 92.0),
                    "native detection must preserve real object geometry"
                );
            }
            undo_redo(&mut h, &before);
            // A genuine authoritative external refresh establishes fixed dimensions for visible arrangement.
            h.state_mut().app.run("flex.set", json!({"width":90,"height":76,"justify":"flexStart","padding":[5,7,8,9],"gapH":10,"gapV":6})).unwrap();
            idle(&mut h, 2);
            for (label, direction) in DIRECTIONS {
                let before = quiet(&h.state().app);
                let changed = serde_json::to_value(flex(&h.state().app, container).direction).unwrap() != json!(direction);
                click_choice(&mut h, label);
                assert_command(&h.state().app, &before, "flex.set", json!({"direction": direction}), changed);
                assert_geometry(&h.state().app, container, &made.ids);
                if changed {
                    undo_redo(&mut h, &before);
                }
            }
            let before = quiet(&h.state().app);
            click_choice(&mut h, "Wrap Content");
            assert_command(&h.state().app, &before, "flex.set", json!({"wrap":true}), true);
            assert_geometry(&h.state().app, container, &made.ids);
            undo_redo(&mut h, &before);
            let preserved: Vec<_> = made.ids.iter().map(|id| item(&h.state().app, *id).bounds()).collect();
            let before = quiet(&h.state().app);
            click_choice(&mut h, CREATE);
            assert_command(&h.state().app, &before, "flex.remove", json!({}), true);
            assert!(item(&h.state().app, container).is_group() && item(&h.state().app, container).flex.is_none());
            assert_eq!(child_ids(&h.state().app, container), made.ids);
            assert_eq!(
                made.ids.iter().map(|id| item(&h.state().app, *id).bounds()).collect::<Vec<_>>(),
                preserved,
                "unwrap retains every physical child position"
            );
            assert_eq!(item(&h.state().app, made.untouched), &untouched);
            assert_eq!(
                h.query_all_by_role(egui::accesskit::Role::ListBoxOption).count(),
                0,
                "the coherent workflow has no chooser popup interaction"
            );
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn flex_single_real_frame_and_group_conversion_keep_identity_and_painted_frame_removal_keeps_its_look() {
        for grouped in [false, true] {
            let (mut app, made) = fixture();
            if grouped {
                app.run("object.group", json!({})).unwrap();
            } else {
                select(&mut app, &[made.untouched]);
            }
            let original = selected(&app);
            if !grouped {
                app.run("object.fill", json!({"swatch":"[Paper]"})).unwrap();
            }
            let mut h = harness(app, true, Parent::Flex, 300.0);
            let before = quiet(&h.state().app);
            click_choice(&mut h, CREATE);
            assert_command(&h.state().app, &before, "flex.add", json!({}), true);
            assert_eq!(selected(&h.state().app), original, "single convertible frame/group keeps the actual object ID");
            assert!(item(&h.state().app, original).flex.is_some());
            undo_redo(&mut h, &before);
            let look = (item(&h.state().app, original).fill.clone(), item(&h.state().app, original).stroke.clone());
            let before = quiet(&h.state().app);
            click_choice(&mut h, CREATE);
            assert_command(&h.state().app, &before, "flex.remove", json!({}), true);
            let after = item(&h.state().app, original);
            assert!(after.flex.is_none());
            assert_eq!((after.fill.clone(), after.stroke.clone()), look);
            if grouped {
                assert!(after.is_group());
                assert_eq!(child_ids(&h.state().app, original), made.ids);
            } else {
                assert_eq!(after.shape, designcraft_doc::Shape::Rectangle);
                assert!(!after.fill.is_none());
            }
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn flex_natural_keyboard_repeated_wrap_and_all_direction_activations_preserve_focus_and_exact_geometry() {
        let (mut app, made) = fixture();
        let container = configured_container(&mut app);
        let mut h = harness(app, true, Parent::Flex, 300.0);
        let ids: Vec<_> = DIRECTIONS.iter().map(|(label, _)| control(&h, label).id).collect();
        for (index, (label, direction)) in DIRECTIONS.iter().enumerate() {
            tab_to(&mut h, label);
            let before = quiet(&h.state().app);
            let changed = serde_json::to_value(flex(&h.state().app, container).direction).unwrap() != json!(direction);
            key(&mut h, Key::Enter);
            assert_command(&h.state().app, &before, "flex.set", json!({"direction":direction}), changed);
            assert_eq!(control(&h, label).id, ids[index]);
            assert!(h.get_by_role_and_label(role(label), &translated(&h, label)).is_focused());
            assert_geometry(&h.state().app, container, &made.ids);
            if changed {
                undo_redo(&mut h, &before);
            }
        }
        tab_to(&mut h, "Wrap Content");
        let owner = control(&h, "Wrap Content").id;
        for on in [true, false, true] {
            let before = quiet(&h.state().app);
            key(&mut h, Key::Space);
            assert_command(&h.state().app, &before, "flex.set", json!({"wrap":on}), true);
            assert_eq!(control(&h, "Wrap Content").id, owner, "successive actual bool changes are the same keyboard control");
            assert!(h.get_by_role_and_label(role("Wrap Content"), &translated(&h, "Wrap Content")).is_focused());
            assert_geometry(&h.state().app, container, &made.ids);
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn flex_first_container_is_the_display_authority_but_native_commands_target_all_selected_containers_not_plain_items() {
        let (mut app, made) = fixture();
        let first = configured_container(&mut app);
        select(&mut app, &[made.untouched]);
        let second = configured_container(&mut app);
        app.run("flex.set", json!({"direction":"column","wrap":true})).unwrap();
        let plain = app.run("frame.create", json!({"rect":[440,180,490,200]})).unwrap()["id"].as_u64().unwrap();
        select(&mut app, &[second, first, plain]);
        let mut h = harness(app, true, Parent::Properties, 380.0);
        assert_eq!(selected_container(&h.state().app).unwrap().0.id.0, second);
        assert_eq!(
            h.get_by_role_and_label(role("Column"), &translated(&h, "Column")).accesskit_node().toggled(),
            Some(egui::accesskit::Toggled::True)
        );
        let protected = item(&h.state().app, plain).clone();
        let before = quiet(&h.state().app);
        click_choice(&mut h, "Reverse Row");
        assert_command(&h.state().app, &before, "flex.set", json!({"direction":"rowReverse"}), true);
        for id in [first, second] {
            assert_eq!(flex(&h.state().app, id).direction, designcraft_doc::FlexDirection::RowReverse);
        }
        assert_eq!(item(&h.state().app, plain), &protected);
        undo_redo(&mut h, &before);
        let before = quiet(&h.state().app);
        click_choice(&mut h, "Wrap Content");
        assert_command(&h.state().app, &before, "flex.set", json!({"wrap":false}), true);
        for id in [first, second] {
            assert!(!flex(&h.state().app, id).wrap);
        }
        assert_eq!(item(&h.state().app, plain), &protected);
        undo_redo(&mut h, &before);
    }

    #[test]
    fn flex_held_pointer_direction_retires_actual_selection_document_epoch_disabled_hidden_and_absent_owners() {
        for change in ["selection", "document", "epoch", "disabled", "hidden", "no-selection"] {
            let (mut app, made) = fixture();
            let original = configured_container(&mut app);
            let mut h = harness(app, true, Parent::Flex, 300.0);
            let old = control(&h, "Column");
            let at = old.rect.center();
            frame(&mut h, Some(Event::PointerMoved(at)));
            pointer(&mut h, at, true);
            match change {
                "selection" => select(&mut h.state_mut().app, &[made.untouched]),
                "document" => {
                    h.state_mut().app.run("file.new", json!({})).unwrap();
                }
                "epoch" => {
                    let old = h.state().app.session.active().unwrap();
                    let mut new = DocState::new((*old.doc).clone(), old.path.clone());
                    new.uid = old.uid;
                    new.selection = old.selection.clone();
                    assert_ne!(new.identity_epoch, old.identity_epoch);
                    *h.state_mut().app.session.active_mut().unwrap() = new;
                }
                "disabled" => h.state_mut().disabled = true,
                "hidden" => h.state_mut().visible = false,
                "no-selection" => select(&mut h.state_mut().app, &[]),
                _ => unreachable!(),
            }
            let retired = quiet(&h.state().app);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &retired);
            if matches!(change, "selection" | "document" | "epoch" | "no-selection") {
                assert_ne!(control(&h, "Column").id, old.id);
            }
            pointer(&mut h, at, false);
            key(&mut h, Key::Space);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &retired);
            h.state_mut().disabled = false;
            h.state_mut().visible = true;
            h.state_mut().app.session.set_active(0);
            select(&mut h.state_mut().app, &[original]);
            idle(&mut h, 2);
            let before = quiet(&h.state().app);
            click_choice(&mut h, "Column");
            assert_command(&h.state().app, &before, "flex.set", json!({"direction":"column"}), true);
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn flex_held_keyboard_wrap_retires_document_epoch_and_selection_without_a_release_edit_then_recovers_naturally() {
        for epoch in [false, true] {
            let (mut app, made) = fixture();
            let original = configured_container(&mut app);
            let mut h = harness(app, true, Parent::Flex, 300.0);
            tab_to(&mut h, "Wrap Content");
            let owner = control(&h, "Wrap Content").id;
            let key_down_before = quiet(&h.state().app);
            frame(&mut h, Some(Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }));
            // Public ToggleButton activates on the routed key press, not its release.
            assert_command(&h.state().app, &key_down_before, "flex.set", json!({"wrap":true}), true);
            if epoch {
                let old = h.state().app.session.active().unwrap();
                let mut new = DocState::new((*old.doc).clone(), old.path.clone());
                new.uid = old.uid;
                new.selection = old.selection.clone();
                assert_ne!(new.identity_epoch, old.identity_epoch);
                *h.state_mut().app.session.active_mut().unwrap() = new;
            } else {
                select(&mut h.state_mut().app, &[made.untouched]);
            }
            let retired = quiet(&h.state().app);
            idle(&mut h, 2);
            assert_ne!(control(&h, "Wrap Content").id, owner);
            frame(&mut h, Some(Event::Key { key: Key::Space, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }));
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &retired);
            select(&mut h.state_mut().app, &[original]);
            idle(&mut h, 2);
            tab_to(&mut h, "Wrap Content");
            let before = quiet(&h.state().app);
            key(&mut h, Key::Space);
            assert_command(&h.state().app, &before, "flex.set", json!({"wrap":false}), true);
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn flex_genuine_missing_document_selection_and_parent_disablement_are_quiet_then_recover_in_the_same_real_parent() {
        for absent in ["document", "selection", "disabled"] {
            let (mut app, made) = fixture();
            if absent == "document" {
                while !app.session.documents().is_empty() {
                    app.session.close_document(0);
                }
            }
            if absent == "selection" {
                select(&mut app, &[]);
            }
            let mut h = harness(app, true, Parent::Flex, 300.0);
            h.state_mut().disabled = absent == "disabled";
            idle(&mut h, 2);
            let before = quiet(&h.state().app);
            assert!(control(&h, CREATE).facts.unwrap().disabled);
            click_choice(&mut h, CREATE);
            key(&mut h, Key::Space);
            key(&mut h, Key::Enter);
            assert_quiet(&h.state().app, &before);
            h.state_mut().disabled = false;
            if absent == "document" {
                h.state_mut().app.run("file.new", json!({})).unwrap();
                let id = h.state_mut().app.run("frame.create", json!({"rect":[72,72,142,112]})).unwrap()["id"].as_u64().unwrap();
                select(&mut h.state_mut().app, &[id]);
            } else {
                select(&mut h.state_mut().app, &made.ids);
            }
            idle(&mut h, 2);
            let before = quiet(&h.state().app);
            click_choice(&mut h, CREATE);
            assert_command(&h.state().app, &before, "flex.add", json!({}), true);
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn flex_real_sizing_and_hidden_parent_publish_no_live_sdk_owner_hit_or_focus_stop_and_do_not_replay_held_input() {
        for sizing in [false, true] {
            let (mut app, _) = fixture();
            configured_container(&mut app);
            let mut h = harness(app, true, Parent::Flex, 300.0);
            let at = control(&h, "Column").rect.center();
            frame(&mut h, Some(Event::PointerMoved(at)));
            pointer(&mut h, at, true);
            h.state_mut().sizing = sizing;
            h.state_mut().visible = sizing;
            let before = quiet(&h.state().app);
            idle(&mut h, 2);
            let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with(REGION)).unwrap();
            assert!(!record.elements.iter().any(|e| matches!(e.component, "Checkbox" | "ToggleButton")));
            assert!(record.hits.is_empty());
            assert!(record.elements.iter().all(|e| e.stop.is_none()));
            pointer(&mut h, at, false);
            key(&mut h, Key::Enter);
            assert_quiet(&h.state().app, &before);
            h.state_mut().sizing = false;
            h.state_mut().visible = true;
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &before);
            let before = quiet(&h.state().app);
            click_choice(&mut h, "Column");
            assert_command(&h.state().app, &before, "flex.set", json!({"direction":"column"}), true);
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn flex_authoritative_refresh_localized_names_narrow_parent_and_untouched_native_numeric_fields_stay_honest() {
        for (language, width) in [("en", 190.0), ("de", 300.0)] {
            let (mut app, made) = fixture();
            let container = configured_container(&mut app);
            app.ui.language = language.into();
            let mut h = harness(app, true, Parent::Flex, width);
            let numbers = |h: &Harness<'_, State>| {
                h.query_all_by_role(egui::accesskit::Role::TextInput).map(|n| (n.accesskit_node().label(), n.value())).collect::<Vec<_>>()
            };
            let original = numbers(&h);
            assert!(original.len() >= 8, "real original dimensions/padding/gaps native inputs");
            let owner = control(&h, "Reverse Row").id;
            h.state_mut().app.run("flex.set", json!({"direction":"rowReverse","wrap":true})).unwrap();
            let refreshed = quiet(&h.state().app);
            idle(&mut h, 3);
            assert_quiet(&h.state().app, &refreshed);
            assert_eq!(control(&h, "Reverse Row").id, owner);
            assert_eq!(
                h.get_by_role_and_label(role("Reverse Row"), &translated(&h, "Reverse Row")).accesskit_node().toggled(),
                Some(egui::accesskit::Toggled::True)
            );
            let before = quiet(&h.state().app);
            click_choice(&mut h, "Column");
            assert_command(&h.state().app, &before, "flex.set", json!({"direction":"column"}), true);
            assert_geometry(&h.state().app, container, &made.ids);
            assert_eq!(numbers(&h), original, "no numeric grammar/precision/proportions/padding/gap field substitution");
            let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with(REGION)).unwrap();
            for label in [CREATE, "Row", "Reverse Row", "Column", "Reverse Column", "Wrap Content"] {
                // This loop observes complete names, including native controls clipped by the
                // deliberately narrow190px parent. It is not a request to hit every control.
                // Keep exact typed/AX uniqueness and original allocated geometry here; every
                // actual input still goes through the unchanged strict reachable control() helper.
                let component = if label == CREATE { "Checkbox" } else { "ToggleButton" };
                let name = translated(&h, label);
                let elements: Vec<_> = record
                    .elements
                    .iter()
                    .filter(|e| {
                        e.part.is_none() && e.component == component && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(name.as_str())
                    })
                    .collect();
                assert_eq!(elements.len(), 1, "complete real names remain accessible, not icon IDs or clipped display fragments");
                assert_eq!(h.get_all_by_role_and_label(role(label), &name).count(), 1, "one complete-name AX role owner, not a LabelLeaf");
                let element = elements[0];
                assert!(element.rect.is_finite() && element.rect.is_positive());
                if label != CREATE {
                    assert_eq!(
                        element.rect.size(),
                        vec2(if label == "Wrap Content" { 22.0 } else { 31.0 }, 21.0),
                        "the original allocated direction/wrap geometry"
                    );
                }
            }
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn flex_original_physical_three_by_three_alignment_grid_stays_native_for_each_real_direction() {
        for (label, direction) in DIRECTIONS {
            let (mut app, _) = fixture();
            configured_container(&mut app);
            app.run("flex.set", json!({"direction":direction})).unwrap();
            let mut h = harness(app, true, Parent::Flex, 300.0);
            for cell in 0..9 {
                let grid = h.state().grid.expect("actual painted 48-point native alignment grid");
                let at = grid.min + vec2((cell % 3) as f32 * 16.0 + 8.0, (cell / 3) as f32 * 16.0 + 8.0);
                assert!(grid.contains(at));
                let before = quiet(&h.state().app);
                let mut oracle = designcraft_engine::Session::new();
                oracle.add_document(before.docs[before.active.unwrap()].clone());
                oracle.execute("flex.set", &json!({"cell":cell})).unwrap();
                let changed = oracle.active().unwrap().doc.as_ref() != before.docs[before.active.unwrap()].doc.as_ref();
                click_at(&mut h, at);
                assert_command(&h.state().app, &before, "flex.set", json!({"cell":cell}), changed);
                assert_eq!(serde_json::to_value(flex(&h.state().app, selected(&h.state().app)).direction).unwrap(), json!(direction));
                if changed {
                    undo_redo(&mut h, &before);
                }
            }
            assert_eq!(control(&h, label).facts.unwrap().name, Some(translated(&h, label)));
        }
    }

    #[test]
    fn flex_genuinely_uninstalled_native_create_direction_wrap_remove_recovers_to_installed_public_controls() {
        let (app, _) = fixture();
        let mut h = harness(app, false, Parent::Flex, 300.0);
        assert!(!h.state().app.framework_installed);
        for (label, command, payload) in [
            (CREATE, "flex.add", json!({})),
            ("Column", "flex.set", json!({"direction":"column"})),
            ("Wrap Content", "flex.set", json!({"wrap":true})),
            (CREATE, "flex.remove", json!({})),
        ] {
            let before = quiet(&h.state().app);
            let at = h.get_by_role_and_label(role(label), &translated(&h, label)).rect().center();
            click_at(&mut h, at);
            assert_command(&h.state().app, &before, command, payload, true);
            undo_redo(&mut h, &before);
        }
        let ctx = h.ctx.clone();
        h.state_mut().app.framework_opacity = true;
        h.state_mut().app.install_framework(&ctx);
        assert!(h.state().app.framework_installed);
        let before = quiet(&h.state().app);
        idle(&mut h, 3);
        assert_quiet(&h.state().app, &before);
        click_choice(&mut h, CREATE);
        assert_command(&h.state().app, &before, "flex.add", json!({}), true);
        undo_redo(&mut h, &before);
    }

    #[test]
    fn flex_real_nested_plain_container_removal_refusal_keeps_full_domain_and_history_then_a_valid_target_recovers() {
        let (mut app, made) = fixture();
        let inner = configured_container(&mut app);
        select(&mut app, &[inner, made.untouched]);
        app.run("object.group", json!({})).unwrap();
        let outer = selected(&app);
        select(&mut app, &[inner]);
        let mut h = harness(app, true, Parent::Flex, 300.0);
        let before = quiet(&h.state().app);
        let mut oracle = designcraft_engine::Session::new();
        oracle.add_document(before.docs[before.active.unwrap()].clone());
        assert!(
            oracle.execute("flex.remove", &json!({})).is_err(),
            "genuine existing nested plain-container removal refusal, not a fake disabled flag"
        );
        click_choice(&mut h, CREATE);
        assert_quiet(&h.state().app, &before);
        assert!(flex(&h.state().app, inner).direction.is_row());
        assert_eq!(
            h.get_by_role_and_label(role(CREATE), &translated(&h, CREATE)).accesskit_node().toggled(),
            Some(egui::accesskit::Toggled::True),
            "refusal redraws authoritative membership, not the proposed false value"
        );
        select(&mut h.state_mut().app, &[outer]);
        idle(&mut h, 2);
        let before = quiet(&h.state().app);
        click_choice(&mut h, CREATE);
        assert_command(&h.state().app, &before, "flex.add", json!({}), true);
        assert!(item(&h.state().app, inner).flex.is_some());
        undo_redo(&mut h, &before);
    }
}
