use super::{DesignApp, Tokens, json};

pub fn start_screen(app: &mut DesignApp, ui: &mut egui::Ui) {
    framework::start_screen(app, ui);
}

/// Recent files shown on the start screen (File › Open Recent has them all).
const START_SCREEN_RECENT: usize = 8;

/// How often a recent file is looked up on disk again, in ms: the start screen and the Open Recent
/// menu draw every frame and must not touch the file system each time.
pub(super) const RECENT_STAT_MS: f64 = 3000.0;

/// A recent file as last seen on disk.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RecentStat {
    pub exists: bool,
    pub modified: Option<std::time::SystemTime>,
    checked: f64,
}

/// Whether recent file `path` is there and when it last changed, looked up again at most every
/// [`RECENT_STAT_MS`].
pub(crate) fn recent_file_stat(ctx: &egui::Context, path: &str) -> RecentStat {
    recent_file_stat_at(ctx, path, crate::now_ms())
}

pub(super) fn recent_file_stat_at(ctx: &egui::Context, path: &str, now: f64) -> RecentStat {
    let key = egui::Id::unique(("recent_stat", path));
    if let Some(st) = ctx.data(|d| d.get_temp::<RecentStat>(key))
        && (0.0..RECENT_STAT_MS).contains(&(now - st.checked))
    {
        return st;
    }
    let meta = designcraft_engine::io::metadata(path).ok().filter(designcraft_engine::io::Metadata::is_file);
    let st = RecentStat { exists: meta.is_some(), modified: meta.and_then(|m| m.modified().ok()), checked: now };
    ctx.data_mut(|d| d.insert_temp(key, st.clone()));
    st
}

/// The texture of a recent file's saved first-page picture, decoded once per file version.
fn recent_preview_texture(ctx: &egui::Context, path: &str, modified: Option<std::time::SystemTime>) -> Option<egui::TextureHandle> {
    type Cached = (Option<std::time::SystemTime>, Option<egui::TextureHandle>);
    let key = egui::Id::unique(("recent_preview", path));
    if let Some((m, tex)) = ctx.data(|d| d.get_temp::<Cached>(key))
        && m == modified
    {
        return tex;
    }
    let tex = designcraft_engine::cmd::recent_preview(path).and_then(|p| {
        let size = [usize::try_from(p.width).ok()?, usize::try_from(p.height).ok()?];
        (size[0].checked_mul(size[1])?.checked_mul(4)? == p.rgba.len()).then(|| {
            ctx.load_texture(format!("recent_preview:{path}"), egui::ColorImage::from_rgba_unmultiplied(size, &p.rgba), egui::TextureOptions::LINEAR)
        })
    });
    ctx.data_mut(|d| d.insert_temp::<Cached>(key, (modified, tex.clone())));
    tex
}

/// The start screen on the framework's components: the welcome text and the three buttons, the community card, the recent
/// files and the presets as cards, with the same commands.
mod framework {
    use super::{DesignApp, START_SCREEN_RECENT, Tokens, json, recent_file_stat, recent_preview_texture};
    use craft_ui_panels::sections::GroupBox;
    use craft_ui_widgets::buttons::{Arrangement, Button, Link};
    use craft_ui_widgets::contract::{Props as _, Scroll, Show as _, ShowContainer as _, Sizing};
    use craft_ui_widgets::display::{Picture, PictureSize, Shade, Text};
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::list::{ThumbAspect, ThumbSize};
    use craft_ui_widgets::style::{TypeRole, Variant};

    /// What a click on the screen asks for.
    enum Ask {
        Run(&'static str, serde_json::Value),
        Recent(usize),
        Preset(&'static str),
    }

    /// A button label from a key that the catalogue spells with padding spaces.
    fn padded(lang: &str, key: &str) -> String {
        crate::i18n::tr(lang, key).trim().to_owned()
    }

    pub(super) fn start_screen(app: &mut DesignApp, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        let r = ui.available_rect_before_wrap();
        ui.painter().rect_filled(r, 0.0, t.panel_darker);
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
        let rows: Vec<(usize, String)> = app.session.prefs.recent_files.iter().take(START_SCREEN_RECENT).cloned().enumerate().collect();
        let mut asks: Vec<Ask> = Vec::new();
        let ctx = ui.ctx().clone();
        let room = (r.height() - 96.0).max(120.0);
        ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink(48.0)), |ui| {
            Stack::new().id_salt("start_screen").fill_width().scroll(Scroll::vertical()).room_height(room).show(ui, |ui, _| {
                Text::new(tr("Welcome to DesignCraft")).role(TypeRole::Title).sizing(Sizing::FillWidth).show(ui);
                Text::new(tr("Page layout for print and screen — fast, open and scriptable.")).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
                Row::new().id_salt("start_actions").wrap(true).show(ui, |ui, _| {
                    if Button::new(padded(&lang, "  New file  ")).id_salt("start_new").variant(Variant::PRIMARY).show(ui).clicked() {
                        asks.push(Ask::Run("app.newDocumentDialog", json!({})));
                    }
                    if Button::new(padded(&lang, "  Open  ")).id_salt("start_open").show(ui).clicked() {
                        asks.push(Ask::Run("app.openDialog", json!({})));
                    }
                    if Button::new(padded(&lang, "  Open sample magazine  ")).id_salt("start_sample").show(ui).clicked() {
                        asks.push(Ask::Run("file.newSample", json!({})));
                    }
                });
                GroupBox::new(tr("Join the ArtCraft community"), None).id_salt("start_community").show(ui, |ui, _| {
                    Row::new().id_salt("start_community_row").fill_width().show(ui, |ui, _| {
                        Picture::painter("", ThumbAspect::SQUARE, move |cx, rect| {
                            crate::about::paint_mark_in(cx.painter.ctx(), &cx.painter, rect, crate::about::BRAND);
                        })
                        .id_salt("start_mark")
                        .extent(PictureSize::Thumb(ThumbSize::Large))
                        .show(ui);
                        Stack::new().id_salt("start_community_text").fill_width().show(ui, |ui, _| {
                            Text::new(tr("Get help, share your layouts and shape what we build next."))
                                .shade(Shade::Muted)
                                .sizing(Sizing::FillWidth)
                                .show(ui);
                            Row::new().id_salt("start_links").wrap(true).show(ui, |ui, _| {
                                for (text, id) in
                                    [("DesignCraft page", "help.appPage"), ("GitHub", "help.github"), ("getartcraft.com", "help.website")]
                                {
                                    let url = designcraft_engine::links::get(&id[5..]).unwrap_or_default();
                                    if Link::new(text).url(url.to_string()).id_salt(id).show(ui).clicked() {
                                        asks.push(Ask::Run(id, json!({})));
                                    }
                                }
                            });
                        });
                        if crate::about::discord_framework(ui, tr("Join our Discord"), "start_discord") {
                            asks.push(Ask::Run("help.discord", json!({})));
                        }
                    });
                });
                if !rows.is_empty() {
                    Text::new(tr("Recent files")).role(TypeRole::BodyStrong).sizing(Sizing::FillWidth).show(ui);
                    Row::new().id_salt("start_recent").wrap(true).show(ui, |ui, _| {
                        for (i, path) in &rows {
                            let stat = recent_file_stat(&ctx, path);
                            let modified = stat.exists.then_some(stat.modified).flatten();
                            let tex = modified.and_then(|m| recent_preview_texture(&ctx, path, Some(m)));
                            let there = stat.exists;
                            // The name without `.designcraft`.
                            let label = crate::menus::recent_label(path);
                            let name = label.strip_suffix(".designcraft").unwrap_or(&label).to_string();
                            let (border, blank) = (t.border, if there { egui::Color32::from_gray(245) } else { egui::Color32::from_gray(110) });
                            let card = Button::new(name)
                                .id_salt(("start_recent", *i))
                                .tooltip(path.clone())
                                .arrangement(Arrangement::Stacked)
                                .picture(
                                    ThumbAspect::new(4, 5),
                                    Picture::painter("", ThumbAspect::new(4, 5), move |cx, rect| match &tex {
                                        Some(tex) => {
                                            let size = tex.size_vec2();
                                            let k = (rect.width() / size.x).min(rect.height() / size.y);
                                            let pr = egui::Rect::from_center_size(rect.center(), size * k);
                                            cx.painter.image(
                                                tex.id(),
                                                pr,
                                                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                                egui::Color32::WHITE,
                                            );
                                            cx.painter.rect_stroke(pr, 0.0, egui::Stroke::new(1.0, border), egui::StrokeKind::Outside);
                                        }
                                        // No picture (saved before files had one) or the file is gone: a blank page.
                                        None => {
                                            cx.painter.rect_filled(egui::Rect::from_center_size(rect.center(), rect.size() * 0.8), 0.0, blank);
                                        }
                                    })
                                    .extent(PictureSize::Thumb(ThumbSize::Large)),
                                )
                                .show(ui);
                            if card.clicked() {
                                asks.push(Ask::Recent(*i));
                            }
                        }
                    });
                }
                Text::new(tr("Start a new document")).role(TypeRole::BodyStrong).sizing(Sizing::FillWidth).show(ui);
                Row::new().id_salt("start_presets").wrap(true).show(ui, |ui, _| {
                    for p in designcraft_doc::build::PRESETS.iter().filter(|p| p.intent == designcraft_doc::Intent::Print) {
                        let aspect = ThumbAspect::new(p.width.round().clamp(1.0, 60000.0) as u16, p.height.round().clamp(1.0, 60000.0) as u16);
                        let size = format!(
                            "{} × {}",
                            designcraft_geom::format_measure(p.width, p.units),
                            designcraft_geom::format_measure(p.height, p.units)
                        );
                        let card = Button::new(p.name.to_owned())
                            .id_salt(("start_preset", p.name))
                            .tooltip(size)
                            .arrangement(Arrangement::Stacked)
                            .picture(
                                aspect,
                                Picture::painter("", aspect, move |cx, rect| {
                                    cx.painter.rect_filled(rect, 0.0, egui::Color32::from_gray(245));
                                })
                                .extent(PictureSize::Thumb(ThumbSize::Large)),
                            )
                            .show(ui);
                        if card.clicked() {
                            asks.push(Ask::Preset(p.name));
                        }
                    }
                });
            });
        });
        for ask in asks {
            match ask {
                Ask::Run(id, params) => {
                    let _ = app.run(id, params);
                }
                Ask::Recent(i) => {
                    let _ = app.run("app.openRecent", json!({"index": i}));
                }
                Ask::Preset(name) => {
                    let _ = app.run("file.new", json!({"preset": name}));
                }
            }
        }
    }
}
