//! Panels and shared panel helpers.

pub mod cell_stroke;
pub mod conditions;
mod font_menu;
pub use font_menu::{FontPick, SAMPLES as FONT_SAMPLES, font_menu_body};
pub mod datamerge;
pub mod glyphs;
pub mod interactive;
pub mod layers;
pub mod library;
pub mod notes;
pub mod pages;
pub mod properties;
pub mod story;
pub mod styles;
pub mod swatches;
pub mod table;
pub mod tabs;
pub mod typography;

#[cfg(test)]
mod color_panel_tests;

use designcraft_doc::{Content, StrokeAlign, WrapMode};
use designcraft_geom::Rect;
use egui::vec2;
use serde_json::{Value, json};

use crate::DesignApp;
use crate::theme::Tokens;

/// Summary of the current object selection (for the Control panel and Properties).
#[derive(Clone, Debug)]
pub struct SelInfo {
    /// Bounds measured from the rulers' zero point (as the X/Y fields show them).
    pub page_rect: Rect,
    pub count: usize,
    pub fill: String,
    pub stroke: String,
    pub stroke_weight: f64,
    pub stroke_align: StrokeAlign,
    pub opacity: f64,
    pub shadow: bool,
    pub wrap: &'static str,
    pub columns: Option<u32>,
    pub kind: &'static str,
    pub is_graphic: bool,
    pub is_text: bool,
    /// Stroke type id (`solid`, `dashed`, `thickThin`, …).
    pub stroke_kind: String,
    /// Uniform corner (shape, size) — the first corner.
    pub corner: (designcraft_geom::corners::CornerShape, f64),
    pub wrap_invert: bool,
    /// Text wrap side and contour type ids (`bothSides`, `sameAsClipping`, …).
    pub wrap_side: String,
    pub wrap_contour: String,
    /// Text wrap offsets: top, left, bottom, right.
    pub wrap_offsets: [f64; 4],
    pub locked: bool,
}

/// Colour edits go to text: text is selected, or text frames are selected with Formatting
/// Affects Text on.
pub fn colors_affect_text(app: &DesignApp) -> bool {
    let Some(st) = app.session.active() else { return false };
    st.selection.text.is_some()
        || (app.ui.formatting_affects_text && st.selection.items.iter().any(|id| st.doc.item(*id).is_some_and(|i| i.is_text_frame())))
}

/// Fill and stroke swatches and tints of what colour edits apply to: the text (see
/// [`colors_affect_text`]) or the first selected object.
pub fn color_target(app: &mut DesignApp) -> Option<(String, f32, String, f32)> {
    if colors_affect_text(app) {
        let a = text_attrs(app)?;
        let c = &a["chars"];
        let tint = |k: &str| c[k].as_f64().unwrap_or(1.0) as f32;
        return Some((c["fill"].as_str()?.to_string(), tint("fillTint"), c["stroke"].as_str()?.to_string(), tint("strokeTint")));
    }
    let i = sel_info(app)?;
    Some((i.fill, sel_tint(app, false), i.stroke, sel_tint(app, true)))
}

/// Apply a swatch (and tint) to the fill or stroke of what colour edits apply to.
pub fn apply_swatch(app: &mut DesignApp, stroke: bool, swatch: &str, tint: Option<f32>) -> Result<Value, String> {
    remember_swatch(app, swatch);
    if colors_affect_text(app) {
        let (k, kt) = if stroke { ("stroke", "strokeTint") } else { ("fill", "fillTint") };
        let mut attrs = json!({k: swatch});
        if let Some(t) = tint {
            attrs[kt] = json!(t);
        }
        return app.run("type.char", json!({ "attrs": attrs }));
    }
    let mut p = json!({"swatch": swatch});
    if let Some(t) = tint {
        p["tint"] = json!(t);
    }
    app.run(if stroke { "object.stroke" } else { "object.fill" }, p)
}

/// Keep `swatch` as the colour or gradient Apply Color / Apply Gradient use next.
pub fn remember_swatch(app: &mut DesignApp, swatch: &str) {
    let Some(st) = app.session.active() else { return };
    if swatch == designcraft_color::swatch::NONE {
        return;
    }
    if designcraft_color::swatch::resolve_gradient(&st.doc.swatches, swatch).is_some() {
        app.ui.last_gradient = Some(swatch.to_string());
    } else if st.doc.resolve_color(swatch, 1.0).is_some() {
        app.ui.last_color = Some(swatch.to_string());
    }
}

/// Fill (or stroke) tint of the first selected item (1 = 100%).
pub fn sel_tint(app: &DesignApp, stroke: bool) -> f32 {
    let Some(st) = app.session.active() else { return 1.0 };
    st.selection.items.first().and_then(|id| st.doc.item(*id)).map_or(1.0, |it| if stroke { it.stroke.tint } else { it.fill.tint })
}

/// The one selected ruler guide, as the X field (vertical guide) or Y field (horizontal guide)
/// of the Control and Properties panels shows it: measured as the rulers measure, from the zero
/// point (and the page, when rulers restart per page), in that ruler's units.
#[derive(Clone, Copy, Debug)]
pub struct GuideField {
    pub guide: designcraft_doc::GuideSel,
    pub orientation: designcraft_doc::Orientation,
    /// Points from the ruler origin.
    pub value: f64,
    /// Where `value` 0 is: a spread x for a vertical guide, a y for a horizontal one.
    pub origin: f64,
    pub unit: designcraft_geom::Unit,
}

impl GuideField {
    /// Which field shows it: 0 for X, 1 for Y.
    pub fn row(&self) -> usize {
        match self.orientation {
            designcraft_doc::Orientation::Vertical => 0,
            designcraft_doc::Orientation::Horizontal => 1,
        }
    }
    /// The `guide.move` params that put it at `value` (as the field reads).
    pub fn move_to(&self, value: f64) -> Value {
        let g = self.guide;
        json!({"spread": g.spread, "page": g.page, "index": g.index, "position": value + self.origin})
    }
}

/// [`GuideField`] when exactly one ruler guide is selected.
pub fn guide_field(app: &DesignApp) -> Option<GuideField> {
    let st = app.session.active()?;
    let [g] = st.selection.guides.as_slice() else { return None };
    let pg = st.doc.spread(g.spread)?.pages.get(g.page)?;
    let guide = pg.guides.get(g.index)?;
    let (origin, unit) = match guide.orientation {
        designcraft_doc::Orientation::Vertical => (st.doc.ruler_origin(g.spread, guide.position)?.x, st.doc.settings.horizontal_units),
        designcraft_doc::Orientation::Horizontal => (st.doc.ruler_origin(g.spread, pg.x + pg.width / 2.0)?.y, st.doc.settings.vertical_units),
    };
    let value = guide.position - origin;
    value.is_finite().then_some(GuideField { guide: *g, orientation: guide.orientation, value, origin, unit })
}

pub fn sel_info(app: &DesignApp) -> Option<SelInfo> {
    let st = app.session.active()?;
    if st.selection.items.is_empty() {
        return None;
    }
    let d = &st.doc;
    let with_stroke = app.session.prefs.dimensions_include_stroke;
    let mut b: Option<Rect> = None;
    for id in &st.selection.items {
        let it = d.item(*id)?;
        // Direct Selection of a placed graphic: its bounds, as the Transform fields then set.
        let ib = match it.graphic() {
            Some(g) if st.selection.content => {
                let full = d.find(*id).map_or(designcraft_geom::Affine::IDENTITY, |l| d.parent_xf(&l));
                (full * it.xf * g.xf).transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1))
            }
            _ if with_stroke => it.visible_bounds(),
            _ => it.bounds(),
        };
        b = Some(b.map_or(ib, |r| r.union(ib)));
    }
    let b = b?;
    let first = d.item(st.selection.items[0])?;
    let loc = d.find(first.id)?;
    let o = d.ruler_origin(loc.spread, b.center().x)?;
    // With the graphic selected (Direct Selection), the Text Wrap panel shows its own wrap.
    let tw = match (&first.content, st.selection.content) {
        (Content::Graphic(g), true) => &g.wrap,
        _ => &first.wrap,
    };
    let wrap = match tw.mode {
        WrapMode::None => "none",
        WrapMode::BoundingBox => "boundingBox",
        WrapMode::Contour => "contour",
        WrapMode::JumpObject => "jumpObject",
        WrapMode::JumpToNextColumn => "jumpToNextColumn",
    };
    Some(SelInfo {
        page_rect: Rect::new(b.x0 - o.x, b.y0 - o.y, b.x1 - o.x, b.y1 - o.y),
        count: st.selection.items.len(),
        fill: first.fill.swatch.clone(),
        stroke: first.stroke.swatch.clone(),
        stroke_weight: first.stroke.weight,
        stroke_align: first.stroke.align,
        opacity: first.opacity as f64,
        shadow: first.effects.drop_shadow.on,
        wrap,
        columns: first.text_frame().map(|t| t.options.columns),
        kind: first.default_label(),
        is_graphic: matches!(first.content, Content::Graphic(_)),
        is_text: first.is_text_frame(),
        stroke_kind: serde_json::to_value(&first.stroke.kind)
            .ok()
            .and_then(|v| v.get("kind").and_then(Value::as_str).map(str::to_string).or_else(|| v.as_str().map(str::to_string)))
            .unwrap_or_else(|| "solid".into()),
        corner: (first.corners.corners[0].shape, first.corners.corners[0].size),
        wrap_invert: tw.invert,
        wrap_side: serde_json::to_value(tw.side).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
        wrap_contour: serde_json::to_value(tw.contour).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
        wrap_offsets: tw.offsets,
        locked: first.locked,
    })
}

/// Resolved attributes at the text selection (or the selected text frames).
/// The fonts the active document sees: its own (its `Document Fonts` folder) ahead of the shared
/// ones.
pub fn fonts(app: &DesignApp) -> designcraft_fonts::ScopedFonts<'static> {
    designcraft_fonts::FontDb::global().scoped(font_scope(app))
}

/// The font menus' families in menu order: Western (and other scripts) first, then Japanese,
/// Simplified Chinese, Traditional Chinese and Korean, each alphabetical by the name shown (CJK
/// families by their native names unless Show Font Names in English).
/// macOS's hidden system families (named with a leading `.`) are left out.
pub fn font_menu(app: &DesignApp) -> Vec<designcraft_fonts::FamilyInfo> {
    let mut v = fonts(app).family_infos();
    v.retain(|f| !f.family.starts_with('.'));
    designcraft_fonts::sort_for_menu(&mut v, app.session.prefs.show_font_names_in_english);
    v
}

/// The name the font menus show for `family` (its native name, see [`font_menu`]).
pub fn font_label(app: &DesignApp, menu: &[designcraft_fonts::FamilyInfo], family: &str) -> String {
    let english = app.session.prefs.show_font_names_in_english;
    menu.iter().find(|f| f.family == family).map_or(family, |f| f.label(english)).to_string()
}

/// The active document's font scope (0 without a document or fonts of its own).
pub fn font_scope(app: &DesignApp) -> u32 {
    app.session.active().map_or(0, |d| d.doc.font_scope)
}

pub fn text_attrs(app: &mut DesignApp) -> Option<Value> {
    let v = app.session.execute("type.selectionAttrs", &json!({})).ok()?;
    if v.is_null() { None } else { Some(serde_json::to_value(v).ok()?) }
}

/// Count preflight errors: overset stories (more checks land with the Preflight panel).
pub fn preflight_errors(app: &DesignApp) -> usize {
    // Memoized per document snapshot (the status bar asks every frame).
    static MEMO: std::sync::Mutex<Option<(usize, usize)>> = std::sync::Mutex::new(None);
    let Some(st) = app.session.active() else { return 0 };
    let key = std::sync::Arc::as_ptr(&st.doc) as usize;
    let mut g = MEMO.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((k, n)) = *g
        && k == key
    {
        return n;
    }
    let n = designcraft_engine::cmd::preflight::check(&app.session, 150.0).iter().filter(|i| i.severity == "error").count();
    *g = Some((key, n));
    n
}

/// Height of one swatch-menu row.
const SWATCH_MENU_ROW_H: f32 = 22.0;

/// One swatch-menu row: a chip and the swatch name; the whole row is clickable (the chip included).
/// `name == current` highlights it. Returns true when the row was clicked.
/// Where a swatch row's name starts, after its chip.
const SWATCH_MENU_TEXT_X: f32 = 28.0;

fn swatch_menu_font() -> egui::FontId {
    egui::FontId::proportional(12.5)
}

/// One swatch in a swatch list, `width` wide: the swatch under the pointer is highlighted (white
/// on blue, like a menu), the current one shaded.
pub fn swatch_menu_row(ui: &mut egui::Ui, doc: &designcraft_doc::Document, name: &str, current: &str, width: f32) -> bool {
    let t = Tokens::get(ui.ctx());
    let (row, resp) = ui.allocate_exact_size(vec2(width, SWATCH_MENU_ROW_H), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, name == current, name));
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(row, 2.0, t.accent_strong);
    } else if name == current {
        ui.painter().rect_filled(row, 2.0, t.row_selected);
    }
    let (c, g) = crate::widgets::swatch_colors(doc, name, 1.0);
    let chip = egui::Rect::from_min_size(row.min + vec2(4.0, 3.0), vec2(16.0, 16.0));
    crate::widgets::paint_chip(ui.painter(), chip, c, g);
    // A name too long for the list is cut at the row's end.
    let text = ui.painter().with_clip_rect(ui.clip_rect().intersect(row.shrink2(vec2(2.0, 0.0))));
    crate::rtl::paint(
        &text,
        row.min + vec2(SWATCH_MENU_TEXT_X, SWATCH_MENU_ROW_H / 2.0),
        egui::Align2::LEFT_CENTER,
        name,
        swatch_menu_font(),
        if hovered { egui::Color32::WHITE } else { t.text },
    );
    resp.clicked()
}

/// The list in a swatch popup → the swatch picked. It is as wide as its longest name (within
/// limits), and its rows stop short of the scroll bar that floats over the list's edge.
pub fn swatch_list(ui: &mut egui::Ui, doc: &designcraft_doc::Document, current: &str) -> Option<String> {
    let font = swatch_menu_font();
    let widest = doc
        .swatches
        .iter()
        .map(|sw| ui.painter().layout_no_wrap(sw.name.clone(), font.clone(), egui::Color32::PLACEHOLDER).size().x)
        .fold(0.0, f32::max);
    let scroll = &ui.spacing().scroll;
    let bar = scroll.bar_inner_margin + scroll.bar_width + scroll.bar_outer_margin;
    let width = (SWATCH_MENU_TEXT_X + widest + 10.0 + bar).clamp(200.0, 360.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    let mut picked = None;
    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
        for sw in &doc.swatches {
            if swatch_menu_row(ui, doc, &sw.name, current, width - bar) {
                picked = Some(sw.name.clone());
                ui.close();
            }
        }
    });
    picked
}

/// A swatch dropdown showing a chip and name; `on_pick` gets the chosen swatch name. Returns the
/// chip's response (`None` without a document), for a caller to label or annotate.
pub fn swatch_picker(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    id: &str,
    current: Option<String>,
    on_pick: impl FnOnce(&mut DesignApp, String),
) -> Option<egui::Response> {
    let doc = app.session.active().map(|d| d.doc.clone())?;
    let cur = current.unwrap_or_default();
    let (c, g) = crate::widgets::swatch_colors(&doc, &cur, 1.0);
    let mut picked = None;
    let resp = ui
        .push_id(id, |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 18.0), egui::Sense::click());
            crate::widgets::paint_chip(
                ui.painter(),
                egui::Rect::from_min_size(r.min, vec2(18.0, 18.0)),
                if cur.is_empty() { Some(egui::Color32::GRAY) } else { c },
                g,
            );
            crate::icons::paint(
                ui.painter(),
                egui::Rect::from_min_size(r.min + vec2(18.0, 3.0), vec2(12.0, 12.0)),
                "chevron-down",
                Tokens::get(ui.ctx()).icon,
            );
            egui::Popup::menu(&resp).show(|ui| {
                picked = swatch_list(ui, &doc, &cur);
            });
            resp
        })
        .inner;
    if let Some(p) = picked {
        on_pick(app, p);
    }
    Some(resp)
}

/// A font family combo box showing `current` by its menu name, with [`font_menu_body`] as its menu.
/// Returns the font picked.
pub fn font_combo(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
    width: f32,
) -> Option<FontPick> {
    let menu = font_menu(app);
    let shown = if current.is_empty() { "—".to_string() } else { font_label(app, &menu, current) };
    // A field and a plain popup rather than egui's combo box: its own scroll area would keep the
    // menu at the size it last had (a family opening, a filter clearing, wouldn't show).
    let field = ui.push_id(id_salt, |ui| crate::widgets::dropdown(ui, &shown, width)).inner;
    font_popup(app, &field, &menu, current)
}

/// [`font_menu_body`] in a popup under `field` (a font field the caller draws), which screen
/// readers hear as a combo box with the current family as its value. Returns the font picked.
pub fn font_popup(app: &mut DesignApp, field: &egui::Response, menu: &[designcraft_fonts::FamilyInfo], current: &str) -> Option<FontPick> {
    let shown = if current.is_empty() { "—".to_string() } else { font_label(app, menu, current) };
    field.widget_info(|| {
        let mut info = egui::WidgetInfo::new(egui::WidgetType::ComboBox);
        info.enabled = true;
        info.current_text_value = Some(shown.clone());
        info
    });
    egui::Popup::menu(field)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| font_menu_body(app, ui, menu, current, field.rect.width()))
        .and_then(|r| r.inner)
}

/// The style a pick sets: the one picked, else the family's Regular (else its first).
pub fn pick_style(app: &DesignApp, pick: &FontPick) -> String {
    if let Some(s) = &pick.style {
        return s.clone();
    }
    let styles = fonts(app).styles(&pick.family);
    if styles.iter().any(|s| s == "Regular") { "Regular".to_string() } else { styles.first().cloned().unwrap_or_default() }
}

/// Set the text's font to the one picked (a family's Regular style unless a style was picked).
pub fn apply_font(app: &mut DesignApp, pick: &FontPick) {
    let style = pick_style(app, pick);
    let _ = app.run("type.char", json!({"attrs": {"fontFamily": pick.family, "fontStyle": style}}));
}

/// The text's font family field: [`font_combo`], applied to the text.
pub fn font_family_picker(app: &mut DesignApp, ui: &mut egui::Ui, current: &str, width: f32) {
    if let Some(f) = font_combo(app, ui, "font_family", current, width) {
        apply_font(app, &f);
    }
}

/// The style a style field shows: `—` for none; a style with axis settings is no longer its named
/// style, so it shows its axes alone (`Cd Hair {wght:620}` → `(wght:620)`).
pub fn style_shown(current: &str) -> String {
    if current.is_empty() {
        "—".to_string()
    } else if let Some((_, axes)) = current.split_once('{') {
        format!("({})", axes.trim_end_matches('}').trim())
    } else {
        current.to_string()
    }
}

pub fn font_style_picker(app: &mut DesignApp, ui: &mut egui::Ui, family: &str, current: &str, width: f32) {
    let styles = fonts(app).styles(family);
    let mut pick = None;
    egui::ComboBox::from_id_salt("font_style").selected_text(style_shown(current)).width(width).show_ui(ui, |ui| {
        for s in &styles {
            if ui.selectable_label(s == current, s).clicked() {
                pick = Some(s.clone());
            }
        }
    });
    if let Some(s) = pick {
        let _ = app.run("type.char", json!({"attrs": {"fontStyle": s}}));
    }
}

pub fn para_style_picker(app: &mut DesignApp, ui: &mut egui::Ui, current: &str, overridden: bool, width: f32) {
    let names: Vec<String> = app.session.active().map(|d| d.doc.styles.paragraph.iter().map(|p| p.name.clone()).collect()).unwrap_or_default();
    let mut pick = None;
    let shown = crate::i18n::style_name(&app.ui.language, current);
    let label = if overridden { format!("{shown}+") } else { shown.to_string() };
    egui::ComboBox::from_id_salt("para_style").selected_text(crate::rtl::widget(ui, label)).width(width).show_ui(ui, |ui| {
        for n in names.iter().filter(|n| *n != designcraft_doc::NO_PARA_STYLE) {
            if ui.selectable_label(n == current, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                pick = Some(n.clone());
            }
        }
    });
    if let Some(n) = pick {
        let _ = app.run("style.paragraph.apply", json!({"name": n, "clearOverrides": false}));
    }
}

#[cfg(test)]
mod font_menu_tests {
    use designcraft_fonts::FontGroup;
    use designcraft_fonts::testing::{font_with, with_names};

    use super::*;

    #[test]
    fn a_style_with_axis_settings_shows_its_axes_alone() {
        assert_eq!(style_shown("Cd Hair {wdth:110,wght:620,ital:1}"), "(wdth:110,wght:620,ital:1)");
        assert_eq!(style_shown("Cd Hair"), "Cd Hair");
        assert_eq!(style_shown(""), "—");
    }

    #[test]
    fn cjk_fonts_follow_the_western_ones_under_their_native_names() {
        const FAMILY: &str = "DC UI Test Mincho";
        let font = font_with(FAMILY, &['a']).unwrap();
        let font = with_names(&font, &[(3, 1, 0x409, 1, FAMILY), (3, 1, 0x409, 2, "Regular"), (3, 1, 0x0411, 1, "UIテスト明朝")]).unwrap();
        designcraft_fonts::FontDb::global().add_font(font);
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let menu = font_menu(&app);
        let at = menu.iter().position(|f| f.family == FAMILY).unwrap();
        assert_eq!(menu[at].group, FontGroup::Japanese);
        assert!(menu[..at].iter().any(|f| f.family == designcraft_fonts::DEFAULT_FAMILY), "Western fonts first");
        assert!(menu.windows(2).all(|w| w[0].group <= w[1].group), "grouped");
        assert_eq!(font_label(&app, &menu, FAMILY), "UIテスト明朝");
        assert_eq!(font_label(&app, &menu, designcraft_fonts::DEFAULT_FAMILY), designcraft_fonts::DEFAULT_FAMILY);
        app.run("prefs.set", json!({"showFontNamesInEnglish": true})).unwrap();
        assert_eq!(font_label(&app, &font_menu(&app), FAMILY), FAMILY);
    }

    #[test]
    fn hidden_system_fonts_stay_out_of_the_font_menus() {
        const HIDDEN: &str = ".DC UI Test Hidden";
        designcraft_fonts::FontDb::global().add_font(font_with(HIDDEN, &['a']).unwrap());
        let app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert!(fonts(&app).family_infos().iter().any(|f| f.family == HIDDEN), "installed");
        let menu = font_menu(&app);
        assert!(menu.iter().all(|f| !f.family.starts_with('.')), "no hidden family is listed");
        // Text that already uses one still shows its name.
        assert_eq!(font_label(&app, &menu, HIDDEN), HIDDEN);
    }
}

#[cfg(test)]
mod font_menu_ui_tests {
    use egui::vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use crate::test_window::{self, Window, click_at};

    /// The Type tool's selection over a new text frame's text, with the Control bar on. Returns the
    /// story.
    fn typing() -> (Harness<'static, Window>, u64) {
        typing_in("Advanced")
    }

    /// [`typing`] in the workspace `name`.
    fn typing_in(name: &str) -> (Harness<'static, Window>, u64) {
        typing_on(name, vec2(1440.0, 900.0), false)
    }

    /// [`typing_in`] a window of `size`, with the macOS window chrome (the native menu bar and the
    /// title bar inside the window) when `mac`.
    fn typing_on(name: &str, size: egui::Vec2, mac: bool) -> (Harness<'static, Window>, u64) {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("window.workspace", json!({ "name": name })).unwrap();
        app.native_menu = mac;
        app.integrated_titlebar = mac;
        let mut h = test_window::open(app, size);
        let app = &mut h.state_mut().app;
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hello", "caret": true})).unwrap();
        app.run("tool.select", json!({"tool": "type"})).unwrap();
        app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 5})).unwrap();
        h.run_steps(4);
        (h, r["story"].as_u64().unwrap())
    }

    fn story_text(h: &mut Harness<'static, Window>, story: u64) -> String {
        h.state_mut().app.run("story.get", json!({"story": story})).unwrap()["text"].as_str().unwrap().to_string()
    }

    /// The font fields showing `family`, top to bottom.
    fn fields(h: &Harness<'static, Window>, family: &str) -> Vec<egui::Rect> {
        let mut r: Vec<egui::Rect> =
            h.query_all_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.value().as_deref() == Some(family)).map(|n| n.rect()).collect();
        r.sort_by(|a, b| a.min.y.total_cmp(&b.min.y).then(a.min.x.total_cmp(&b.min.x)));
        r
    }

    /// The Control bar's font field (the topmost), showing `family`.
    fn field(h: &Harness<'static, Window>, family: &str) -> egui::Rect {
        *fields(h, family).first().unwrap_or_else(|| panic!("a font field shows {family}"))
    }

    /// The text in the focused search field.
    fn search(h: &Harness<'static, Window>) -> String {
        h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.is_focused()).value().unwrap_or_default()
    }

    fn family(h: &mut Harness<'static, Window>) -> String {
        h.state_mut().app.run("type.selectionAttrs", json!({})).unwrap()["chars"]["fontFamily"].as_str().unwrap().to_string()
    }

    /// The first matching family in this machine's font catalog. Installed variable fonts
    /// can add more matches beside the bundled families.
    fn first_match(h: &Harness<'static, Window>, query: &str) -> String {
        super::font_menu(&h.state().app).into_iter().find(|f| f.matches(query)).expect("a matching family").family
    }

    /// Run the menu until the filters' font kinds are read (it says Loading fonts… meanwhile).
    fn settle(h: &mut Harness<'static, Window>) {
        for _ in 0..3000 {
            if h.query_by_label("Loading fonts…").is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.run_steps(1);
        }
        panic!("the font kinds were never read");
    }

    /// Open the Control bar's font menu (showing `family`) and search for `query`.
    fn open_menu(h: &mut Harness<'static, Window>, family: &str, query: &str) {
        let at = field(h, family);
        click_at(h, at.center());
        if !query.is_empty() {
            h.event(egui::Event::Text(query.into()));
        }
        h.run_steps(4);
    }

    #[test]
    fn a_family_opens_to_its_styles_and_a_style_is_picked() {
        let (mut h, _) = typing();
        let serif = designcraft_fonts::DEFAULT_FAMILY;
        open_menu(&mut h, serif, "serif 4");
        // The arrow before the name opens the family; the menu stays open.
        {
            let at = h.get_by_label("Show Styles").rect().center();
            click_at(&mut h, at);
        }
        assert!(h.query_by_label("Bold Italic").is_some(), "its styles are listed");
        {
            let at = h.get_by_label("Bold Italic").rect().center();
            click_at(&mut h, at);
        }
        let attrs = h.state_mut().app.run("type.selectionAttrs", json!({})).unwrap();
        assert_eq!((attrs["chars"]["fontFamily"].as_str(), attrs["chars"]["fontStyle"].as_str()), (Some(serif), Some("Bold Italic")));
        assert!(!h.ctx.text_edit_focused(), "picking closes the menu");
    }

    #[test]
    fn picked_fonts_come_first_as_recently_used() {
        let (mut h, _) = typing();
        open_menu(&mut h, designcraft_fonts::DEFAULT_FAMILY, "source sans");
        {
            let at = h.get_by_label("Source Sans 3").rect().center();
            click_at(&mut h, at);
        }
        assert_eq!(h.state().app.session.prefs.recent_fonts, ["Source Sans 3"]);
        // Reopened without a search: Source Sans 3 is listed at the top, and again in its place (on
        // screen too when few fonts are installed).
        open_menu(&mut h, "Source Sans 3", "");
        let recent =
            h.query_all_by_label("Source Sans 3").map(|n| n.rect()).min_by(|a, b| a.min.y.total_cmp(&b.min.y)).expect("Source Sans 3 is listed");
        let search = h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.is_focused()).rect();
        assert!(recent.min.y > search.max.y && recent.min.y - search.max.y < 80.0, "the first row, under the filters: {recent:?} vs {search:?}");
    }

    #[test]
    fn the_class_filter_and_similar_fonts_narrow_the_list() {
        let (mut h, _) = typing();
        open_menu(&mut h, designcraft_fonts::DEFAULT_FAMILY, "");
        {
            let at = h.get_by(|n| n.label().is_some_and(|l| l.starts_with("All Classes"))).rect().center();
            click_at(&mut h, at);
        }
        {
            let at = h.get_by_label("Monospaced").rect().center();
            click_at(&mut h, at);
        }
        settle(&mut h);
        assert!(h.query_by_label("JetBrains Mono").is_some(), "a monospaced family is listed");
        assert!(h.query_by_label(designcraft_fonts::DEFAULT_FAMILY).is_none(), "a serif isn't");
        // Back to all classes, then fonts like the serif: it leads, other serifs follow, no sans.
        {
            let at = h.get_by(|n| n.label().is_some_and(|l| l.starts_with("Monospaced ▾"))).rect().center();
            click_at(&mut h, at);
        }
        {
            let at = h.get_by_label("All Classes").rect().center();
            click_at(&mut h, at);
        }
        h.event(egui::Event::Text("serif 4".into()));
        h.run_steps(4);
        let row = h.get_by_label(designcraft_fonts::DEFAULT_FAMILY).rect();
        h.hover_at(row.center());
        h.run_steps(2);
        {
            let at = h.get_by_label("Show Similar Fonts").rect().center();
            click_at(&mut h, at);
        }
        assert!(h.query_by_label(&format!("Similar to {}", designcraft_fonts::DEFAULT_FAMILY)).is_some());
        assert!(h.query_by_label("Source Sans 3").is_none() && h.query_by_label("JetBrains Mono").is_none(), "other classes aren't similar");
        assert!(h.query_by_label(designcraft_fonts::DEFAULT_FAMILY).is_some());
    }

    #[test]
    fn every_font_field_reports_as_a_combo_box_showing_the_family() {
        let (h, _) = typing();
        let shown = fields(&h, designcraft_fonts::DEFAULT_FAMILY);
        // The Control bar's, and the hand-drawn ones (Properties, the Contextual Task Bar).
        assert!(shown.len() >= 2, "{shown:?}");
    }

    #[test]
    fn typing_in_the_font_menu_searches_and_never_reaches_the_story() {
        let (mut h, story) = typing();
        let before = story_text(&mut h, story);
        let at = field(&h, designcraft_fonts::DEFAULT_FAMILY);
        click_at(&mut h, at.center());
        assert!(h.ctx.text_edit_focused(), "the search field takes the keyboard when the menu opens");
        h.event(egui::Event::Text("source".into()));
        h.run_steps(4);
        assert_eq!(search(&h), "source");
        assert_eq!(story_text(&mut h, story), before, "the typing went to the search, not the story");
        assert!(h.query_by_label("Source Sans 3").is_some(), "a match is listed");
        assert!(h.query_by_label(designcraft_fonts::DEFAULT_FAMILY).is_some(), "both bundled Source families match");
        assert!(h.query_by_label("JetBrains Mono").is_none(), "families that don't match aren't");
        // Typing highlights the first match. ↓ then ↑ exercise navigation and return to it.
        let expected = first_match(&h, "source");
        assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(0));
        h.key_press(egui::Key::ArrowDown);
        h.run_steps(2);
        assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(1), "Down highlights the next family");
        h.key_press(egui::Key::ArrowUp);
        h.run_steps(2);
        assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(0), "Up restores the first family");
        h.key_press(egui::Key::Enter);
        h.run_steps(4);
        assert_eq!(family(&mut h), expected);
        assert_eq!(story_text(&mut h, story), before);
        assert!(!h.ctx.text_edit_focused(), "the menu closed");
        // Reopened, the search starts empty.
        let at = field(&h, &expected);
        click_at(&mut h, at.center());
        assert_eq!(search(&h), "", "the search starts empty");
        // A click on a family picks it.
        h.event(egui::Event::Text("serif 4".into()));
        h.run_steps(4);
        let row = h.get_by_label(designcraft_fonts::DEFAULT_FAMILY).rect();
        click_at(&mut h, row.center());
        assert_eq!(family(&mut h), designcraft_fonts::DEFAULT_FAMILY);
        // Escape closes the menu and leaves the text as it was.
        let at = field(&h, designcraft_fonts::DEFAULT_FAMILY);
        click_at(&mut h, at.center());
        // A star marks a favourite and keeps the menu open.
        h.event(egui::Event::Text("serif 4".into()));
        h.run_steps(4);
        let row = h.get_by_label(designcraft_fonts::DEFAULT_FAMILY).rect();
        click_at(&mut h, row.min + vec2(10.0, 12.0));
        assert!(h.state().app.session.prefs.favorite_fonts.iter().any(|f| f == designcraft_fonts::DEFAULT_FAMILY));
        assert_eq!(search(&h), "serif 4", "the menu stays open");
        h.key_press(egui::Key::Escape);
        h.run_steps(4);
        assert!(!h.ctx.text_edit_focused(), "the menu closed");
        assert_eq!(story_text(&mut h, story), before);
        assert!(h.state().app.session.active().unwrap().selection.text.is_some(), "the text is still selected");
    }

    /// Opens the font menu of the field at `at`, clicks its search field, types, checks the menu
    /// stays open and lists only the matches, then picks the first with ↓, ↑ and Return.
    fn search_in_menu(h: &mut Harness<'static, Window>, at: egui::Rect, what: &str) -> String {
        click_at(h, at.center());
        let field = h.query_by(|n| n.role() == egui::accesskit::Role::TextInput && n.is_focused()).map(|n| n.rect());
        let field = field.unwrap_or_else(|| panic!("{what}: the menu opens with the search field focused"));
        click_at(h, field.center());
        assert!(h.ctx.text_edit_focused(), "{what}: a click on the search field keeps the menu open");
        h.event(egui::Event::Text("SOURCE".into()));
        h.run_steps(4);
        assert_eq!(search(h), "SOURCE", "{what}: the typing went to the search");
        assert!(h.query_by_label("Source Sans 3").is_some(), "{what}: a match is listed");
        assert!(h.query_by_label(designcraft_fonts::DEFAULT_FAMILY).is_some(), "{what}: both bundled Source families match");
        assert!(h.query_by_label("JetBrains Mono").is_none(), "{what}: families that don't match aren't");
        let expected = first_match(h, "source");
        assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(0), "{what}: typing highlights the first family");
        h.key_press(egui::Key::ArrowDown);
        h.run_steps(2);
        assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(1), "{what}: Down highlights the next family");
        h.key_press(egui::Key::ArrowUp);
        h.run_steps(2);
        assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(0), "{what}: Up restores the first family");
        h.key_press(egui::Key::Enter);
        h.run_steps(4);
        assert!(!h.ctx.text_edit_focused(), "{what}: the menu closed");
        expected
    }

    /// Every font field over selected text (Control bar, Contextual Task Bar, Properties ›
    /// Character, Character panel) searches, in the workspaces that show them and in a small
    /// window with the macOS window chrome.
    #[test]
    fn every_font_field_searches_in_every_workspace() {
        for (name, size, mac, panel) in [
            ("Advanced", vec2(1440.0, 900.0), false, None),
            ("Printing and Proofing", vec2(1440.0, 900.0), false, None),
            ("Essentials", vec2(1440.0, 900.0), false, Some("character")),
            ("Printing and Proofing", vec2(900.0, 560.0), true, None),
        ] {
            let (mut h, story) = typing_on(name, size, mac);
            if panel.is_some() {
                h.state_mut().app.ui.open_panel = panel.map(str::to_string);
                h.run_steps(4);
            }
            let name = &format!("{name} {size:?} {panel:?}");
            let before = story_text(&mut h, story);
            let count = fields(&h, designcraft_fonts::DEFAULT_FAMILY).len();
            assert_eq!(count, 3, "{name}: the font fields are shown");
            for k in 0..count {
                h.state_mut().app.run("type.char", json!({"attrs": {"fontFamily": designcraft_fonts::DEFAULT_FAMILY}})).unwrap();
                h.run_steps(4);
                let at = fields(&h, designcraft_fonts::DEFAULT_FAMILY)[k];
                let expected = search_in_menu(&mut h, at, &format!("{name}, field {k}"));
                assert_eq!(family(&mut h), expected, "{name}, field {k}");
                assert_eq!(story_text(&mut h, story), before, "{name}, field {k}");
            }
        }
    }

    /// The font fields away from the text: the Glyphs panel, Paragraph Style Options › Basic
    /// Character Formats and Find/Replace Font's Replace With.
    #[test]
    fn font_fields_in_panels_and_dialogs_search() {
        let paragraph = json!({"name": designcraft_doc::BASIC_PARAGRAPH, "section": "chars"});
        for (what, panel, dialog, shown) in [
            ("Glyphs", Some("glyphs"), None, designcraft_fonts::DEFAULT_FAMILY),
            ("Paragraph Style Options", None, Some(("paragraphStyleOptions", paragraph)), designcraft_fonts::DEFAULT_FAMILY),
            (
                "Find/Replace Font",
                None,
                Some(("findFont", json!({"toFamily": designcraft_fonts::DEFAULT_FAMILY}))),
                designcraft_fonts::DEFAULT_FAMILY,
            ),
        ] {
            let app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            let mut h = test_window::open(app, vec2(1440.0, 900.0));
            let dialog_id = dialog.as_ref().map(|(id, _)| id.to_string());
            h.state_mut().app.ui.open_panel = panel.map(str::to_string);
            h.state_mut().app.ui.dialog = dialog.map(|(id, fields)| crate::dialogs::Dialog::new(id, fields));
            h.run_steps(4);
            let at = *fields(&h, shown).first().unwrap_or_else(|| panic!("{what}: the font field is shown"));
            let expected = search_in_menu(&mut h, at, what);
            assert_eq!(h.state().app.ui.dialog.as_ref().map(|d| d.id.clone()), dialog_id, "{what}: Return in the menu leaves the dialog open");
            assert!(!fields(&h, &expected).is_empty(), "{what}: the field shows the family picked");
            // Escape closes the menu only.
            let at = fields(&h, &expected)[0];
            click_at(&mut h, at.center());
            assert!(h.ctx.text_edit_focused(), "{what}: the menu opened");
            h.key_press(egui::Key::Escape);
            h.run_steps(4);
            assert!(!h.ctx.text_edit_focused(), "{what}: Escape closed the menu");
            assert_eq!(h.state().app.ui.dialog.as_ref().map(|d| d.id.clone()), dialog_id, "{what}: Escape in the menu leaves the dialog open");
        }
    }
}

#[cfg(test)]
mod swatch_list_tests {
    use super::*;

    fn rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<(egui::Rect, egui::Color32)> {
        fn walk(s: &egui::Shape, out: &mut Vec<(egui::Rect, egui::Color32)>) {
            match s {
                egui::Shape::Rect(r) => out.push((r.rect, r.fill)),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = vec![];
        shapes.iter().for_each(|s| walk(&s.shape, &mut out));
        out
    }

    fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
        fn walk(s: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match s {
                egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.visual_bounding_rect())),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = vec![];
        shapes.iter().for_each(|s| walk(&s.shape, &mut out));
        out
    }

    #[test]
    fn the_swatch_under_the_pointer_is_highlighted_and_names_clear_the_scroll_bar() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", serde_json::json!({})).unwrap();
        // A long name, as process swatches have.
        app.run("swatch.create", serde_json::json!({"name": "C=15 M=100 Y=100 K=0 Long Name", "color": {"c": 15, "m": 100, "y": 100, "k": 0}}))
            .unwrap();
        let long = "C=15 M=100 Y=100 K=0 Long Name";
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        let tokens = Tokens::for_brightness(crate::theme::Brightness::MediumDark);
        crate::theme::apply(&ctx, &tokens);
        let mut time = 0.0;
        let mut frame = |app: &mut DesignApp, events: Vec<egui::Event>| {
            time += 0.1;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 700.0))),
                time: Some(time),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                swatch_picker(app, ui, "list", Some("[Black]".into()), |_, _| {});
            });
            out.textures_delta.clear();
            out.shapes
        };
        frame(&mut app, vec![]);
        let chip = egui::pos2(17.0, 17.0);
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(chip),
                    egui::Event::PointerButton { pos: chip, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
                ],
            );
        }
        let shapes = frame(&mut app, vec![]);
        let (_, name) = texts(&shapes).into_iter().find(|(t, _)| t == "[Registration]").expect("the list is open");
        let pointer = name.center();
        frame(&mut app, vec![egui::Event::PointerMoved(pointer)]);
        let shapes = frame(&mut app, vec![]);
        let all = rects(&shapes);
        let (row, _) = *all.iter().find(|(r, c)| *c == tokens.accent_strong && r.contains(pointer)).expect("the hovered row is highlighted");
        let (list, _) = *all
            .iter()
            .filter(|(r, c)| *c == tokens.panel && r.contains_rect(row))
            .min_by(|a, b| a.0.area().total_cmp(&b.0.area()))
            .expect("the popup");
        let scroll = ctx.global_style().spacing.scroll;
        assert!(row.right() + scroll.bar_width <= list.right(), "rows {row:?} stop short of the scroll bar in {list:?}");
        let (_, long_rect) = texts(&shapes).into_iter().find(|(t, _)| t == long).unwrap();
        assert!(long_rect.right() <= row.right(), "the longest name {long_rect:?} fits its row {row:?}");
        assert!(texts(&shapes).iter().any(|(t, r)| t == "[Black]" && r.right() <= row.right()));
    }
}

#[cfg(test)]
mod content_selection_tests {
    use serde_json::json;

    /// With a placed graphic selected (Direct Selection), X/Y/W/H are the graphic's: what the
    /// Transform fields then set.
    #[test]
    fn a_selected_graphic_shows_its_own_bounds() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let f = app.run("frame.create", json!({"rect": [100, 100, 300, 300], "content": "graphic"})).unwrap()["id"].clone();
        let png = designcraft_render::Rendered { width: 400, height: 400, pixels: vec![200u8; 400 * 400 * 4] }.to_png();
        let b64 = designcraft_engine::cmd::base64_encode(&png);
        app.run("file.place", json!({"base64": b64, "name": "a.png", "frame": f})).unwrap();
        app.run("selection.set", json!({"ids": [f]})).unwrap();
        let frame = super::sel_info(&app).unwrap().page_rect;
        assert!((frame.width() - 200.0).abs() < 1e-6, "{frame:?}");
        app.run("selection.set", json!({"ids": [f], "content": true})).unwrap();
        // Placing fitted it to 50%: back to its own 400 pt.
        app.run("transform.set", json!({"scaleX": 100, "scaleY": 100})).unwrap();
        let g = super::sel_info(&app).unwrap().page_rect;
        assert!((g.width() - 400.0).abs() < 1e-6, "the graphic's width, not the frame's: {g:?}");
        // So a width typed from it scales the graphic by that ratio, not by half.
        app.run("transform.set", json!({"width": 401})).unwrap();
        let after = super::sel_info(&app).unwrap().page_rect;
        assert!((after.width() - 401.0).abs() < 1e-6, "{after:?}");
    }
}
