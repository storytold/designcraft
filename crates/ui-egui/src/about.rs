//! Help › About DesignCraft: tabs About · Contributors · Models. About is the splash with the
//! ArtCraft mark, version, and community links (Discord first and largest), plus the other ArtCraft
//! apps; Contributors and Models are the compiled-in credits (`crate::credits`).

use designcraft_engine::links;
use egui::{Color32, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::DesignApp;
use crate::theme::{Tokens, semibold};

/// ArtCraft brand blue (from `docs/brand/artcraft-mark.svg`).
pub const BRAND: Color32 = Color32::from_rgb(0x4e, 0x7b, 0xfb);
/// Discord's brand colour, used for the "Join our Discord" buttons (colour only; no Discord artwork).
pub const DISCORD: Color32 = Color32::from_rgb(0x58, 0x65, 0xf2);

/// The ArtCraft mark outline (viewBox -10 -19.42 136.34 136.34), from `docs/brand/artcraft-mark.svg`.
const MARK_PATH: &str = "M104.28,49.49L81.55,0h-31.23l-3.17,4.76L14.75,53.63,0,75.85l21.55,21.55,63.79-36.94,16.99,37.04,14.01-21.74-12.06-26.27ZM32.89,65.66l32.42-48.87,10.91,23.77-43.32,25.09Z";

/// The other ArtCraft apps (app-page slug, name, what it is) — same list as the README.
pub const SIBLINGS: &[(&str, &str, &str)] = &[
    ("photocraft", "PhotoCraft", "image editing"),
    ("drawcraft", "VectorCraft", "vector illustration"),
    ("filmcraft", "FilmCraft", "video editing, color and sound"),
    ("lightcraft", "LightCraft", "photo library and raw development"),
    ("pdfcraft", "PdfCraft", "reading, organizing and protecting PDFs"),
    ("effectcraft", "EffectCraft", "motion graphics and visual effects"),
];

/// The About window's tabs, in `UiState::about_tab` order (`help.about {tab}` names them in lowercase).
pub const ABOUT_TABS: [&str; 3] = ["About", "Contributors", "Models"];

/// Paint the ArtCraft mark into `rect` (square), rasterised once per size and cached.
pub fn paint_mark(ui: &egui::Ui, rect: Rect, color: Color32) {
    use designcraft_render::vello_cpu::{self, kurbo, peniko};
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let px = ((rect.width().min(rect.height()) * ppp).round() as u32).clamp(8, 1024);
    let key = egui::Id::new(("artcraft_mark", px, color.to_array()));
    let tex: Option<egui::TextureHandle> = ctx.data(|d| d.get_temp(key));
    let tex = tex.unwrap_or_else(|| {
        let mut rc = vello_cpu::RenderContext::new_with(px as u16, px as u16, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
        let k = px as f64 / 136.34;
        rc.set_transform(kurbo::Affine::scale(k) * kurbo::Affine::translate((10.0, 19.42)));
        let [r, g, b, a] = color.to_srgba_unmultiplied();
        rc.set_paint(peniko::Color::from_rgba8(r, g, b, a));
        if let Ok(p) = kurbo::BezPath::from_svg(MARK_PATH) {
            rc.fill_path(&p);
        }
        rc.flush();
        let mut pm = vello_cpu::Pixmap::new(px as u16, px as u16);
        let mut res = vello_cpu::Resources::new();
        rc.render(&mut pm, &mut res);
        let ci = egui::ColorImage::from_rgba_premultiplied([px as usize, px as usize], pm.data_as_u8_slice());
        let t = ctx.load_texture("artcraft_mark", ci, egui::TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(key, t.clone()));
        t
    });
    let side = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(side, side));
    ui.painter().image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}

/// A speech-bubble glyph (drawn in code) for Discord buttons.
pub fn paint_chat_icon(p: &egui::Painter, c: egui::Pos2, s: f32, color: Color32) {
    let body = Rect::from_center_size(c - vec2(0.0, s * 0.06), vec2(s, s * 0.72));
    p.rect_filled(body, s * 0.24, color);
    let tail = vec![
        pos2(body.left() + s * 0.22, body.bottom() - 1.0),
        pos2(body.left() + s * 0.12, body.bottom() + s * 0.24),
        pos2(body.left() + s * 0.44, body.bottom() - 1.0),
    ];
    p.add(egui::Shape::convex_polygon(tail, color, Stroke::NONE));
    let eye = s * 0.09;
    let bg = if color.r() as u32 + color.g() as u32 + color.b() as u32 > 380 { DISCORD } else { Color32::WHITE };
    p.circle_filled(body.center() - vec2(s * 0.18, 0.0), eye, bg);
    p.circle_filled(body.center() + vec2(s * 0.18, 0.0), eye, bg);
}

/// The big "Join our Discord" button. Returns true when clicked.
pub fn discord_button(ui: &mut egui::Ui, label: &str, size: egui::Vec2) -> bool {
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let resp = resp.on_hover_text(links::DISCORD).on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if resp.hovered() { Color32::from_rgb(0x47, 0x52, 0xc4) } else { DISCORD };
    ui.painter().rect_filled(r, size.y / 2.0, fill);
    let icon = size.y * 0.5;
    let font = semibold((size.y * 0.42).max(11.0));
    let galley = crate::rtl::plain(ui.ctx(), label, font, Color32::WHITE);
    let total = icon + 8.0 + galley.size().x;
    let x0 = r.center().x - total / 2.0;
    paint_chat_icon(ui.painter(), pos2(x0 + icon / 2.0, r.center().y), icon, Color32::WHITE);
    ui.painter().galley(pos2(x0 + icon + 8.0, r.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
    resp.clicked()
}

/// A text link that opens `url`. Returns true when clicked.
fn link(ui: &mut egui::Ui, text: &str, url: &str, color: Color32) -> bool {
    let resp = ui.add(egui::Label::new(egui::RichText::new(text).size(13.0).color(color).underline()).sense(Sense::click()));
    let resp = resp.on_hover_text(url).on_hover_cursor(egui::CursorIcon::PointingHand);
    resp.clicked()
}

pub fn show(app: &mut DesignApp, ctx: &egui::Context) {
    if !app.ui.about {
        return;
    }
    let t = Tokens::get(ctx);
    let mut open: Option<&'static str> = None;
    let resp = egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
        ui.set_width(if app.ui.about_tab == 0 { 460.0 } else { 640.0 });
        ui.horizontal(|ui| {
            for (i, l) in ABOUT_TABS.iter().enumerate() {
                let i = i as u8;
                if ui.selectable_label(app.ui.about_tab == i, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                    app.ui.about_tab = i;
                }
            }
        });
        ui.separator();
        if app.ui.about_tab != 0 {
            let h = 400.0_f32.min(ctx.content_rect().height() * 0.7).max(160.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), h), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_min_height(h);
                if app.ui.about_tab == 1 {
                    crate::credits::contributors_ui(ui);
                } else {
                    crate::credits::models_ui(ui);
                }
            });
            ui.separator();
            ui.vertical_centered(|ui| {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "  Close  "))).clicked() {
                    app.ui.about = false;
                }
            });
            return;
        }
        ui.vertical_centered(|ui| {
            ui.add_space(8.0);
            let (r, _) = ui.allocate_exact_size(vec2(72.0, 72.0), Sense::hover());
            paint_mark(ui, r, BRAND);
            ui.add_space(6.0);
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "DesignCraft")).font(semibold(26.0)).color(t.text_strong),
            ));
            crate::rtl::label(
                ui,
                egui::RichText::new(format!("{} {}", crate::i18n::tr(&app.ui.language, "Version"), env!("CARGO_PKG_VERSION")))
                    .size(12.0)
                    .color(t.text_dim),
            );
            ui.add_space(4.0);
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page layout for print and screen — fast, open and scriptable."))
                    .size(13.0)
                    .color(t.text),
            ));
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Part of the ArtCraft suite of open creative tools."))
                    .size(12.0)
                    .color(t.text_dim),
            ));
            ui.add_space(16.0);
            if discord_button(ui, "Join the ArtCraft Discord", vec2(300.0, 40.0)) {
                open = Some("discord");
            }
            ui.add_space(4.0);
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "discord.gg/artcraft — help, feedback and show-and-tell"))
                    .size(11.0)
                    .color(t.text_dim),
            ));
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                // Centre the row of links.
                let w = 330.0;
                ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                for (text, key) in
                    [("DesignCraft page", "appPage"), ("GitHub", "github"), ("getartcraft.com", "website"), ("Report an issue", "issues")]
                {
                    if link(ui, text, links::get(key).unwrap_or_default(), t.accent) {
                        open = Some(key);
                    }
                    ui.add_space(6.0);
                }
            });
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(6.0);
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "More ArtCraft apps")).font(semibold(12.0)).color(t.text_strong),
            ));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let pad = ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x;
                let w: f32 = SIBLINGS
                    .iter()
                    .map(|(_, n, _)| ui.painter().layout_no_wrap(n.to_string(), egui::FontId::proportional(12.0), Color32::WHITE).size().x + pad)
                    .sum::<f32>()
                    - ui.spacing().item_spacing.x;
                ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                for (slug, name, what) in SIBLINGS {
                    let url = format!("{}/apps/{slug}", links::WEBSITE);
                    let b =
                        ui.add(egui::Button::new(egui::RichText::new(*name).size(12.0)).corner_radius(10.0)).on_hover_text(format!("{what} — {url}"));
                    if b.clicked() {
                        app.ui.pending_urls.push(url);
                    }
                }
            });
            ui.add_space(12.0);
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Open source. No telemetry.")).size(11.0).color(t.text_dim),
            ));
            ui.add_space(8.0);
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "  Close  "))).clicked() {
                app.ui.about = false;
            }
        });
    });
    if resp.should_close() {
        app.ui.about = false;
    }
    if let Some(k) = open {
        let _ = app.run(&format!("help.{k}"), json!({}));
    }
}
