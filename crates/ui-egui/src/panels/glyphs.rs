//! Glyphs panel: every character of a font in a grid; click to insert it at the text cursor. The
//! grid also picks a list's bullet character.

use egui::{Sense, vec2};
use serde_json::json;

use crate::DesignApp;
use crate::theme::Tokens;

const CELL: f32 = 30.0;
/// Characters shown at most (the rest via the search field).
const MAX: usize = 1200;

#[derive(Clone, Default)]
struct State {
    family: String,
    style: String,
    query: String,
    recent: Vec<char>,
}

// Only this panel's style, query and recent action faces. The exported grid and shared family
// chooser remain unchanged and retain their own future SDK migration boundary.
fn panel_style(ui: &mut egui::Ui, language: &str, framework: bool, family: &str, current: &mut String, styles: Vec<String>) {
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _};
        use craft_ui_widgets::pickers::Select;

        let label = crate::i18n::tr(language, "Font Style");
        let entries: Vec<_> = styles.into_iter().map(|style| Entry::new(style.clone(), style)).collect();
        let (rect, _) = ui.allocate_exact_size(vec2(80.0, 24.0), Sense::hover());
        let placeholder = current.clone();
        Select::new(label, Bind::bound(current), &entries).id_salt(("glyph_style", family)).placeholder(placeholder).tooltip(label).show_at(ui, rect);
        return;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (framework, family);
    let combo = craft_ui::ComboBox::from_id_salt("glyph_style").selected_text(&*current).width(80.0).show_ui(ui, |ui| {
        for style in styles {
            if ui.selectable_label(style == *current, &style).clicked() {
                *current = style;
            }
        }
    });
    let _ = crate::widgets::name_combo(combo, crate::i18n::tr(language, "Font Style"));
}

fn panel_query(ui: &mut egui::Ui, language: &str, framework: bool, query: &mut String) {
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
        use craft_ui_widgets::style::ControlSize;
        use craft_ui_widgets::text::TextField;

        let label = crate::i18n::tr(language, "Search: character or U+code");
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        TextField::new(label, Bind::bound(query))
            .keeps_on_escape()
            .limit(None)
            .id_salt("glyph_query")
            .placeholder(label)
            .tooltip(label)
            .size(ControlSize::Sm)
            .show_at(ui, rect);
        return;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = framework;
    ui.add(
        egui::TextEdit::singleline(query)
            .hint_text(crate::rtl::widget(ui, crate::i18n::tr(language, "Search: character or U+code")))
            .desired_width(f32::INFINITY),
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn recent_name(language: &str, c: char) -> String {
    format!("{}: {c} (U+{:04X})", crate::i18n::tr(language, "Insert"), u32::from(c))
}

fn panel_recent(ui: &mut egui::Ui, language: &str, framework: bool, target: &Option<String>, c: char) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::buttons::Button;
        use craft_ui_widgets::contract::{Props as _, Show as _};
        use craft_ui_widgets::style::ControlSize;

        return Button::new(recent_name(language, c))
            .painted(c.to_string())
            .tooltip(format!("U+{:04X}", u32::from(c)))
            .id_salt(("glyph_recent", target, c))
            .size(ControlSize::Sm)
            .show(ui)
            .clicked();
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (language, framework, target);
    ui.small_button(c.to_string()).on_hover_text(format!("U+{:04X}", c as u32)).clicked()
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::runtime::env::{Direction, with_env};

        let direction = if crate::i18n::is_rtl(&app.ui.language) { Direction::Rtl } else { Direction::Ltr };
        with_env(ui, |scope| scope.direction = Some(direction), |ui| show_contents(app, ui));
        return;
    }
    show_contents(app, ui);
}

fn show_contents(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    let framework = app.framework_installed;
    #[cfg(target_arch = "wasm32")]
    let framework = false;
    let target = app.session.active().map(|st| format!("{:?}", (st.uid, st.identity_epoch, &st.selection.text, &st.selection.cells)));
    let t = Tokens::get(ui.ctx());
    let id = egui::Id::unique("glyphs_panel");
    let mut st: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    // Default to the font at the text cursor.
    if st.family.is_empty() {
        let a = super::text_attrs(app);
        st.family = a
            .as_ref()
            .and_then(|a| a["chars"]["fontFamily"].as_str().map(str::to_string))
            .unwrap_or_else(|| designcraft_fonts::DEFAULT_FAMILY.into());
        st.style = a.as_ref().and_then(|a| a["chars"]["fontStyle"].as_str().map(str::to_string)).unwrap_or_else(|| "Regular".into());
    }
    let (db, scope) = (super::fonts(app), super::font_scope(app));
    ui.horizontal(|ui| {
        if let Some(f) = super::font_combo(app, ui, "glyph_family", &st.family, 140.0) {
            st.style = f.style.clone().or_else(|| db.styles(&f.family).into_iter().next()).unwrap_or_else(|| "Regular".into());
            st.family = f.family;
        }
        panel_style(ui, &app.ui.language, framework, &st.family, &mut st.style, db.styles(&st.family));
    });
    panel_query(ui, &app.ui.language, framework, &mut st.query);
    let q = st.query.trim().to_string();
    let mut insert: Option<char> = None;
    if !st.recent.is_empty() {
        ui.horizontal_wrapped(|ui| {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Recently Used")).size(10.5).color(t.text_dim));
            for c in st.recent.clone() {
                if panel_recent(ui, &app.ui.language, framework, &target, c) {
                    insert = Some(c);
                }
            }
        });
    }
    let (picked, count) = grid(ui, &db, scope, &st.family, &st.style, &q, 320.0);
    insert = insert.or(picked);
    if let Some(c) = insert {
        match app.run("text.insert", json!({"text": c.to_string(), "raw": true})) {
            Ok(_) => {
                st.recent.retain(|x| *x != c);
                st.recent.insert(0, c);
                st.recent.truncate(12);
            }
            Err(e) => app.status_error("Glyphs", e),
        }
    }
    crate::rtl::label(
        ui,
        egui::RichText::new(
            crate::i18n::tr(&app.ui.language, "{count} glyphs — click to insert at the text cursor").replace("{count}", &count.to_string()),
        )
        .size(10.5)
        .color(t.text_dim),
    );
    ui.data_mut(|d| d.insert_temp(id, st));
}

/// The characters of font `family` `style` in a grid (those matching `query`, a character or
/// U+code, when it isn't empty), at most `max_height` tall and scrolling: the character clicked,
/// and how many are shown.
pub fn grid(
    ui: &mut egui::Ui,
    db: &designcraft_fonts::ScopedFonts<'_>,
    scope: u32,
    family: &str,
    style: &str,
    query: &str,
    max_height: f32,
) -> (Option<char>, usize) {
    let t = Tokens::get(ui.ctx());
    let face = db.face(family, style);
    let q = query.trim();
    let code = q.strip_prefix("U+").or_else(|| q.strip_prefix("u+")).and_then(|h| u32::from_str_radix(h, 16).ok());
    let mut chars: Vec<char> = face.chars().into_iter().map(|(c, _)| c).filter(|c| !c.is_control()).collect();
    chars.sort();
    chars.dedup();
    if !q.is_empty() {
        chars.retain(|c| Some(*c as u32) == code || q.contains(*c) || format!("{:04X}", *c as u32).contains(&q.to_ascii_uppercase()));
    }
    chars.truncate(MAX);
    // The scroll bar floats over the grid's right edge: leave it room.
    let scroll = &ui.spacing().scroll;
    let bar = scroll.bar_inner_margin + scroll.bar_width + scroll.bar_outer_margin;
    let cols = (((ui.available_width() - bar) / CELL).floor() as u32).max(1);
    let ppp = ui.ctx().pixels_per_point();
    let cell_px = (CELL * ppp).round().max(1.0) as u32;
    // The grid is one texture: no more rows than the GPU's largest texture holds (a narrow
    // panel of a large CJK font ran past it).
    let max_rows = (ui.ctx().input(|i| i.max_texture_side) as u32 / cell_px).max(1);
    chars.truncate((max_rows * cols) as usize);
    let ink = t.text_strong;
    let key = egui::Id::unique(("glyph_grid", family, style, scope, q, cols, cell_px, ink.to_array()));
    let tex: egui::TextureHandle = match ui.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        Some(t) => t,
        None => {
            let img = designcraft_render::glyphs::glyph_grid(db, family, style, &chars, cols, cell_px, ink.to_array());
            let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
            let h = crate::widgets::load_texture(ui.ctx(), "glyph_grid", ci, egui::TextureOptions::LINEAR);
            ui.data_mut(|d| d.insert_temp(key, h.clone()));
            h
        }
    };
    let rows = (chars.len() as u32).div_ceil(cols);
    let mut picked = None;
    egui::ScrollArea::vertical().id_salt(("glyph_scroll", family, style)).max_height(max_height).show(ui, |ui| {
        let size = vec2(cols as f32 * CELL, rows as f32 * CELL);
        let (r, resp) = ui.allocate_exact_size(size, Sense::click());
        ui.painter().rect_filled(r, 0.0, t.input);
        ui.painter().image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        for c in 1..cols {
            let x = r.min.x + c as f32 * CELL;
            ui.painter().line_segment([egui::pos2(x, r.min.y), egui::pos2(x, r.max.y)], egui::Stroke::new(1.0, t.border));
        }
        for row in 1..rows {
            let y = r.min.y + row as f32 * CELL;
            ui.painter().line_segment([egui::pos2(r.min.x, y), egui::pos2(r.max.x, y)], egui::Stroke::new(1.0, t.border));
        }
        let cell_at = |p: egui::Pos2| {
            let (cx, cy) = (((p.x - r.min.x) / CELL) as u32, ((p.y - r.min.y) / CELL) as u32);
            chars.get((cy * cols + cx.min(cols - 1)) as usize).copied()
        };
        if let Some(p) = resp.hover_pos()
            && let Some(c) = cell_at(p)
        {
            let (cx, cy) = (((p.x - r.min.x) / CELL).floor(), ((p.y - r.min.y) / CELL).floor());
            let cr = egui::Rect::from_min_size(r.min + vec2(cx * CELL, cy * CELL), vec2(CELL, CELL));
            ui.painter().rect_stroke(cr, 0.0, egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
            resp.clone().on_hover_text(format!("U+{:04X}", c as u32));
        }
        if resp.clicked()
            && let Some(c) = resp.interact_pointer_pos().and_then(cell_at)
        {
            picked = Some(c);
        }
    });
    (picked, chars.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_leaves_room_for_the_scroll_bar() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        crate::theme::apply(&ctx, &Tokens::for_brightness(crate::theme::Brightness::MediumDark));
        let scroll = ctx.global_style().spacing.scroll;
        let bar = scroll.bar_inner_margin + scroll.bar_width + scroll.bar_outer_margin;
        for width in [236.0, 256.0, 300.0, 333.0] {
            let mut panel = egui::Rect::NOTHING;
            let mut grid = None;
            for _ in 0..2 {
                // The app raises the texture limit for the grid (see `test_window`).
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 700.0))),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| {
                    ui.allocate_ui(vec2(width, 650.0), |ui| {
                        ui.set_width(width);
                        panel = ui.max_rect();
                        show(&mut app, ui);
                    });
                });
                out.textures_delta.clear();
                // The grid is the one textured mesh that isn't text.
                grid = out.shapes.iter().find_map(|s| match &s.shape {
                    egui::Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => Some(m.calc_bounds()),
                    _ => None,
                });
            }
            let grid = grid.expect("the glyph grid is drawn");
            assert!(grid.right() <= panel.right() - bar + 0.5, "{width}: the grid {grid:?} runs under the scroll bar of {panel:?}");
            assert!(grid.right() > panel.right() - bar - CELL, "{width}: no column is left out ({grid:?})");
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod framework_actions_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded};
    use egui::{Event, Key, Modifiers, PointerButton, Rect};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use serde_json::Value;
    use std::sync::Arc;

    struct Rig {
        app: DesignApp,
        recorded: Option<Arc<Recorded>>,
        parent: Rect,
    }

    fn text_app() -> (DesignApp, Value, Value) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 1, "facingPages": false})).unwrap();
        let first = app.run("frame.create", json!({"rect": [72,72,372,300], "content": "text", "text": "first row\nsecond row"})).unwrap();
        let other = app.run("frame.create", json!({"rect": [390,72,590,300], "content": "text", "text": "other story"})).unwrap();
        app.run("text.select", json!({"story": first["story"], "anchor": 0, "focus": 0})).unwrap();
        app.run("type.char", json!({"attrs": {"fontFamily": designcraft_fonts::DEFAULT_FAMILY, "fontStyle": "Regular"}})).unwrap();
        let db = designcraft_fonts::FontDb::global();
        let face = db.face(designcraft_fonts::DEFAULT_FAMILY, "Regular");
        assert_eq!(face.family, designcraft_fonts::DEFAULT_FAMILY);
        assert!(!face.is_placeholder());
        for c in ['A', 'B', 'é', '\'', '"', ' '] {
            assert!(face.covers(c), "real fixture requires U+{:04X}", u32::from(c));
        }
        (app, first["story"].clone(), other["story"].clone())
    }

    fn install_arabic_fixture(ctx: &egui::Context, language: &str) {
        #[cfg(target_os = "macos")]
        let family = "Geeza Pro";
        #[cfg(target_os = "windows")]
        let family = "Segoe UI";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let family = "DejaVu Sans";
        let db = designcraft_fonts::FontDb::global();
        assert!(db.has_family(family), "requires already-installed {family}, no install/tofu/locale skip");
        let face = db.face(family, "Regular");
        assert!(face.family.eq_ignore_ascii_case(family) && !face.is_placeholder());
        let keys = [
            "Glyphs",
            "Font Family",
            "Font Style",
            "Recently Used",
            "Insert",
            "Search: character or U+code",
            "{count} glyphs — click to insert at the text cursor",
        ];
        let mut covered = std::collections::BTreeSet::new();
        for key in keys {
            for c in crate::i18n::tr(language, key).chars().filter(|c| designcraft_fonts::is_rtl(*c) || ('\u{0600}'..='\u{06ff}').contains(c)) {
                assert!(face.covers(c), "{family} lacks Glyphs U+{:04X} from {key}", u32::from(c));
                covered.insert(c);
            }
        }
        assert!(!covered.is_empty());
        let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        let fallback = crate::theme::SystemFont::new("Arab", face.family.clone(), face.data().to_vec(), face.index());
        crate::theme::add_system_fallbacks(&mut definitions, &[fallback], language);
        ctx.set_fonts(definitions);
        eprintln!(
            "Glyphs Arabic fixture: {} {}, face {}, {} bytes, {} actual translated glyphs",
            face.family,
            face.style,
            face.index(),
            face.data().len(),
            covered.len()
        );
    }

    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, Rig> {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        if installed {
            app.framework_opacity = true;
            app.install_framework(&ctx);
            assert!(app.framework_installed);
        }
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut locale_fonts_ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(900.0, 600.0)).build_ui_state(
            move |ui, rig: &mut Rig| {
                if !locale_fonts_ready {
                    locale_fonts_ready = true;
                    if rig.app.ui.language == "ar" {
                        install_arabic_fixture(ui.ctx(), &rig.app.ui.language);
                        return;
                    }
                }
                let region = ui.scope_id();
                let (_, recorded) = craft_ui_widgets::runtime::inspect::record(ui, region, |ui| show(&mut rig.app, ui));
                rig.parent = ui.min_rect();
                if recorded.is_some() {
                    rig.recorded = recorded;
                }
            },
            Rig { app, recorded: None, parent: Rect::NOTHING },
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn steps(h: &mut Harness<'_, Rig>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }
    fn event(h: &mut Harness<'_, Rig>, event: Event) {
        h.input_mut().events.push(event);
        steps(h, 1);
    }
    fn key_mod(h: &mut Harness<'_, Rig>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        steps(h, 2);
    }
    fn key(h: &mut Harness<'_, Rig>, key: Key) {
        key_mod(h, key, Modifiers::NONE);
    }
    fn pointer(h: &mut Harness<'_, Rig>, pos: egui::Pos2, pressed: bool) {
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    fn click_at(h: &mut Harness<'_, Rig>, pos: egui::Pos2) {
        pointer(h, pos, true);
        pointer(h, pos, false);
        steps(h, 2);
    }
    fn label(h: &Harness<'_, Rig>, english: &str) -> String {
        crate::i18n::tr(&h.state().app.ui.language, english).to_owned()
    }
    fn panel_state(h: &Harness<'_, Rig>) -> super::State {
        // Observe the actual production state key, never install or alter a fixture state.
        h.ctx.data(|d| d.get_temp(egui::Id::unique("glyphs_panel"))).expect("production show persisted its state")
    }
    fn component(h: &Harness<'_, Rig>, kind: &str, name: &str) -> Element {
        let got: Vec<_> = h
            .state()
            .recorded
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.component == kind && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some(name)))
            .cloned()
            .collect();
        if got.len() != 1 {
            eprintln!("complete actual SDK record BEFORE accessibility traversal: {:#?}", h.state().recorded);
        }
        assert_eq!(
            got.len(),
            1,
            "one actual {kind} named {name}: {got:?}; complete actual SDK record: {:#?}; actual accessibility nodes (role, name, focus, optional AccessKit bounds in physical coordinates): {:?}",
            h.state().recorded,
            h.query_all_by(|_| true)
                .map(|node| (node.accesskit_node().role(), node.accesskit_node().label(), node.is_focused(), node.accesskit_node().bounding_box()))
                .collect::<Vec<_>>()
        );
        got[0].clone()
    }
    fn tab_to(h: &mut Harness<'_, Rig>, role: egui::accesskit::Role, name: Option<&str>) {
        let bound = h.query_all_by(|_| true).count();
        for _ in 0..=bound {
            if h.query_by(|n| n.role() == role && name.is_none_or(|name| n.label().as_deref() == Some(name))).is_some_and(|n| n.is_focused()) {
                return;
            }
            key(h, Key::Tab);
        }
        panic!("natural Tab could not reach actual {role:?} {name:?}");
    }
    fn query(h: &mut Harness<'_, Rig>, text: &str) {
        let name = h.state().app.framework_installed.then(|| label(h, "Search: character or U+code"));
        tab_to(h, egui::accesskit::Role::TextInput, name.as_deref());
        key_mod(h, Key::A, Modifiers::COMMAND);
        event(h, Event::Text(text.to_owned()));
        steps(h, 2);
        assert_eq!(panel_state(h).query, text, "query filters live before Enter/blur");
    }
    fn recent_label(h: &Harness<'_, Rig>, c: char) -> String {
        recent_name(&h.state().app.ui.language, c)
    }
    fn recent_at(h: &Harness<'_, Rig>, c: char) -> egui::Pos2 {
        let name = recent_label(h, c);
        let _ = component(h, "Button", &name);
        h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(name.as_str())).rect().center()
    }
    fn recent_click(h: &mut Harness<'_, Rig>, c: char) {
        let pos = recent_at(h, c);
        click_at(h, pos);
    }
    fn recent_key(h: &mut Harness<'_, Rig>, c: char, activation: Key) {
        let name = recent_label(h, c);
        tab_to(h, egui::accesskit::Role::Button, Some(&name));
        key(h, activation);
    }
    fn grid_chars(h: &Harness<'_, Rig>, cols: usize) -> Vec<char> {
        let st = panel_state(h);
        let fonts = super::super::fonts(&h.state().app);
        let face = fonts.face(&st.family, &st.style);
        let q = st.query.trim();
        let code = q.strip_prefix("U+").or_else(|| q.strip_prefix("u+")).and_then(|s| u32::from_str_radix(s, 16).ok());
        // The unchanged renderer's actual FontDb ordering, including its literal/hex union.
        let mut chars: Vec<_> = face.chars().into_iter().map(|(c, _)| c).filter(|c| !c.is_control()).collect();
        chars.sort();
        chars.dedup();
        if !q.is_empty() {
            chars.retain(|c| Some(u32::from(*c)) == code || q.contains(*c) || format!("{:04X}", u32::from(*c)).contains(&q.to_ascii_uppercase()));
        }
        chars.truncate(MAX);
        let cell_px = (CELL * h.ctx.pixels_per_point()).round().max(1.0) as usize;
        let rows = (h.ctx.input(|i| i.max_texture_side) / cell_px).max(1);
        chars.truncate(rows * cols);
        chars
    }
    fn grid_mesh(h: &Harness<'_, Rig>) -> (Rect, Rect) {
        let found: Vec<_> = h
            .output()
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                    Some((mesh.calc_bounds(), shape.clip_rect.intersect(h.state().parent)))
                }
                _ => None,
            })
            .collect();
        assert_eq!(found.len(), 1, "actual parent must paint its one glyph atlas, not a test-created surface");
        found[0]
    }
    fn grid_pick_current(h: &mut Harness<'_, Rig>, c: char) {
        let (rect, clip) = grid_mesh(h);
        let cols = (rect.width() / CELL).round() as usize;
        assert!(cols > 0);
        let chars = grid_chars(h, cols);
        assert_eq!((rect.height() / CELL).round() as usize, chars.len().div_ceil(cols));
        let at = chars.iter().position(|&value| value == c).expect("real selected face/query contains the requested glyph");
        let pos = rect.min + vec2((at % cols) as f32 * CELL + CELL * 0.5, (at / cols) as f32 * CELL + CELL * 0.5);
        assert!(clip.contains(pos), "stimulus must hit a visible cell of the real texture: {pos:?}, {rect:?}, {clip:?}");
        click_at(h, pos);
    }
    fn grid_pick(h: &mut Harness<'_, Rig>, c: char) {
        query(h, &format!("U+{:04X}", u32::from(c)));
        grid_pick_current(h, c);
    }
    fn count(h: &Harness<'_, Rig>, expected: usize) {
        let text = label(h, "{count} glyphs — click to insert at the text cursor").replace("{count}", &expected.to_string());
        assert!(
            h.query_by(|n| n.role() == egui::accesskit::Role::Label && n.value().as_deref() == Some(text.as_str())).is_some(),
            "expected complete count footer: {text}"
        );
    }
    fn calls(h: &Harness<'_, Rig>, start: usize) -> Vec<(String, Value)> {
        h.state().app.session.journal[start..].iter().filter(|(id, _)| id == "text.insert").cloned().collect()
    }
    fn raw(c: char) -> (String, Value) {
        ("text.insert".to_owned(), json!({"text":c.to_string(),"raw":true}))
    }
    fn undo_len(h: &Harness<'_, Rig>) -> usize {
        h.state().app.session.active().unwrap().history.undo.len()
    }
    fn story(h: &mut Harness<'_, Rig>, sid: &Value) -> String {
        h.state_mut().app.session.execute("story.get", &json!({"story":sid})).unwrap()["text"].as_str().unwrap().to_owned()
    }
    fn select(h: &mut Harness<'_, Rig>, sid: &Value, anchor: usize, focus: usize) {
        h.state_mut().app.run("text.select", json!({"story":sid,"anchor":anchor,"focus":focus})).unwrap();
        steps(h, 2);
    }
    fn undo(h: &mut Harness<'_, Rig>) {
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        steps(h, 2);
    }

    #[test]
    fn installed_default_and_cursor_font_style_pick_cancel_keep_document_and_natural_focus() {
        let (app, _, _) = text_app();
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        let start = h.state().app.session.journal.len();
        assert_eq!(panel_state(&h).family, designcraft_fonts::DEFAULT_FAMILY);
        assert_eq!(panel_state(&h).style, "Regular");
        let name = label(&h, "Font Style");
        assert!(!component(&h, "Select", &name).facts.unwrap().disabled);
        assert!(
            h.get_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.label().as_deref() == Some(name.as_str()))
                .accesskit_node()
                .has_popup()
                .is_some()
        );
        let styles = super::super::fonts(&h.state().app).styles(designcraft_fonts::DEFAULT_FAMILY);
        for desired in ["Bold", "Italic"] {
            let at = styles.iter().position(|s| s == desired).expect("real bundled family supplies the fixture style");
            tab_to(&mut h, egui::accesskit::Role::ComboBox, Some(&name));
            key(&mut h, Key::Space);
            assert!(h.query_by(|n| n.role() == egui::accesskit::Role::ListBox).is_some());
            key(&mut h, Key::Home);
            for _ in 0..at {
                key(&mut h, Key::ArrowDown);
            }
            key(&mut h, Key::Enter);
            assert_eq!(panel_state(&h).style, desired);
            assert!(h.query_by(|n| n.role() == egui::accesskit::Role::ListBox).is_none());
            assert!(
                h.get_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.label().as_deref() == Some(name.as_str())).is_focused(),
                "ordinary style edits must not replace the focus identity"
            );
            key(&mut h, Key::Enter);
            key(&mut h, Key::Home);
            key(&mut h, Key::Escape);
            assert_eq!(panel_state(&h).style, desired, "cancel restores the actual picked style");
        }
        assert!(calls(&h, start).is_empty());
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
    }

    #[test]
    fn installed_real_family_pick_preserves_query_and_family_style_fallback_without_document_edits() {
        let (app, _, _) = text_app();
        let mut h = harness(app, true);
        query(&mut h, "é");
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        let start = h.state().app.session.journal.len();
        let family_name = label(&h, "Font Family");
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.label().as_deref() == Some(family_name.as_str())).rect().center();
        click_at(&mut h, pos);
        // The unchanged family menu naturally puts typing in its real search field.
        event(&mut h, Event::Text("Source Sans 3".to_owned()));
        steps(&mut h, 2);
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some("Source Sans 3")).rect().center();
        click_at(&mut h, pos);
        let st = panel_state(&h);
        assert_eq!(st.family, "Source Sans 3");
        assert_eq!(st.style, super::super::fonts(&h.state().app).styles(&st.family).into_iter().next().unwrap_or_else(|| "Regular".into()));
        assert_eq!(st.query, "é");
        assert!(calls(&h, start).is_empty());
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
    }

    #[test]
    fn installed_missing_saved_family_has_real_empty_style_list_without_invented_edit_or_disabled_state() {
        let (mut app, _, _) = text_app();
        let family = "Glyphs Missing Saved Family";
        assert!(super::super::fonts(&app).styles(family).is_empty());
        app.run("type.char", json!({"attrs":{"fontFamily":family,"fontStyle":"Regular"}})).unwrap();
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        let name = label(&h, "Font Style");
        assert_eq!(panel_state(&h).family, family);
        assert!(!component(&h, "Select", &name).facts.unwrap().disabled, "legacy empty chooser is not a document-edit refusal gate");
        tab_to(&mut h, egui::accesskit::Role::ComboBox, Some(&name));
        key(&mut h, Key::Space);
        key(&mut h, Key::ArrowDown);
        key(&mut h, Key::Enter);
        key(&mut h, Key::Escape);
        assert_eq!(panel_state(&h).style, "Regular");
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
    }

    #[test]
    fn installed_live_literal_unicode_code_and_no_match_queries_preserve_plain_escape_and_history() {
        let (app, sid, other) = text_app();
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let first = story(&mut h, &sid);
        let second = story(&mut h, &other);
        let history = undo_len(&h);
        let start = h.state().app.session.journal.len();
        for q in ["é", " U+0041 ", "u+00E9", "0041", "'"] {
            query(&mut h, q);
            let (rect, _) = grid_mesh(&h);
            let chars = grid_chars(&h, (rect.width() / CELL).round() as usize);
            assert!(!chars.is_empty());
            count(&h, chars.len());
            if q.trim() == "U+0041" {
                assert!(
                    chars.contains(&'A') && chars.contains(&'U') && chars.contains(&'+'),
                    "preserve the existing code/literal union, not an exact-code-only rewrite"
                );
            }
            if q == "é" {
                assert_eq!(chars, ['é']);
            }
            if q == "'" {
                assert_eq!(chars, ['\'']);
            }
            let value = panel_state(&h).query;
            key(&mut h, Key::Escape);
            assert_eq!(panel_state(&h).query, value, "plain legacy Escape keeps typed query, never clears/restores it");
            let name = label(&h, "Search: character or U+code");
            assert!(
                !h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.label().as_deref() == Some(name.as_str())).is_focused(),
                "plain Escape surrenders the actual field focus"
            );
        }
        let fonts = super::super::fonts(&h.state().app);
        let ui_face = fonts.face("Source Sans 3", "Regular");
        let grid_face = fonts.face(designcraft_fonts::DEFAULT_FAMILY, "Regular");
        assert_eq!(ui_face.family, "Source Sans 3");
        assert!(!ui_face.is_placeholder());
        let absent = ui_face
            .chars()
            .into_iter()
            .map(|(c, _)| c)
            .find(|c| !c.is_ascii() && !c.is_control() && !c.is_whitespace() && !grid_face.covers(*c))
            .expect("real bundled GUI face must supply a character the selected serif face lacks");
        assert!(ui_face.covers(absent) && !grid_face.covers(absent));
        query(&mut h, &absent.to_string());
        count(&h, 0);
        steps(&mut h, 3);
        assert!(calls(&h, start).is_empty());
        assert_eq!(story(&mut h, &sid), first);
        assert_eq!(story(&mut h, &other), second);
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
    }

    #[test]
    fn installed_query_is_unbounded_and_browsing_never_inserts_into_the_editor() {
        let (app, _, _) = text_app();
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        let start = h.state().app.session.journal.len();
        let long = "A".repeat(1100);
        query(&mut h, &long);
        assert_eq!(panel_state(&h).query.chars().count(), 1100);
        count(&h, 1);
        assert_eq!(grid_chars(&h, 1), ['A']);
        key(&mut h, Key::Escape);
        assert_eq!(panel_state(&h).query, long);
        assert!(calls(&h, start).is_empty());
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
    }

    #[test]
    fn installed_real_atlas_hit_and_recent_pointer_keyboard_keep_exact_raw_replacement_and_undo() {
        let (app, sid, other) = text_app();
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        let start = h.state().app.session.journal.len();
        grid_pick(&mut h, 'é');
        assert_eq!(calls(&h, start), [raw('é')]);
        assert_eq!(panel_state(&h).recent, ['é']);
        assert_eq!(story(&mut h, &sid), "éfirst row\nsecond row");
        assert_eq!(undo_len(&h), history + 1);
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        assert_eq!(panel_state(&h).recent, ['é'], "document Undo must not pretend to undo the panel MRU");
        for activation in [None, Some(Key::Space), Some(Key::Enter)] {
            select(&mut h, &sid, 0, 5);
            let before = h.state().app.session.active().unwrap().doc.clone();
            let history = undo_len(&h);
            let other_before = story(&mut h, &other);
            let start = h.state().app.session.journal.len();
            if let Some(key) = activation {
                recent_key(&mut h, 'é', key);
            } else {
                recent_click(&mut h, 'é');
            }
            assert_eq!(calls(&h, start), [raw('é')]);
            assert_eq!(story(&mut h, &sid), "é row\nsecond row");
            let text = h.state().app.session.active().unwrap().selection.text.unwrap();
            assert_eq!((text.anchor, text.focus), (2, 2));
            assert_eq!(story(&mut h, &other), other_before);
            assert_eq!(undo_len(&h), history + 1);
            steps(&mut h, 3);
            assert_eq!(calls(&h, start).len(), 1);
            undo(&mut h);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        }
    }

    #[test]
    fn installed_literal_quote_and_space_actions_bypass_smart_quotes_and_autocorrect() {
        for c in ['\'', '"', ' '] {
            let (mut app, sid, other) = text_app();
            app.run("prefs.set", json!({"typographersQuotes":true,"autocorrect":true,"autocorrectList":[["first","corrected"]]})).unwrap();
            if c == ' ' {
                app.run("text.select", json!({"story":sid,"anchor":5,"focus":5})).unwrap();
            }
            let mut h = harness(app, true);
            let before = h.state().app.session.active().unwrap().doc.clone();
            let history = undo_len(&h);
            let start = h.state().app.session.journal.len();
            grid_pick(&mut h, c);
            assert_eq!(calls(&h, start), [raw(c)]);
            let expected = if c == ' ' { "first  row\nsecond row".to_owned() } else { format!("{c}first row\nsecond row") };
            assert_eq!(story(&mut h, &sid), expected);
            assert_eq!(story(&mut h, &other), "other story");
            assert_eq!(undo_len(&h), history + 1);
            undo(&mut h);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
            let start = h.state().app.session.journal.len();
            recent_click(&mut h, c);
            assert_eq!(calls(&h, start), [raw(c)]);
            assert_eq!(story(&mut h, &sid), expected);
        }
    }

    #[test]
    fn installed_success_only_recent_dedup_order_cap_and_real_refusal_preserve_history() {
        let (app, sid, _) = text_app();
        let mut h = harness(app, true);
        let start = h.state().app.session.journal.len();
        for c in 'A'..='M' {
            grid_pick(&mut h, c);
        }
        assert_eq!(calls(&h, start), ('A'..='M').map(raw).collect::<Vec<_>>());
        let expected: Vec<_> = ('B'..='M').rev().collect();
        assert_eq!(panel_state(&h).recent, expected);
        recent_key(&mut h, 'D', Key::Space);
        let mut reordered = expected.clone();
        reordered.retain(|&c| c != 'D');
        reordered.insert(0, 'D');
        assert_eq!(panel_state(&h).recent, reordered);
        assert_eq!(calls(&h, start).len(), 14);
        h.state_mut().app.run("selection.set", json!({"ids":[]})).unwrap();
        steps(&mut h, 2);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        let text = story(&mut h, &sid);
        let start = h.state().app.session.journal.len();
        recent_click(&mut h, 'B');
        assert!(calls(&h, start).is_empty(), "real refusal is not a successful journal insertion");
        assert!(h.state().app.ui.status.contains("insertion point"));
        assert_eq!(panel_state(&h).recent, reordered);
        assert_eq!(story(&mut h, &sid), text);
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        select(&mut h, &sid, 0, 0);
        recent_click(&mut h, 'B');
        assert_eq!(calls(&h, start), [raw('B')]);
        assert_eq!(panel_state(&h).recent[0], 'B');
    }

    #[test]
    fn installed_held_recent_cancels_other_story_range_cell_document_and_absence_with_fresh_recovery() {
        for transition in ["range", "story", "cell", "document", "absent"] {
            let (mut app, sid, other) = text_app();
            let table = if transition == "cell" { Some(app.run("table.insert", json!({"rows":1,"cols":2})).unwrap()["table"].clone()) } else { None };
            let mut h = harness(app, true);
            grid_pick(&mut h, 'A');
            assert_eq!(panel_state(&h).recent, ['A']);
            let pos = recent_at(&h, 'A');
            pointer(&mut h, pos, true);
            match transition {
                "range" => {
                    h.state_mut().app.run("text.select", json!({"story":sid,"anchor":2,"focus":5})).unwrap();
                }
                "story" => {
                    h.state_mut().app.run("text.select", json!({"story":other,"anchor":1,"focus":3})).unwrap();
                }
                "cell" => {
                    h.state_mut().app.run("table.select", json!({"story":sid,"table":table,"row":0,"col":1,"what":"cell"})).unwrap();
                    assert_eq!(h.state().app.session.active().unwrap().selection.text.unwrap().cell.unwrap().col, 1);
                }
                "document" => {
                    h.state_mut().app.run("file.new", json!({"pages":1})).unwrap();
                    let frame = h.state_mut().app.run("frame.create", json!({"rect":[72,72,372,300],"content":"text","text":"new story"})).unwrap();
                    h.state_mut().app.run("text.select", json!({"story":frame["story"],"anchor":0,"focus":0})).unwrap();
                }
                "absent" => {
                    h.state_mut().app.run("selection.set", json!({"ids":[]})).unwrap();
                }
                _ => unreachable!(),
            }
            steps(&mut h, 2);
            let before = h.state().app.session.active().unwrap().doc.clone();
            let original = h.state().app.session.documents()[0].doc.clone();
            let history = undo_len(&h);
            let start = h.state().app.session.journal.len();
            let saved_query = panel_state(&h).query;
            pointer(&mut h, pos, false);
            steps(&mut h, 3);
            assert!(calls(&h, start).is_empty(), "held action cancelled across {transition}");
            assert_eq!(undo_len(&h), history);
            assert_eq!(panel_state(&h).recent, ['A']);
            assert_eq!(panel_state(&h).query, saved_query, "target fencing must not reset persistent browser state");
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
            assert!(Arc::ptr_eq(&h.state().app.session.documents()[0].doc, &original));
            if transition == "absent" {
                select(&mut h, &sid, 0, 0);
            }
            let text = h.state().app.session.active().unwrap().selection.text.unwrap();
            let mut expected = before.text_story(text.story, text.cell).unwrap().text.clone();
            expected.replace_range(text.range(), "A");
            recent_click(&mut h, 'A');
            assert_eq!(calls(&h, start), [raw('A')]);
            let active = h.state().app.session.active().unwrap();
            assert_eq!(active.doc.text_story(text.story, text.cell).unwrap().text, expected, "fresh real action reaches the exact new text target");
            assert_eq!(undo_len(&h), history + 1);
            undo(&mut h);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
            assert!(Arc::ptr_eq(&h.state().app.session.documents()[0].doc, &original));
        }
    }

    #[test]
    fn installed_held_recent_slot_cannot_follow_real_keyboard_mru_reassignment() {
        let (app, sid, _) = text_app();
        let mut h = harness(app, true);
        grid_pick(&mut h, 'A');
        grid_pick(&mut h, 'B');
        assert_eq!(panel_state(&h).recent, ['B', 'A']);
        let old_name = recent_label(&h, 'B');
        let old_owner = component(&h, "Button", &old_name).id;
        let pos = recent_at(&h, 'B');
        pointer(&mut h, pos, true);
        let start = h.state().app.session.journal.len();
        // A real independent keyboard activation updates the real MRU while the pointer is held.
        // No fixture writes its recent vector or forges a second primary-pointer press.
        recent_key(&mut h, 'A', Key::Space);
        assert_eq!(calls(&h, start), [raw('A')], "establish the real reassignment stimulus before testing the held release");
        assert_eq!(panel_state(&h).recent, ['A', 'B']);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let text = story(&mut h, &sid);
        let history = undo_len(&h);
        assert_ne!(component(&h, "Button", &old_name).id, old_owner, "normal insertion changed the complete text target");
        pointer(&mut h, pos, false);
        steps(&mut h, 3);
        assert_eq!(calls(&h, start), [raw('A')], "release cannot activate the character now occupying the old recent slot");
        assert_eq!(story(&mut h, &sid), text);
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        recent_click(&mut h, 'B');
        assert_eq!(calls(&h, start), [raw('A'), raw('B')]);
    }

    #[test]
    fn installed_complete_translated_names_and_rtl_keep_real_style_query_and_recent_keyboard_input() {
        for language in ["de", "ar"] {
            let (mut app, sid, _) = text_app();
            app.ui.language = language.to_owned();
            let mut h = harness(app, true);
            let name = label(&h, "Font Style");
            assert_ne!(name, "Font Style");
            let style = component(&h, "Select", &name);
            assert_eq!(style.facts.as_ref().unwrap().name.as_deref(), Some(name.as_str()));
            assert!(style.stop.is_some());
            let before = h.state().app.session.active().unwrap().doc.clone();
            let history = undo_len(&h);
            let start = h.state().app.session.journal.len();
            tab_to(&mut h, egui::accesskit::Role::ComboBox, Some(&name));
            key(&mut h, Key::Enter);
            key(&mut h, Key::End);
            key(&mut h, Key::Escape);
            assert_eq!(panel_state(&h).style, "Regular");
            query(&mut h, "u+0041");
            assert_eq!(panel_state(&h).query, "u+0041", "RTL must retain logical code order");
            assert!(calls(&h, start).is_empty());
            assert_eq!(undo_len(&h), history);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
            grid_pick_current(&mut h, 'A');
            assert_eq!(calls(&h, start), [raw('A')]);
            let insert = label(&h, "Insert");
            assert_ne!(insert, "Insert");
            let expected = format!("{insert}: A (U+0041)");
            assert_eq!(component(&h, "Button", &expected).facts.as_ref().unwrap().name.as_deref(), Some(expected.as_str()));
            assert!(!component(&h, "Button", &expected).facts.unwrap().disabled);
            recent_key(&mut h, 'A', Key::Space);
            assert_eq!(calls(&h, start), [raw('A'), raw('A')]);
            assert_eq!(story(&mut h, &sid), "AAfirst row\nsecond row");
        }
    }

    #[test]
    fn uninstalled_full_parent_retains_real_style_query_grid_recent_payloads_and_undo() {
        let (app, sid, _) = text_app();
        let mut h = harness(app, false);
        assert!(!h.state().app.framework_installed);
        assert!(!h.state().recorded.as_ref().is_some_and(|r| r.elements.iter().any(|e| matches!(e.component, "Select" | "TextField" | "Button"))));
        let name = label(&h, "Font Style");
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.label().as_deref() == Some(name.as_str())).rect().center();
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        click_at(&mut h, pos);
        let pos = h.get_by_label("Bold").rect().center();
        click_at(&mut h, pos);
        assert_eq!(panel_state(&h).style, "Bold");
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        let start = h.state().app.session.journal.len();
        grid_pick(&mut h, 'A');
        assert_eq!(calls(&h, start), [raw('A')]);
        assert_eq!(story(&mut h, &sid), "Afirst row\nsecond row");
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some("A")).rect().center();
        let start = h.state().app.session.journal.len();
        click_at(&mut h, pos);
        assert_eq!(calls(&h, start), [raw('A')]);
        assert_eq!(story(&mut h, &sid), "Afirst row\nsecond row");
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
    }
    #[test]
    fn installed_default_without_text_and_real_nondefault_cursor_font_initialize_without_edits() {
        for text_cursor in [false, true] {
            let (mut app, _, _) = text_app();
            if text_cursor {
                app.run("type.char", json!({"attrs": {"fontFamily": "Source Sans 3", "fontStyle": "Italic"}})).unwrap();
            } else {
                app.run("selection.set", json!({"ids": []})).unwrap();
            }
            let before = app.session.active().unwrap().doc.clone();
            let history = app.session.active().unwrap().history.undo.len();
            let h = harness(app, true);
            let st = panel_state(&h);
            assert_eq!(st.family, if text_cursor { "Source Sans 3" } else { designcraft_fonts::DEFAULT_FAMILY });
            assert_eq!(st.style, if text_cursor { "Italic" } else { "Regular" });
            assert_eq!(undo_len(&h), history);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        }
    }
    // The public plain-field opt-in is an isolated caller trial, not native-parent or visual parity.
    fn sdk_query(h: &Harness<'_, Rig>, expected: &str, focused: bool) -> Element {
        let name = label(h, "Search: character or U+code");
        assert_eq!(panel_state(h).query, expected, "existing persisted query updates live before Enter/blur");
        let field = component(h, "TextField", &name);
        assert!(field.part.is_none(), "inspect the actual field root, never a different part");
        let facts = field.facts.as_ref().unwrap();
        assert_eq!(facts.role, format!("{:?}", egui::accesskit::Role::TextInput));
        assert!(!facts.disabled && facts.interactive);
        assert_eq!(facts.value.as_deref(), Some(expected), "complete SDK literal value");
        let node = h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.label().as_deref() == Some(name.as_str()));
        assert_eq!(node.value().as_deref(), Some(expected), "complete actual AccessKit value");
        assert_eq!(node.is_focused(), focused, "actual named field focus at this input boundary");
        let bounds = node.rect();
        assert!(bounds.width() > 0.0 && bounds.height() > 0.0, "real named field bounds remain required");
        assert!(field.rect.contains(bounds.center()), "recorded field must contain its actual AX hit target");
        field
    }
    fn query_target(h: &Harness<'_, Rig>) -> String {
        let st = h.state().app.session.active().unwrap();
        format!("{:?}", (st.uid, st.identity_epoch, &st.selection.text, &st.selection.cells))
    }

    #[test]
    fn installed_plain_sdk_query_keeps_each_live_edit_and_actual_focus_until_escape_then_fresh_refocus() {
        let (mut app, sid, other) = text_app();
        app.run("text.select", json!({"story":sid,"anchor":2,"focus":7})).unwrap();
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let target = query_target(&h);
        let history = undo_len(&h);
        let first = story(&mut h, &sid);
        let second = story(&mut h, &other);
        let start = h.state().app.session.journal.len();
        let unchanged = |h: &mut Harness<'_, Rig>| {
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
            assert_eq!(query_target(h), target, "typing a query must not move the actual document/range/cell target");
            assert_eq!(undo_len(h), history);
            assert!(calls(h, start).is_empty());
            assert_eq!(story(h, &sid), first);
            assert_eq!(story(h, &other), second);
        };
        let name = label(&h, "Search: character or U+code");
        let id = sdk_query(&h, "", false).id;
        tab_to(&mut h, egui::accesskit::Role::TextInput, Some(&name));
        assert_eq!(sdk_query(&h, "", true).id, id);
        event(&mut h, Event::Text("al".to_owned()));
        assert_eq!(sdk_query(&h, "al", true).id, id);
        unchanged(&mut h);
        key_mod(&mut h, Key::A, Modifiers::COMMAND);
        assert_eq!(sdk_query(&h, "al", true).id, id);
        unchanged(&mut h);
        key(&mut h, Key::Backspace);
        assert_eq!(sdk_query(&h, "", true).id, id);
        unchanged(&mut h);
        event(&mut h, Event::Text("zz".to_owned()));
        assert_eq!(sdk_query(&h, "zz", true).id, id);
        unchanged(&mut h);
        key(&mut h, Key::Escape);
        assert_eq!(sdk_query(&h, "zz", false).id, id);
        assert_eq!(panel_state(&h).query, "zz", "first Escape keeps the last live query, not the run start");
        unchanged(&mut h);
        tab_to(&mut h, egui::accesskit::Role::TextInput, Some(&name));
        assert_eq!(sdk_query(&h, "zz", true).id, id);
        key_mod(&mut h, Key::A, Modifiers::COMMAND);
        event(&mut h, Event::Text("é".to_owned()));
        assert_eq!(sdk_query(&h, "é", true).id, id);
        unchanged(&mut h);
        key(&mut h, Key::Escape);
        assert_eq!(sdk_query(&h, "é", false).id, id);
        assert_eq!(panel_state(&h).query, "é");
        unchanged(&mut h);
    }

    #[test]
    fn installed_plain_sdk_query_real_pointer_clean_escape_and_enter_preserve_document_and_other_story() {
        let (mut app, sid, other) = text_app();
        app.run("text.select", json!({"story":other,"anchor":1,"focus":4})).unwrap();
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let target = query_target(&h);
        let history = undo_len(&h);
        let first = story(&mut h, &sid);
        let second = story(&mut h, &other);
        let start = h.state().app.session.journal.len();
        let unchanged = |h: &mut Harness<'_, Rig>| {
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
            assert_eq!(query_target(h), target);
            assert_eq!(undo_len(h), history);
            assert!(calls(h, start).is_empty(), "query typing/Enter/Escape must not insert into the editor");
            assert_eq!(story(h, &sid), first);
            assert_eq!(story(h, &other), second);
        };
        let name = label(&h, "Search: character or U+code");
        let id = sdk_query(&h, "", false).id;
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.label().as_deref() == Some(name.as_str())).rect().center();
        click_at(&mut h, pos);
        assert_eq!(sdk_query(&h, "", true).id, id);
        key(&mut h, Key::Escape);
        assert_eq!(sdk_query(&h, "", false).id, id);
        unchanged(&mut h);
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.label().as_deref() == Some(name.as_str())).rect().center();
        click_at(&mut h, pos);
        assert_eq!(sdk_query(&h, "", true).id, id);
        event(&mut h, Event::Text("u+0041".to_owned()));
        assert_eq!(sdk_query(&h, "u+0041", true).id, id);
        unchanged(&mut h);
        key(&mut h, Key::Enter);
        assert_eq!(panel_state(&h).query, "u+0041", "Enter never inserts or rewrites the live query");
        unchanged(&mut h);
        key(&mut h, Key::Escape);
        assert_eq!(sdk_query(&h, "u+0041", false).id, id);
        unchanged(&mut h);
    }

    #[test]
    fn installed_plain_sdk_query_has_complete_translated_semantics_and_real_logical_unicode_code_input() {
        for language in ["", "de", "ar"] {
            let (mut app, _, _) = text_app();
            app.ui.language = language.to_owned();
            let mut h = harness(app, true);
            let before = h.state().app.session.active().unwrap().doc.clone();
            let target = query_target(&h);
            let history = undo_len(&h);
            let start = h.state().app.session.journal.len();
            let name = label(&h, "Search: character or U+code");
            let id = sdk_query(&h, "", false).id;
            tab_to(&mut h, egui::accesskit::Role::TextInput, Some(&name));
            event(&mut h, Event::Text("é".to_owned()));
            assert_eq!(sdk_query(&h, "é", true).id, id);
            key_mod(&mut h, Key::A, Modifiers::COMMAND);
            event(&mut h, Event::Text("u+0041".to_owned()));
            assert_eq!(sdk_query(&h, "u+0041", true).id, id);
            assert_eq!(panel_state(&h).query, "u+0041", "RTL preserves the actual logical query");
            key(&mut h, Key::Escape);
            assert_eq!(sdk_query(&h, "u+0041", false).id, id);
            assert_eq!(query_target(&h), target);
            assert_eq!(undo_len(&h), history);
            assert!(calls(&h, start).is_empty());
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        }
    }
}
