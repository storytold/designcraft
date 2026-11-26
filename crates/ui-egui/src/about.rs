//! Help › About DesignCraft: tabs About · Contributors · Models. About is the splash with the
//! ArtCraft mark, version, and community links (Discord first and largest), plus the other ArtCraft
//! apps; Contributors and Models are the compiled-in credits (`crate::credits`).

use designcraft_engine::links;
use egui::{Color32, Rect, Stroke, pos2, vec2};
use serde_json::json;

use crate::DesignApp;

/// ArtCraft brand blue (from `docs/brand/artcraft-mark.svg`).
pub const BRAND: Color32 = Color32::from_rgb(0x4e, 0x7b, 0xfb);
/// Discord's brand colour, used for the "Join our Discord" buttons (colour only; no Discord artwork).
pub const DISCORD: Color32 = Color32::from_rgb(0x58, 0x65, 0xf2);

/// The ArtCraft mark outline (viewBox -10 -19.42 136.34 136.34), from `docs/brand/artcraft-mark.svg`.
const MARK_PATH: &str = "M104.28,49.49L81.55,0h-31.23l-3.17,4.76L14.75,53.63,0,75.85l21.55,21.55,63.79-36.94,16.99,37.04,14.01-21.74-12.06-26.27ZM32.89,65.66l32.42-48.87,10.91,23.77-43.32,25.09Z";

/// The other ArtCraft apps (app-page slug, name, what it is) — same list as the README.
pub const SIBLINGS: &[(&str, &str, &str)] = &[
    ("photocraft", "PhotoCraft", "image editing"),
    ("vectorcraft", "VectorCraft", "vector illustration"),
    ("filmcraft", "FilmCraft", "video editing, color and sound"),
    ("lightcraft", "LightCraft", "photo library and raw development"),
    ("pdfcraft", "PdfCraft", "reading, organizing and protecting PDFs"),
    ("effectcraft", "EffectCraft", "motion graphics and visual effects"),
];

/// The About window's tabs, in `UiState::about_tab` order (`help.about {tab}` names them in lowercase).
pub const ABOUT_TABS: [&str; 3] = ["About", "Contributors", "Models"];

/// Paint the ArtCraft mark into `rect` (square), rasterised once per size and cached, on a painter that is not a `Ui`'s (a framework picture's).
pub fn paint_mark_in(ctx: &egui::Context, painter: &egui::Painter, rect: Rect, color: Color32) {
    use designcraft_render::vello_cpu::{self, kurbo, peniko};
    let ppp = ctx.pixels_per_point();
    let px = ((rect.width().min(rect.height()) * ppp).round() as u32).clamp(8, 1024);
    let key = egui::Id::unique(("artcraft_mark", px, color.to_array()));
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
        let t = crate::widgets::load_texture(ctx, "artcraft_mark", ci, egui::TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(key, t.clone()));
        t
    });
    let side = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(side, side));
    painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
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

pub fn show(app: &mut DesignApp, ctx: &egui::Context) {
    if !app.ui.about {
        return;
    }
    framework::show(app, ctx);
}

/// Help › About on the framework: a dialog with its three tabs, the same links, commands and credits.
mod framework {
    use super::{ABOUT_TABS, BRAND, DesignApp, SIBLINGS, json, links, paint_chat_icon, paint_mark_in};
    use craft_ui_panels::dialog::Dialog as FrameworkDialog;
    use craft_ui_panels::sections::{DialogRole, FooterButton};
    use craft_ui_widgets::buttons::{Button, Link};
    use craft_ui_widgets::choice::Segmented;
    use craft_ui_widgets::contract::{Bind, Entry, LeastWidth, Props as _, Show as _, ShowContainer as _, Sizing};
    use craft_ui_widgets::display::{Divider, Picture, PictureSize, Shade, Text};
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::list::{ThumbAspect, ThumbSize};
    use craft_ui_widgets::primitives::IconSpec;
    use craft_ui_widgets::runtime::overlay::Backdrop;
    use craft_ui_widgets::style::{TypeRole, Variant};

    /// The speech bubble of the Discord buttons.
    #[derive(craft_ui::contract::MemoProps)]
    struct Chat;

    impl craft_ui_widgets::primitives::Glyph for Chat {
        fn paint(
            &self,
            painter: &egui::Painter,
            _tokens: &craft_ui_widgets::style::UiTokens,
            rect: egui::Rect,
            color: egui::Color32,
            _mirrored: bool,
        ) {
            paint_chat_icon(painter, rect.center(), rect.width().min(rect.height()), color);
        }
    }

    /// The Join our Discord button: the accent button with the speech bubble before its label; whether it was clicked.
    pub fn discord_button(ui: &mut egui::Ui, label: String, salt: &str) -> bool {
        let face = Chat;
        Button::new(label)
            .id_salt(salt.to_owned())
            .variant(Variant::PRIMARY)
            .icon(IconSpec::glyph(&face))
            .tooltip(links::DISCORD.to_owned())
            .show(ui)
            .clicked()
    }

    pub(super) fn show(app: &mut DesignApp, ctx: &egui::Context) {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
        let mut opened = true;
        let mut open: Option<&'static str> = None;
        let mut urls: Vec<String> = Vec::new();
        let tab = app.ui.about_tab;
        let mut next_tab = tab;
        let shown = egui::Area::new(egui::Id::unique("about_framework_host")).order(egui::Order::Foreground).show(ctx, |ui| {
            FrameworkDialog::new(tr("About DesignCraft"))
                .id_salt("about_dialog")
                .opened(Bind::bound(&mut opened))
                .backdrop(Backdrop::None)
                .closable(false)
                .outer_gesture(false)
                .close_on_enter(false)
                .least_width(if tab == 0 { LeastWidth::Dialog } else { LeastWidth::WideDialog })
                .buttons(vec![FooterButton::new(tr("  Close  ").trim().to_owned(), DialogRole::Cancel)])
                .show(ui, |ui, _| {
                    Stack::new().id_salt("about").fill_width().show(ui, |ui, _| {
                        let entries: Vec<Entry<String>> = ABOUT_TABS.iter().enumerate().map(|(i, l)| Entry::new(i.to_string(), tr(l))).collect();
                        let out = Segmented::new(tr("About DesignCraft"), Bind::given(tab.to_string()), &entries).id_salt("about_tabs").show(ui);
                        if let Some(picked) = out.events.committed().and_then(|v| v.parse::<u8>().ok()) {
                            next_tab = picked;
                        }
                        match tab {
                            1 => urls.extend(crate::credits::contributors_framework(ui, &lang)),
                            2 => crate::credits::models_framework(ui, &lang),
                            _ => about_page(ui, &lang, &mut open, &mut urls),
                        }
                    });
                })
                .output
        });
        app.ui.about_tab = next_tab;
        app.ui.pending_urls.extend(urls);
        if !opened || shown.inner.closed.is_some() || shown.inner.picked.is_some() {
            app.ui.about = false;
        }
        if let Some(k) = open {
            let _ = app.run(&format!("help.{k}"), json!({}));
        }
    }

    fn about_page(ui: &mut egui::Ui, lang: &str, open: &mut Option<&'static str>, urls: &mut Vec<String>) {
        let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
        Picture::painter("", ThumbAspect::SQUARE, move |cx, rect| paint_mark_in(cx.painter.ctx(), &cx.painter, rect, BRAND))
            .id_salt("about_mark")
            .extent(PictureSize::Thumb(ThumbSize::Large))
            .show(ui);
        // The channel's own name and version: DesignCraft (stable) or DesignCraft Nightly.
        let channel = crate::channel::current();
        let app_name = match channel {
            crate::channel::Channel::Stable => tr("DesignCraft"),
            crate::channel::Channel::NightlyV1 => channel.name().to_string(),
        };
        Text::new(app_name).role(TypeRole::Title).sizing(Sizing::FillWidth).show(ui);
        Text::new(format!("{} {}", tr("Version"), crate::channel::version())).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
        // A nightly build says which commit it was made from; the links below include Report an issue.
        if let Some(commit) = crate::channel::commit() {
            let short = commit.get(..12).unwrap_or(commit);
            Text::new(format!("nightly-v1 · {short}")).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
        }
        Text::new(tr("Page layout for print and screen — fast, open and scriptable.")).sizing(Sizing::FillWidth).show(ui);
        Text::new(tr("Part of the ArtCraft suite of open creative tools.")).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
        if discord_button(ui, tr("Join the ArtCraft Discord"), "about_discord") {
            *open = Some("discord");
        }
        Text::new(tr("discord.gg/artcraft — help, feedback and show-and-tell"))
            .role(TypeRole::Caption)
            .shade(Shade::Muted)
            .sizing(Sizing::FillWidth)
            .show(ui);
        Row::new().id_salt("about_links").wrap(true).show(ui, |ui, _| {
            for (text, key) in [("DesignCraft page", "appPage"), ("GitHub", "github"), ("getartcraft.com", "website"), ("Report an issue", "issues")]
            {
                if Link::new(tr(text)).url(links::get(key).unwrap_or_default().to_string()).id_salt(key).show(ui).clicked() {
                    *open = Some(key);
                }
            }
        });
        Divider::horizontal().show(ui);
        Text::new(tr("More ArtCraft apps")).role(TypeRole::BodyStrong).sizing(Sizing::FillWidth).show(ui);
        Row::new().id_salt("about_siblings").wrap(true).show(ui, |ui, _| {
            for (slug, name, what) in SIBLINGS {
                let url = format!("{}/apps/{slug}", links::WEBSITE);
                if Button::new((*name).to_owned()).id_salt(("about_sibling", *slug)).tooltip(format!("{} — {url}", tr(what))).show(ui).clicked() {
                    urls.push(url);
                }
            }
        });
        Text::new(tr("Open source. No telemetry.")).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
    }
}

/// The Join our Discord button as a framework component (see [`framework::discord_button`]).
pub fn discord_framework(ui: &mut egui::Ui, label: String, salt: &str) -> bool {
    framework::discord_button(ui, label, salt)
}
