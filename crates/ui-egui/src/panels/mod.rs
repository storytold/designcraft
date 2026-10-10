//! Panels and shared panel helpers.

pub mod conditions;
pub mod datamerge;
pub mod glyphs;
pub mod interactive;
pub mod layers;
pub mod library;
pub mod notes;
pub mod pages;
pub mod properties;
pub mod styles;
pub mod swatches;
pub mod table;

use designcraft_doc::{Content, StrokeAlign, WrapMode};
use designcraft_geom::Rect;
use egui::vec2;
use serde_json::{Value, json};

use crate::DesignApp;
use crate::theme::Tokens;

/// Summary of the current object selection (for the Control panel and Properties).
#[derive(Clone, Debug)]
pub struct SelInfo {
    /// Bounds relative to the page the selection is on.
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
    pub locked: bool,
}

/// Fill (or stroke) tint of the first selected item (1 = 100%).
pub fn sel_tint(app: &DesignApp, stroke: bool) -> f32 {
    let Some(st) = app.session.active() else { return 1.0 };
    st.selection.items.first().and_then(|id| st.doc.item(*id)).map_or(1.0, |it| if stroke { it.stroke.tint } else { it.fill.tint })
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
        let ib = if with_stroke { it.visible_bounds() } else { it.bounds() };
        b = Some(b.map_or(ib, |r| r.union(ib)));
    }
    let b = b?;
    let first = d.item(st.selection.items[0])?;
    let loc = d.find(first.id)?;
    let sp = d.spread(loc.spread)?;
    let pi = sp.page_at_x(b.center().x).unwrap_or(0);
    let px = sp.pages.get(pi).map(|p| p.x).unwrap_or(0.0);
    let wrap = match first.wrap.mode {
        WrapMode::None => "none",
        WrapMode::BoundingBox => "boundingBox",
        WrapMode::Contour => "contour",
        WrapMode::JumpObject => "jumpObject",
        WrapMode::JumpToNextColumn => "jumpToNextColumn",
    };
    Some(SelInfo {
        page_rect: Rect::new(b.x0 - px, b.y0, b.x1 - px, b.y1),
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
        wrap_invert: first.wrap.invert,
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
    let mut g = MEMO.lock().unwrap_or_else(|e| e.into_inner());
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
pub fn swatch_menu_row(ui: &mut egui::Ui, doc: &designcraft_doc::Document, name: &str, current: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), SWATCH_MENU_ROW_H), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, name == current, name));
    if name == current {
        ui.painter().rect_filled(row, 0.0, t.row_selected);
    } else if resp.hovered() {
        ui.painter().rect_filled(row, 0.0, t.hover);
    }
    let (c, g) = crate::widgets::swatch_colors(doc, name, 1.0);
    let chip = egui::Rect::from_min_size(row.min + vec2(4.0, 3.0), vec2(16.0, 16.0));
    crate::widgets::paint_chip(ui.painter(), chip, c, g);
    crate::rtl::paint(
        ui.painter(),
        row.min + vec2(28.0, SWATCH_MENU_ROW_H / 2.0),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(12.5),
        t.text,
    );
    resp.clicked()
}

/// A swatch dropdown showing a chip and name; `on_pick` gets the chosen swatch name.
pub fn swatch_picker(app: &mut DesignApp, ui: &mut egui::Ui, id: &str, current: Option<String>, on_pick: impl FnOnce(&mut DesignApp, String)) {
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let cur = current.unwrap_or_default();
    let (c, g) = crate::widgets::swatch_colors(&doc, &cur, 1.0);
    let mut picked = None;
    ui.push_id(id, |ui| {
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
            ui.set_min_width(200.0);
            ui.set_max_width(280.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for sw in &doc.swatches {
                    if swatch_menu_row(ui, &doc, &sw.name, &cur) {
                        picked = Some(sw.name.clone());
                        ui.close();
                    }
                }
            });
        });
    });
    if let Some(p) = picked {
        on_pick(app, p);
    }
}

/// A five-pointed star (filled, or outlined).
fn paint_star(p: &egui::Painter, c: egui::Pos2, r: f32, filled: bool, color: egui::Color32) {
    let pt = |i: usize, rad: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        c + egui::vec2(a.cos(), a.sin()) * rad
    };
    let outline: Vec<egui::Pos2> = (0..10).map(|i| pt(i, if i % 2 == 0 { r } else { r * 0.42 })).collect();
    if filled {
        // The inner pentagon and the five points (each convex).
        let inner: Vec<egui::Pos2> = (0..5).map(|i| outline[2 * i + 1]).collect();
        p.add(egui::Shape::convex_polygon(inner, color, egui::Stroke::NONE));
        for i in 0..5 {
            p.add(egui::Shape::convex_polygon(vec![outline[(2 * i + 9) % 10], outline[2 * i], outline[2 * i + 1]], color, egui::Stroke::NONE));
        }
    } else {
        p.add(egui::Shape::closed_line(outline, egui::Stroke::new(1.0, color)));
    }
}

/// The font menu's state while it is open: the search, Show Favorites Only, the match ↑/↓ highlight
/// and the frame it was last shown (a menu not shown the frame before has just opened).
#[derive(Clone, Default)]
struct FontMenuState {
    query: String,
    only_favs: bool,
    highlight: Option<usize>,
    frame: Option<u64>,
}

/// The Font menu, inside any font field's popup or combo box: a search field (it has the keyboard
/// while the menu is open, and starts empty each time it opens), favourites (★, Show Favorites
/// Only), and each family's name with a sample in that family, the script groups apart. ↑/↓ move
/// through the matches and Return picks one. Returns the family picked and closes the menu; the
/// search, Favorites and the stars keep it open (the popup closes on a click outside only).
pub fn font_menu_body(app: &mut DesignApp, ui: &mut egui::Ui, menu: &[designcraft_fonts::FamilyInfo], current: &str, width: f32) -> Option<String> {
    let (fonts, scope) = (fonts(app), font_scope(app));
    let english = app.session.prefs.show_font_names_in_english;
    let mut favs = app.session.prefs.favorite_fonts.clone();
    let mut pick = None;
    let mut favs_changed = false;
    let state_id = egui::Id::new("font_menu_state");
    let mut st: FontMenuState = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let frame = ui.ctx().cumulative_frame_nr();
    if st.frame.is_none_or(|f| f.saturating_add(1) < frame) {
        st.query.clear();
        st.highlight = None;
    }
    st.frame = Some(frame);
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.set_width(width.max(300.0));
    let search = ui
        .horizontal(|ui| {
            let search = ui.add(
                egui::TextEdit::singleline(&mut st.query)
                    .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Search fonts")))
                    .desired_width(200.0)
                    // Return picks the highlighted match instead of leaving the field.
                    .return_key(None),
            );
            ui.toggle_value(&mut st.only_favs, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Favorites"))).on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Show Favorites Only"));
            });
            search
        })
        .inner;
    // Typing always goes to the search, never to the document under the menu.
    if !search.has_focus() {
        search.request_focus();
    }
    let q = st.query.to_lowercase();
    let shown: Vec<&designcraft_fonts::FamilyInfo> = menu.iter().filter(|f| f.matches(&q) && (!st.only_favs || favs.contains(&f.family))).collect();
    if search.changed() {
        st.highlight = if q.is_empty() { None } else { Some(0) };
    }
    let (down, up, enter) = ui.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::Enter)));
    let last = shown.len().checked_sub(1);
    st.highlight = match (st.highlight, last) {
        (_, None) => None,
        (None, Some(_)) if down => Some(0),
        (Some(h), Some(l)) if down => Some((h + 1).min(l)),
        (Some(h), Some(l)) if up => Some(h.saturating_sub(1).min(l)),
        (h, Some(l)) => h.map(|h| h.min(l)),
    };
    if enter && let Some(f) = st.highlight.and_then(|h| shown.get(h)) {
        pick = Some(f.family.clone());
    }
    let ppp = ui.ctx().pixels_per_point();
    egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
        let mut group = None;
        for (k, info) in shown.iter().enumerate() {
            // A separator between the Western fonts and each CJK language's.
            if group.is_some_and(|g| g != info.group) {
                ui.separator();
            }
            group = Some(info.group);
            let f = &info.family;
            let label = info.label(english);
            let (row, mut resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
            let highlighted = st.highlight == Some(k);
            if highlighted && (down || up) {
                ui.scroll_to_rect(row, None);
            }
            if !ui.is_rect_visible(row) {
                continue;
            }
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, f == current, label));
            if label != f {
                resp = resp.on_hover_text(f);
            }
            if f == current {
                ui.painter().rect_filled(row, 0.0, t.row_selected);
            } else if highlighted || resp.hovered() {
                ui.painter().rect_filled(row, 0.0, t.hover);
            }
            // Favourite star.
            let star = egui::Rect::from_min_size(row.min + egui::vec2(2.0, 4.0), egui::vec2(16.0, 16.0));
            let fav = favs.contains(f);
            let sr = ui.interact(star, ui.id().with(("fav", f)), egui::Sense::click());
            paint_star(ui.painter(), star.center(), 6.5, fav, if fav { t.accent } else { t.text_dim });
            if sr.clicked() {
                if fav {
                    favs.retain(|x| x != f);
                } else {
                    favs.push(f.clone());
                }
                favs_changed = true;
            }
            ui.painter().text(row.min + egui::vec2(22.0, 12.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.0), t.text);
            // The name in its own face (rendered once per family and scale).
            let key = egui::Id::new(("font_preview", f, scope, (ppp * 100.0) as u32, t.text.to_array()));
            let tex: Option<egui::TextureHandle> = ui.data(|d| d.get_temp(key));
            let tex = tex.unwrap_or_else(|| {
                let img = designcraft_render::glyphs::text_line(&fonts, f, "Regular", "Sample", (18.0 * ppp) as u32, t.text.to_array());
                let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
                let h = crate::widgets::load_texture(ui.ctx(), format!("font_preview_{f}"), ci, egui::TextureOptions::LINEAR);
                ui.data_mut(|d| d.insert_temp(key, h.clone()));
                h
            });
            let size = tex.size_vec2() / ppp;
            let at = egui::pos2(row.max.x - size.x - 4.0, row.center().y - size.y / 2.0);
            ui.painter().image(
                tex.id(),
                egui::Rect::from_min_size(at, size),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            if resp.clicked() && !sr.clicked() {
                pick = Some(f.clone());
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(state_id, st));
    if favs_changed {
        let _ = app.run("prefs.set", json!({ "favoriteFonts": favs }));
    }
    if pick.is_some() {
        ui.close();
    }
    pick
}

/// A font family combo box showing `current` by its menu name, with [`font_menu_body`] as its menu.
/// Returns the family picked.
pub fn font_combo(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
    width: f32,
) -> Option<String> {
    let menu = font_menu(app);
    let shown = if current.is_empty() { "—".to_string() } else { font_label(app, &menu, current) };
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(shown)
        .width(width)
        .height(440.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| font_menu_body(app, ui, &menu, current, width))
        .inner
        .flatten()
}

/// [`font_menu_body`] in a popup under `field` (a font field the caller draws). Returns the family
/// picked.
pub fn font_popup(app: &mut DesignApp, field: &egui::Response, menu: &[designcraft_fonts::FamilyInfo], current: &str) -> Option<String> {
    egui::Popup::menu(field)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| font_menu_body(app, ui, menu, current, field.rect.width()))
        .and_then(|r| r.inner)
}

/// Set the text's font to `family`, in its Regular style (else its first).
pub fn apply_font_family(app: &mut DesignApp, family: &str) {
    let styles = fonts(app).styles(family);
    let style = if styles.iter().any(|s| s == "Regular") { "Regular".to_string() } else { styles.first().cloned().unwrap_or_default() };
    let _ = app.run("type.char", json!({"attrs": {"fontFamily": family, "fontStyle": style}}));
}

/// The text's font family field: [`font_combo`], applied to the text.
pub fn font_family_picker(app: &mut DesignApp, ui: &mut egui::Ui, current: &str, width: f32) {
    if let Some(f) = font_combo(app, ui, "font_family", current, width) {
        apply_font_family(app, &f);
    }
}

pub fn font_style_picker(app: &mut DesignApp, ui: &mut egui::Ui, family: &str, current: &str, width: f32) {
    let styles = fonts(app).styles(family);
    let mut pick = None;
    egui::ComboBox::from_id_salt("font_style").selected_text(if current.is_empty() { "—" } else { current }).width(width).show_ui(ui, |ui| {
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
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.control_bar = true;
        let mut h = test_window::open(app, vec2(1440.0, 900.0));
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

    /// The Control bar's font field, showing `family`.
    fn field(h: &Harness<'static, Window>, family: &str) -> egui::Rect {
        h.get_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.value().as_deref() == Some(family)).rect()
    }

    /// The text in the focused search field.
    fn search(h: &Harness<'static, Window>) -> String {
        h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.is_focused()).value().unwrap_or_default()
    }

    fn family(h: &mut Harness<'static, Window>) -> String {
        h.state_mut().app.run("type.selectionAttrs", json!({})).unwrap()["chars"]["fontFamily"].as_str().unwrap().to_string()
    }

    #[test]
    fn typing_in_the_font_menu_searches_and_never_reaches_the_story() {
        let (mut h, story) = typing();
        let before = story_text(&mut h, story);
        let at = field(&h, designcraft_fonts::DEFAULT_FAMILY);
        click_at(&mut h, at.center());
        assert!(h.ctx.text_edit_focused(), "the search field takes the keyboard when the menu opens");
        h.event(egui::Event::Text("source sans".into()));
        h.run_steps(4);
        assert_eq!(search(&h), "source sans");
        assert_eq!(story_text(&mut h, story), before, "the typing went to the search, not the story");
        assert!(h.query_by_label("Source Sans 3").is_some(), "a match is listed");
        assert!(h.query_by_label(designcraft_fonts::DEFAULT_FAMILY).is_none(), "families that don't match aren't");
        // ↓ and Return pick the first match and close the menu.
        h.key_press(egui::Key::ArrowDown);
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(4);
        assert_eq!(family(&mut h), "Source Sans 3");
        assert_eq!(story_text(&mut h, story), before);
        assert!(!h.ctx.text_edit_focused(), "the menu closed");
        // Reopened, the search starts empty.
        let at = field(&h, "Source Sans 3");
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
}
