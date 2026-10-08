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
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for sw in &doc.swatches {
                    let (c, g) = crate::widgets::swatch_colors(&doc, &sw.name, 1.0);
                    let row = ui.horizontal(|ui| {
                        let (cr, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                        crate::widgets::paint_chip(ui.painter(), cr, c, g);
                        ui.add(egui::Button::new(&sw.name).frame(false).selected(sw.name == cur))
                    });
                    if row.inner.clicked() {
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

/// The Font menu: search, favourites (★, Show Favorites Only), and each family's name shown in
/// that family.
pub fn font_family_picker(app: &mut DesignApp, ui: &mut egui::Ui, current: &str, width: f32) {
    let fams = designcraft_fonts::FontDb::global().families();
    let mut favs = app.session.prefs.favorite_fonts.clone();
    let mut pick = None;
    let mut favs_changed = false;
    let state_id = egui::Id::new("font_menu_state");
    let (mut query, mut only_favs): (String, bool) = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let t = crate::theme::Tokens::get(ui.ctx());
    egui::ComboBox::from_id_salt("font_family").selected_text(if current.is_empty() { "—" } else { current }).width(width).height(420.0).show_ui(
        ui,
        |ui| {
            ui.set_min_width(300.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Search fonts")))
                        .desired_width(200.0),
                );
                ui.toggle_value(&mut only_favs, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Favorites"))).on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Show Favorites Only"));
                });
            });
            let q = query.to_lowercase();
            let ppp = ui.ctx().pixels_per_point();
            let shown: Vec<&String> =
                fams.iter().filter(|f| (q.is_empty() || f.to_lowercase().contains(&q)) && (!only_favs || favs.contains(f))).collect();
            for f in shown {
                let (row, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
                if !ui.is_rect_visible(row) {
                    continue;
                }
                if f == current {
                    ui.painter().rect_filled(row, 0.0, t.row_selected);
                } else if resp.hovered() {
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
                ui.painter().text(row.min + egui::vec2(22.0, 12.0), egui::Align2::LEFT_CENTER, f, egui::FontId::proportional(12.0), t.text);
                // The name in its own face (rendered once per family and scale).
                let key = egui::Id::new(("font_preview", f, (ppp * 100.0) as u32, t.text.to_array()));
                let tex: Option<egui::TextureHandle> = ui.data(|d| d.get_temp(key));
                let tex = tex.unwrap_or_else(|| {
                    let img = designcraft_render::glyphs::text_line(f, "Regular", "Sample", (18.0 * ppp) as u32, t.text.to_array());
                    let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
                    let h = ui.ctx().load_texture(format!("font_preview_{f}"), ci, egui::TextureOptions::LINEAR);
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
                if resp.clicked() {
                    pick = Some(f.clone());
                }
            }
        },
    );
    ui.data_mut(|d| d.insert_temp(state_id, (query, only_favs)));
    if favs_changed {
        let _ = app.run("prefs.set", json!({ "favoriteFonts": favs }));
    }
    if let Some(f) = pick {
        let styles = designcraft_fonts::FontDb::global().styles(&f);
        let style = if styles.iter().any(|s| s == "Regular") { "Regular".to_string() } else { styles.first().cloned().unwrap_or_default() };
        let _ = app.run("type.char", json!({"attrs": {"fontFamily": f, "fontStyle": style}}));
    }
}

pub fn font_style_picker(app: &mut DesignApp, ui: &mut egui::Ui, family: &str, current: &str, width: f32) {
    let styles = designcraft_fonts::FontDb::global().styles(family);
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
