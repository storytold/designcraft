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

mod document;
mod flex;
mod links;
mod object;
mod object_panels;
mod text;
mod typography;

pub(crate) use document::{PAGE_PRESETS, preset_name};
use document::{document_sections, page_sections};
pub use flex::flex_panel;
use flex::flex_section;
pub(crate) use links::run_update_links;
pub use links::{info_panel, links_panel, preflight_panel, update_links};
#[cfg(test)]
use object::align_to_icon;
use object::{align_row, appearance_section, guide_section, quick_actions_object, transform_section, wrap_section};
pub(crate) use object::{corner_shape_button, corner_type_dropdown, paint_corner_shape, preview_row, ref_point_proxy, stroke_type_dropdown};
pub use object_panels::{align_panel, attributes_panel, effects_panel, pathfinder_panel, stroke_panel, wrap_panel};
use object_panels::{anchor_point_sections, pathfinder_row};
pub(crate) use text::{LANGUAGES, kerning_control, language_menu};
use text::{
    bullets_section, character_section, paragraph_section, quick_actions_text, text_appearance_section, text_frame_section, text_style_section,
};
pub use typography::{DIGIT_OPTIONS, character_panel, digit_label, paragraph_panel, show_typography_panel};
#[cfg(test)]
use typography::{axis_step, usable_axes, variable_font_style};
pub(crate) use typography::{composer_choice, para_field_min};

/// A link to another panel: `window.panel` shows it in the docking layout. An open flyout of
/// that panel stays: `window.panel` would close it.
fn show_panel(app: &mut DesignApp, panel: &str) {
    if app.ui.docking_flyout.as_deref() != Some(panel) {
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
    ui.data(|d| d.get_temp::<bool>(egui::Id::unique(key))).unwrap_or(default)
}

fn set_temp_bool(ui: &Ui, key: &str, v: bool) {
    ui.data_mut(|d| d.insert_temp(egui::Id::unique(key), v));
}

/// Label of the current selection for the selection-type band.
fn selection_title(app: &DesignApp, info: &Option<SelInfo>) -> String {
    let Some(st) = app.session.active() else { return String::new() };
    if st.selection.text.is_some() {
        return crate::i18n::tr(&app.ui.language, "Characters").into();
    }
    if !st.selection.anchors.is_empty() {
        return crate::i18n::tr(&app.ui.language, "Anchor Point(s)").into();
    }
    if info.is_none() && !st.selection.guides.is_empty() {
        return crate::i18n::tr(&app.ui.language, "Ruler Guide").into();
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
    if info.is_none() && app.session.active().is_some_and(|st| !st.selection.guides.is_empty()) {
        section(ui, crate::i18n::tr(&app.ui.language, "Transform"), true);
        guide_section(app, ui);
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
    if app.session.active().is_some_and(|st| !st.selection.anchors.is_empty()) {
        anchor_point_sections(app, ui, &i);
        return;
    }
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
                // Frame Fitting Options…, as InDesign's Options button beside the fitting buttons.
                if outline_button(ui, crate::i18n::tr(&app.ui.language, "Options"), 64.0).clicked() {
                    crate::menus::activate(app, "app.fittingOptions", &Value::Null);
                }
            });
            // Auto-Fit: the graphic is refitted whenever its frame is resized.
            let mut auto = app
                .session
                .active()
                .and_then(|st| st.selection.items.first().and_then(|id| st.doc.item(*id)))
                .and_then(|it| it.graphic())
                .is_some_and(|g| g.auto_fit != designcraft_doc::Fitting::None);
            if widgets::checkbox(ui, &mut auto, crate::i18n::tr(&app.ui.language, "Auto-Fit")).changed() {
                let _ = app.run("object.fittingOptions", json!({"autoFit": auto}));
            }
            divider(ui);
        }
        section(ui, crate::i18n::tr(&app.ui.language, "Text Wrap"), true);
        wrap_section(app, ui, &i);
        divider(ui);
    }
    section(ui, crate::i18n::tr(&app.ui.language, "Quick Actions"), true);
    quick_actions_object(app, ui, &i);
}

#[cfg(test)]
mod tests;
