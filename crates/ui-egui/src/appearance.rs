//! Appearance modes: follow the system (Auto) or keep a fixed dark or light interface, each with
//! its own saved theme. The header button cycles Auto, Light, Dark; Preferences › Interface
//! shows the mode above a light and a dark theme card, each with a small DesignCraft preview.

use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::theme::{AppearanceMode, Brightness, DarkTheme, LightTheme, Tokens};
use crate::{DesignApp, UiState};

/// The theme on screen for these settings. Auto uses the system's appearance, and Dark when the
/// system gives no answer.
pub fn selected(ui: &UiState, system: Option<egui::Theme>) -> Brightness {
    let light = match ui.appearance_mode {
        AppearanceMode::Light => true,
        AppearanceMode::Dark => false,
        AppearanceMode::Auto => system == Some(egui::Theme::Light),
    };
    if light { ui.light_theme.brightness() } else { ui.dark_theme.brightness() }
}

/// Bring the theme on screen in line with the settings (restyles only when it changes).
pub fn resolve(app: &mut DesignApp) {
    let want = selected(&app.ui, app.system_theme);
    if app.ui.brightness != want {
        app.ui.brightness = want;
        app.restyle = true;
    }
}

/// A single theme picked by name (Window › Interface Color Theme, `ui.set {brightness}`, older
/// scripts): it becomes the theme of its family, and the mode is fixed to that family.
pub fn pick_theme(app: &mut DesignApp, b: Brightness) {
    if let Some(t) = DarkTheme::of(b) {
        app.ui.dark_theme = t;
        app.ui.appearance_mode = AppearanceMode::Dark;
    } else if let Some(t) = LightTheme::of(b) {
        app.ui.light_theme = t;
        app.ui.appearance_mode = AppearanceMode::Light;
    }
    resolve(app);
}

/// `window.appearanceMode {mode?, darkTheme?, lightTheme?}`: every value is checked before any
/// is applied.
pub fn set(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    if !p.is_object() {
        return Err("appearance parameters must be an object".into());
    }
    let field = |k: &str| -> Result<Option<&str>, String> {
        match p.get(k) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(_) => Err(format!("{k} must be a string")),
        }
    };
    let mode =
        field("mode")?.map(|s| AppearanceMode::parse(s).ok_or_else(|| format!("unknown appearance mode `{s}` (auto, dark or light)"))).transpose()?;
    let dark = field("darkTheme")?
        .map(|s| DarkTheme::parse(s).ok_or_else(|| format!("`{s}` is not a dark theme (dark, mediumDark or highContrast)")))
        .transpose()?;
    let light =
        field("lightTheme")?.map(|s| LightTheme::parse(s).ok_or_else(|| format!("`{s}` is not a light theme (light or mediumLight)"))).transpose()?;
    if let Some(m) = mode {
        app.ui.appearance_mode = m;
    }
    if let Some(t) = dark {
        app.ui.dark_theme = t;
    }
    if let Some(t) = light {
        app.ui.light_theme = t;
    }
    resolve(app);
    Ok(state(&app.ui))
}

/// The header button and Window › Next Appearance Mode: Auto, Light, Dark, then Auto again. The
/// saved dark and light themes stay as they are.
pub fn cycle(app: &mut DesignApp) -> Value {
    app.ui.appearance_mode = app.ui.appearance_mode.next();
    resolve(app);
    state(&app.ui)
}

fn state(ui: &UiState) -> Value {
    json!({
        "mode": ui.appearance_mode.id(),
        "darkTheme": ui.dark_theme.brightness().id(),
        "lightTheme": ui.light_theme.brightness().id(),
        "brightness": ui.brightness.id(),
    })
}

/// Saved UI state from before appearance modes had one theme (`brightness`): keep its look as a
/// fixed Dark or Light mode instead of switching anyone to Auto. Appearance values that don't
/// parse are dropped (their defaults apply) rather than discarding the whole file.
pub fn migrate_saved(v: &mut Value) {
    let Some(m) = v.as_object_mut() else { return };
    for (key, ok) in [
        ("appearanceMode", (|s: &str| AppearanceMode::parse(s).is_some()) as fn(&str) -> bool),
        ("darkTheme", |s| DarkTheme::parse(s).is_some()),
        ("lightTheme", |s| LightTheme::parse(s).is_some()),
    ] {
        if m.get(key).is_some_and(|x| !x.as_str().is_some_and(ok)) {
            m.remove(key);
        }
    }
    if m.contains_key("appearanceMode") {
        return;
    }
    let Some(b) = m.get("brightness").and_then(Value::as_str).and_then(Brightness::parse) else { return };
    if let Some(t) = DarkTheme::of(b) {
        m.insert("appearanceMode".into(), json!("dark"));
        m.insert("darkTheme".into(), serde_json::to_value(t).unwrap_or(Value::Null));
    } else if let Some(t) = LightTheme::of(b) {
        m.insert("appearanceMode".into(), json!("light"));
        m.insert("lightTheme".into(), serde_json::to_value(t).unwrap_or(Value::Null));
    }
}

impl UiState {
    /// Read the saved UI state (`ui.json`), migrating older appearance settings.
    pub fn from_saved_json(bytes: &[u8]) -> Result<UiState, String> {
        let mut v: Value = serde_json::from_slice(bytes).map_err(|e| format!("UI state: {e}"))?;
        migrate_saved(&mut v);
        serde_json::from_value(v).map_err(|e| format!("UI state: {e}"))
    }
}

/// The header button: a monitor (Auto), sun (Light) or moon (Dark) for the current mode.
pub fn header_button(app: &mut DesignApp, ui: &mut egui::Ui) {
    let mode = app.ui.appearance_mode;
    let icon = match mode {
        AppearanceMode::Auto => "monitor",
        AppearanceMode::Light => "sun",
        AppearanceMode::Dark => "moon",
    };
    let lang = app.ui.language.clone();
    let tip = format!("{}: {}", crate::i18n::tr(&lang, "Appearance Mode"), crate::i18n::tr(&lang, mode.label()));
    if crate::icons::button(ui, icon, 22.0, false, &tip).clicked() {
        let _ = app.run("window.nextAppearanceMode", json!({}));
    }
}

/// Fields of the Preferences dialog for the current settings.
pub fn dialog_fields(ui: &UiState, f: &mut Value) {
    f["appearanceMode"] = json!(ui.appearance_mode.id());
    f["darkTheme"] = json!(ui.dark_theme.brightness().id());
    f["lightTheme"] = json!(ui.light_theme.brightness().id());
}

/// Preferences › Interface: the Appearance Mode selector above the light and dark theme cards.
pub fn preferences_rows(lang: &str, system: Option<egui::Theme>, ui: &mut egui::Ui, d: &mut crate::dialogs::Dialog) {
    let tr = |s| crate::i18n::tr(lang, s);
    let t = Tokens::get(ui.ctx());
    crate::rtl::label(ui, egui::RichText::new(tr("Appearance")).font(crate::theme::semibold(12.0)));
    let mode = d.fields.get("appearanceMode").and_then(Value::as_str).and_then(AppearanceMode::parse).unwrap_or_default();
    ui.horizontal(|ui| {
        crate::rtl::label(ui, tr("Appearance Mode"));
        let label = |m: AppearanceMode| tr(if m == AppearanceMode::Auto { "Sync with system" } else { m.label() });
        egui::ComboBox::from_id_salt("pref_appearance_mode").selected_text(crate::rtl::widget(ui, label(mode))).width(160.0).show_ui(ui, |ui| {
            for m in AppearanceMode::ALL {
                if ui.selectable_label(m == mode, crate::rtl::widget(ui, label(m))).clicked() {
                    d.fields.insert("appearanceMode".into(), json!(m.id()));
                }
            }
        });
    });
    ui.add_space(4.0);
    let mode = d.fields.get("appearanceMode").and_then(Value::as_str).and_then(AppearanceMode::parse).unwrap_or_default();
    let light_active = mode == AppearanceMode::Light || (mode == AppearanceMode::Auto && system == Some(egui::Theme::Light));
    let light: Vec<Brightness> = LightTheme::ALL.iter().map(|t| t.brightness()).collect();
    let dark: Vec<Brightness> = DarkTheme::ALL.iter().map(|t| t.brightness()).collect();
    ui.horizontal_top(|ui| {
        theme_card(lang, ui, &t, "Light Theme", light_active, "lightTheme", &light, d);
        ui.add_space(6.0);
        theme_card(lang, ui, &t, "Dark Theme", !light_active, "darkTheme", &dark, d);
    });
    ui.add_space(8.0);
}

const CARD_WIDTH: f32 = 186.0;

#[allow(clippy::too_many_arguments)]
fn theme_card(
    lang: &str,
    ui: &mut egui::Ui,
    t: &Tokens,
    title: &str,
    active: bool,
    key: &str,
    options: &[Brightness],
    d: &mut crate::dialogs::Dialog,
) {
    let Some(first) = options.first().copied() else { return };
    let current = d.fields.get(key).and_then(Value::as_str).and_then(Brightness::parse).filter(|b| options.contains(b)).unwrap_or(first);
    egui::Frame::new()
        .fill(t.panel_darker)
        .stroke(Stroke::new(1.0, if active { t.accent } else { t.divider }))
        .corner_radius(3.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(CARD_WIDTH - 16.0);
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, title)).font(crate::theme::semibold(11.5)));
                    if active {
                        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "Active")).color(t.accent).small());
                    }
                });
                ui.add_space(2.0);
                let (r, _) = ui.allocate_exact_size(vec2(CARD_WIDTH - 16.0, 92.0), Sense::hover());
                paint_preview(ui.painter(), r, current);
                ui.add_space(4.0);
                for b in options {
                    if ui.radio(*b == current, crate::rtl::widget(ui, crate::i18n::tr(lang, b.label()))).clicked() {
                        d.fields.insert(key.into(), json!(b.id()));
                    }
                }
            });
        });
}

/// Colours of the page content in the preview (paper, guides, ink): document colours, not
/// interface tokens, the same in every theme as they are on the canvas.
const PAPER: Color32 = Color32::from_rgb(252, 252, 250);
const MARGIN_GUIDE: Color32 = Color32::from_rgb(214, 96, 204);
const COLUMN_GUIDE: Color32 = Color32::from_rgb(150, 118, 232);
const INK: Color32 = Color32::from_rgb(150, 150, 150);
const HEADLINE: Color32 = Color32::from_rgb(92, 34, 84);
const SKY: Color32 = Color32::from_rgb(236, 150, 96);
const HILLS: Color32 = Color32::from_rgb(104, 48, 108);
const QUOTE: Color32 = Color32::from_rgb(252, 242, 226);

/// A miniature DesignCraft window painted with theme `b`'s tokens: the app bar, the Tools panel,
/// a document tab with rulers, a spread on the pasteboard, the panel icons and the Properties
/// panel, and the status bar.
pub fn paint_preview(p: &egui::Painter, rect: Rect, b: Brightness) {
    let t = Tokens::for_brightness(b);
    p.rect_filled(rect, 2.0, t.pasteboard);
    if rect.width() < 120.0 || rect.height() < 60.0 {
        return;
    }
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
    let bar = |x: f32, y: f32, w: f32, c: Color32| p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(w, 1.6)), 0.5, c);
    let (l, top, right, bottom) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    // App bar: home, menus, then the workspace switcher and the search field on the right.
    let app_bar = r(l, top, right, top + 11.0);
    p.rect_filled(app_bar, egui::CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, t.app_bar);
    p.rect_stroke(r(l + 3.5, top + 3.5, l + 7.5, top + 7.5), 0.0, Stroke::new(0.8, t.icon), egui::StrokeKind::Middle);
    let mut x = l + 12.0;
    for w in [7.0, 7.0, 9.0, 7.0, 8.0, 7.0] {
        bar(x, top + 4.8, w, t.text_dim);
        x += w + 3.0;
    }
    let search = r(right - 26.0, top + 2.5, right - 3.0, top + 8.5);
    p.rect_filled(search, 0.5, t.input);
    p.rect_stroke(search, 0.5, Stroke::new(0.6, t.field_border), egui::StrokeKind::Inside);
    bar(right - 40.0, top + 4.8, 11.0, t.text);
    // Status bar.
    let status = r(l, bottom - 6.0, right, bottom);
    p.rect_filled(status, egui::CornerRadius { nw: 0, ne: 0, sw: 2, se: 2 }, t.app_bar);
    bar(l + 3.0, bottom - 3.8, 10.0, t.text_dim);
    bar(l + 22.0, bottom - 3.8, 16.0, t.text_dim);
    p.circle_filled(pos2(l + 44.0, bottom - 3.0), 1.2, Color32::from_rgb(70, 190, 90));
    let body_top = app_bar.bottom() + 2.0;
    let body_bottom = status.top();
    // Tools panel.
    let tools = r(l, body_top, l + 11.0, body_bottom);
    p.rect_filled(tools, 0.0, t.panel);
    p.line_segment([tools.right_top(), tools.right_bottom()], Stroke::new(0.6, t.border));
    for i in 0..8 {
        let y = body_top + 4.0 + i as f32 * 7.0;
        if y + 5.0 > body_bottom {
            break;
        }
        let cell = r(l + 3.0, y, l + 8.0, y + 4.5);
        if i == 3 {
            p.rect_filled(cell.expand(1.0), 0.5, t.well);
        }
        p.rect_stroke(cell, 0.0, Stroke::new(0.7, t.icon), egui::StrokeKind::Middle);
    }
    // Panel dock: an icon column, then the Properties panel with its tabs and fields.
    let dock = r(right - 62.0, body_top, right, body_bottom);
    p.rect_filled(dock, 0.0, t.panel);
    p.line_segment([dock.left_top(), dock.left_bottom()], Stroke::new(0.6, t.border));
    let icons_right = dock.left() + 10.0;
    p.line_segment([pos2(icons_right, body_top), pos2(icons_right, body_bottom)], Stroke::new(0.5, t.divider));
    for i in 0..8 {
        let y = body_top + 4.0 + i as f32 * 7.5;
        if y + 4.0 > body_bottom {
            break;
        }
        p.rect_stroke(r(dock.left() + 3.0, y, dock.left() + 7.0, y + 4.0), 0.5, Stroke::new(0.7, t.icon), egui::StrokeKind::Middle);
    }
    let panel_left = icons_right;
    let tabs = r(panel_left, body_top, right, body_top + 8.0);
    p.rect_filled(tabs, 0.0, t.tab_strip);
    p.rect_filled(r(panel_left, body_top, panel_left + 19.0, body_top + 8.0), 0.0, t.panel);
    bar(panel_left + 3.0, body_top + 3.2, 13.0, t.text_strong);
    bar(panel_left + 22.0, body_top + 3.2, 9.0, t.text_dim);
    bar(panel_left + 35.0, body_top + 3.2, 9.0, t.text_dim);
    let mut y = tabs.bottom() + 4.0;
    for (section, rows) in [(14.0, 2), (16.0, 2), (12.0, 1)] {
        if y + 6.0 > body_bottom {
            break;
        }
        bar(panel_left + 3.0, y, section, t.text);
        y += 4.0;
        for _ in 0..rows {
            if y + 5.0 > body_bottom {
                break;
            }
            for fx in [panel_left + 3.0, panel_left + 26.0] {
                bar(fx, y + 1.6, 3.0, t.text_dim);
                let field = r(fx + 5.0, y, fx + 21.0, y + 4.5);
                p.rect_filled(field, 0.5, t.input);
                p.rect_stroke(field, 0.5, Stroke::new(0.5, t.field_border), egui::StrokeKind::Inside);
            }
            y += 6.5;
        }
        y += 2.0;
    }
    if y + 5.0 < body_bottom {
        // Text Frame › Align: the selected choice.
        p.rect_filled(r(panel_left + 3.0, y, panel_left + 12.0, y + 4.5), 0.5, t.accent_strong);
        for k in 0..3 {
            bar(panel_left + 15.0 + k as f32 * 11.0, y + 1.6, 7.0, t.text_dim);
        }
    }
    // Document: tab strip, rulers, pasteboard and a spread.
    let doc = r(tools.right() + 1.0, body_top, dock.left() - 1.0, body_bottom);
    let doc_tabs = r(doc.left(), doc.top(), doc.right(), doc.top() + 7.0);
    p.rect_filled(doc_tabs, 0.0, t.tab_strip);
    p.rect_filled(r(doc.left() + 6.0, doc.top(), doc.left() + 44.0, doc.top() + 7.0), 0.0, t.panel);
    bar(doc.left() + 10.0, doc.top() + 2.8, 28.0, t.text_strong);
    let ruler_h = r(doc.left(), doc_tabs.bottom(), doc.right(), doc_tabs.bottom() + 4.0);
    let ruler_v = r(doc.left(), ruler_h.top(), doc.left() + 4.0, doc.bottom());
    p.rect_filled(ruler_h, 0.0, t.ruler);
    p.rect_filled(ruler_v, 0.0, t.ruler);
    let mut tx = ruler_v.right() + 3.0;
    while tx < ruler_h.right() {
        p.line_segment([pos2(tx, ruler_h.bottom() - 1.5), pos2(tx, ruler_h.bottom())], Stroke::new(0.5, t.ruler_tick));
        tx += 6.0;
    }
    let mut ty = ruler_h.bottom() + 3.0;
    while ty < ruler_v.bottom() {
        p.line_segment([pos2(ruler_v.right() - 1.5, ty), pos2(ruler_v.right(), ty)], Stroke::new(0.5, t.ruler_tick));
        ty += 6.0;
    }
    let area = r(ruler_v.right(), ruler_h.bottom(), doc.right(), doc.bottom()).shrink(4.0);
    if area.width() < 20.0 || area.height() < 14.0 {
        return;
    }
    // Two letter pages (8.5 × 11) side by side, as large as the area allows.
    let page_h = area.height().min(area.width() / 2.0 * 11.0 / 8.5);
    let page_w = page_h * 8.5 / 11.0;
    let spread = Rect::from_center_size(area.center(), vec2(page_w * 2.0, page_h));
    p.rect_filled(spread.translate(vec2(0.8, 0.8)), 0.0, Color32::from_black_alpha(70));
    p.rect_filled(spread, 0.0, PAPER);
    let thin = |c: Color32| Stroke::new(0.5, c);
    for (i, page) in
        [r(spread.left(), spread.top(), spread.center().x, spread.bottom()), r(spread.center().x, spread.top(), spread.right(), spread.bottom())]
            .into_iter()
            .enumerate()
    {
        let m = page.shrink(page_w * 0.09);
        p.rect_stroke(m, 0.0, thin(MARGIN_GUIDE), egui::StrokeKind::Middle);
        let cols = if i == 0 { 2 } else { 3 };
        let gutter = m.width() * 0.04;
        let col_w = (m.width() - gutter * (cols - 1) as f32) / cols as f32;
        for c in 1..cols {
            let gx = m.left() + c as f32 * col_w + (c - 1) as f32 * gutter;
            for x in [gx, gx + gutter] {
                p.line_segment([pos2(x, m.top()), pos2(x, m.bottom())], thin(COLUMN_GUIDE));
            }
        }
        let lines = |x0: f32, y0: f32, y1: f32, w: f32| {
            let mut y = y0;
            while y + 0.8 <= y1 {
                p.rect_filled(r(x0, y, x0 + w, y + 0.7), 0.0, INK);
                y += 2.0;
            }
        };
        if i == 0 {
            // Headline, a photograph of hills at sunset, then two columns of text.
            p.rect_filled(r(m.left(), m.top() + 2.0, m.left() + m.width() * 0.75, m.top() + 4.5), 0.0, HEADLINE);
            let photo = r(m.left(), m.top() + m.height() * 0.24, m.right(), m.top() + m.height() * 0.62);
            p.rect_filled(photo, 0.0, SKY);
            let hills: Vec<Pos2> = vec![
                pos2(photo.left(), photo.bottom()),
                pos2(photo.left(), photo.top() + photo.height() * 0.62),
                pos2(photo.center().x, photo.top() + photo.height() * 0.48),
                pos2(photo.right(), photo.top() + photo.height() * 0.68),
                pos2(photo.right(), photo.bottom()),
            ];
            p.add(egui::Shape::convex_polygon(hills, HILLS, Stroke::NONE));
            p.circle_filled(
                pos2(photo.left() + photo.width() * 0.62, photo.top() + photo.height() * 0.42),
                photo.height() * 0.16,
                Color32::from_rgb(250, 214, 120),
            );
            for c in 0..cols {
                let x0 = m.left() + c as f32 * (col_w + gutter);
                lines(x0, photo.bottom() + 3.0, m.bottom(), col_w);
            }
        } else {
            // Three threaded columns with a pull quote, the frame selected.
            let quote = r(m.left() + col_w + gutter, m.top() + m.height() * 0.32, m.right(), m.top() + m.height() * 0.48);
            for c in 0..cols {
                let x0 = m.left() + c as f32 * (col_w + gutter);
                if c == 0 {
                    lines(x0, m.top(), m.bottom(), col_w);
                } else {
                    lines(x0, m.top(), quote.top() - 1.0, col_w);
                    lines(x0, quote.bottom() + 1.5, m.bottom() - m.height() * 0.1, col_w);
                }
            }
            p.rect_filled(quote, 0.0, QUOTE);
            p.rect_filled(r(quote.left() + 3.0, quote.center().y - 1.6, quote.right() - 4.0, quote.center().y - 0.6), 0.0, HEADLINE);
            p.rect_filled(r(quote.left() + 3.0, quote.center().y + 0.8, quote.center().x, quote.center().y + 1.8), 0.0, HEADLINE);
            let frame = m.expand(0.5);
            p.rect_stroke(frame, 0.0, Stroke::new(0.7, t.accent), egui::StrokeKind::Middle);
            for c in [frame.left_top(), frame.right_top(), frame.left_bottom(), frame.right_bottom(), frame.center_top(), frame.center_bottom()] {
                p.rect_filled(Rect::from_center_size(c, vec2(1.6, 1.6)), 0.0, t.accent);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> DesignApp {
        DesignApp::new(designcraft_engine::Session::new(), crate::Services::default())
    }

    #[test]
    fn defaults_keep_the_current_look_and_auto_is_opt_in() {
        let ui = UiState::default();
        assert_eq!(ui.appearance_mode, AppearanceMode::Dark);
        assert_eq!(ui.dark_theme, DarkTheme::MediumDark);
        assert_eq!(ui.light_theme, LightTheme::Light);
        assert_eq!(selected(&ui, Some(egui::Theme::Light)), Brightness::MediumDark);
        assert_eq!(selected(&ui, None), ui.brightness);
    }

    #[test]
    fn saved_single_theme_migrates_to_a_fixed_mode() {
        for (saved, mode, dark, light) in [
            ("Light", AppearanceMode::Light, DarkTheme::MediumDark, LightTheme::Light),
            ("MediumLight", AppearanceMode::Light, DarkTheme::MediumDark, LightTheme::MediumLight),
            ("Dark", AppearanceMode::Dark, DarkTheme::Dark, LightTheme::Light),
            ("HighContrast", AppearanceMode::Dark, DarkTheme::HighContrast, LightTheme::Light),
        ] {
            let ui = UiState::from_saved_json(json!({"brightness": saved, "rulers": false}).to_string().as_bytes()).unwrap();
            assert_eq!((ui.appearance_mode, ui.dark_theme, ui.light_theme), (mode, dark, light), "{saved}");
            assert!(!ui.rulers, "the rest of the file still loads");
            assert_eq!(selected(&ui, Some(egui::Theme::Dark)), Brightness::parse(saved).unwrap());
        }
        // A file written with appearance modes keeps them; a bad value falls back to its default
        // instead of discarding the whole file.
        let ui = UiState::from_saved_json(br#"{"brightness":"Light","appearanceMode":"auto","darkTheme":"highContrast","lightTheme":"mediumLight"}"#)
            .unwrap();
        assert_eq!((ui.appearance_mode, ui.dark_theme, ui.light_theme), (AppearanceMode::Auto, DarkTheme::HighContrast, LightTheme::MediumLight));
        let ui = UiState::from_saved_json(br#"{"appearanceMode":"sepia","darkTheme":"light","lightTheme":7,"rulers":false}"#).unwrap();
        assert_eq!((ui.appearance_mode, ui.dark_theme, ui.light_theme), (AppearanceMode::Dark, DarkTheme::MediumDark, LightTheme::Light));
        assert!(!ui.rulers);
        // Round trip.
        let ui = UiState { appearance_mode: AppearanceMode::Auto, dark_theme: DarkTheme::Dark, ..UiState::default() };
        let back = UiState::from_saved_json(&serde_json::to_vec(&ui).unwrap()).unwrap();
        assert_eq!((back.appearance_mode, back.dark_theme, back.light_theme), (ui.appearance_mode, ui.dark_theme, ui.light_theme));
        assert!(UiState::from_saved_json(b"not json").is_err());
    }

    #[test]
    fn choices_validate_and_the_legacy_theme_selects_its_mode() {
        let mut a = app();
        a.run("window.appearanceMode", json!({"mode": "auto", "darkTheme": "highContrast", "lightTheme": "mediumLight"})).unwrap();
        assert_eq!(
            (a.ui.appearance_mode, a.ui.dark_theme, a.ui.light_theme),
            (AppearanceMode::Auto, DarkTheme::HighContrast, LightTheme::MediumLight)
        );
        for bad in
            [json!({"mode": "sepia"}), json!({"darkTheme": "light"}), json!({"lightTheme": "dark"}), json!({"mode": 3}), json!({"lightTheme": []})]
        {
            assert!(a.run("window.appearanceMode", bad.clone()).is_err(), "{bad}");
        }
        // Nothing changed by a rejected call, even when one of its values was valid.
        assert!(a.run("window.appearanceMode", json!({"mode": "light", "darkTheme": "nope"})).is_err());
        assert_eq!(a.ui.appearance_mode, AppearanceMode::Auto);
        // The single-theme command (menu, `ui.set {brightness}`) fixes the mode to the family.
        a.run("window.brightness", json!({"brightness": "dark"})).unwrap();
        assert_eq!((a.ui.appearance_mode, a.ui.dark_theme, a.ui.light_theme), (AppearanceMode::Dark, DarkTheme::Dark, LightTheme::MediumLight));
        assert_eq!(a.ui.brightness, Brightness::Dark);
        a.run("window.brightness", json!({"brightness": "light"})).unwrap();
        assert_eq!((a.ui.appearance_mode, a.ui.dark_theme, a.ui.light_theme), (AppearanceMode::Light, DarkTheme::Dark, LightTheme::Light));
        assert_eq!(a.ui.brightness, Brightness::Light);
        assert_eq!(crate::menus::checked(&a, "window.brightness", &json!({"brightness": "light"})), Some(true));
        assert_eq!(crate::menus::checked(&a, "window.appearanceMode", &json!({"mode": "light"})), Some(true));
    }

    #[test]
    fn the_button_cycles_auto_light_dark_without_losing_theme_choices() {
        let mut a = app();
        a.run("window.appearanceMode", json!({"darkTheme": "highContrast", "lightTheme": "mediumLight"})).unwrap();
        a.system_theme = Some(egui::Theme::Dark);
        for (mode, shown) in [
            (AppearanceMode::Auto, Brightness::HighContrast),
            (AppearanceMode::Light, Brightness::MediumLight),
            (AppearanceMode::Dark, Brightness::HighContrast),
            (AppearanceMode::Auto, Brightness::HighContrast),
        ] {
            a.run("window.nextAppearanceMode", json!({})).unwrap();
            assert_eq!(a.ui.appearance_mode, mode);
            assert_eq!(a.ui.brightness, shown);
            assert_eq!((a.ui.dark_theme, a.ui.light_theme), (DarkTheme::HighContrast, LightTheme::MediumLight));
        }
    }

    #[test]
    fn auto_follows_a_stubbed_system_theme_and_falls_back_to_dark() {
        use std::sync::{Arc, Mutex};
        let system = Arc::new(Mutex::new(Some(egui::Theme::Light)));
        let reader = Arc::clone(&system);
        let mut a = DesignApp::new(
            designcraft_engine::Session::new(),
            crate::Services { system_theme: Some(Box::new(move |_| *reader.lock().unwrap())), ..Default::default() },
        );
        a.run("window.appearanceMode", json!({"mode": "auto"})).unwrap();
        let ctx = egui::Context::default();
        a.logic(&ctx);
        assert_eq!(a.ui.brightness, Brightness::Light);
        assert!(!crate::theme::Tokens::get(&ctx).dark, "the light tokens are applied");
        *system.lock().unwrap() = Some(egui::Theme::Dark);
        a.logic(&ctx);
        assert_eq!(a.ui.brightness, Brightness::MediumDark);
        assert!(crate::theme::Tokens::get(&ctx).dark);
        // No answer from the system: Dark. A fixed mode ignores the system.
        *system.lock().unwrap() = None;
        a.run("window.appearanceMode", json!({"mode": "light"})).unwrap();
        a.logic(&ctx);
        assert_eq!(a.ui.brightness, Brightness::Light);
        a.run("window.appearanceMode", json!({"mode": "auto"})).unwrap();
        a.logic(&ctx);
        assert_eq!(a.ui.brightness, Brightness::MediumDark);
    }

    #[test]
    fn theme_card_stacks_the_preview_and_choices_below_its_title() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        let mut dialog = crate::dialogs::Dialog::new("preferences", json!({"lightTheme": "light"}));
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.horizontal_top(|ui| {
                theme_card(
                    "en",
                    ui,
                    &Tokens::for_brightness(Brightness::MediumDark),
                    "Light Theme",
                    false,
                    "lightTheme",
                    &[Brightness::Light, Brightness::MediumLight],
                    &mut dialog,
                );
            });
        });
        output.textures_delta.clear();
        let text_y = |label: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => Some(text.pos.y),
                    _ => None,
                })
                .unwrap()
        };
        assert!(text_y("Light") > text_y("Light Theme") + 92.0);
        assert!(text_y("Medium Light") > text_y("Light"));
    }

    #[test]
    fn preview_paints_each_theme_with_its_own_tokens() {
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for b in Brightness::ALL {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                paint_preview(ui.painter(), Rect::from_min_size(pos2(0.0, 0.0), vec2(170.0, 92.0)), b);
                // Too small to draw anything but the background: no panic.
                paint_preview(ui.painter(), Rect::from_min_size(pos2(0.0, 0.0), vec2(4.0, 4.0)), b);
            });
            out.textures_delta.clear();
            shapes.push(format!("{:?}", out.shapes.iter().map(|s| &s.shape).collect::<Vec<_>>()));
        }
        for (i, s) in shapes.iter().enumerate() {
            assert!(shapes[..i].iter().all(|o| o != s), "{:?} draws like another theme", Brightness::ALL[i]);
        }
    }
}
