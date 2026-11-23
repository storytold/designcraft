use super::{Color32, Content, DesignApp, Sense, Stroke, Tokens, json, semibold, vec2};

pub fn start_screen(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::start_screen(app, ui);
        return;
    }
    let t = Tokens::get(ui.ctx());
    let r = ui.available_rect_before_wrap();
    ui.painter().rect_filled(r, 0.0, t.panel_darker);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(r.shrink(48.0)).layout(egui::Layout::top_down(if crate::i18n::is_rtl(&app.ui.language) {
            egui::Align::Max
        } else {
            egui::Align::Min
        })),
        |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Welcome to DesignCraft")).font(semibold(28.0)).color(t.text_strong),
                ));
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page layout for print and screen — fast, open and scriptable."))
                        .size(15.0)
                        .color(t.text_dim),
                ));
                ui.add_space(24.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(crate::rtl::widget(
                                ui,
                                egui::RichText::new(crate::i18n::tr(&app.ui.language, "  New file  ")).size(14.0).color(Color32::WHITE),
                            ))
                            .fill(t.accent_strong)
                            .corner_radius(16.0)
                            .min_size(vec2(0.0, 32.0)),
                        )
                        .clicked()
                    {
                        let _ = app.run("app.newDocumentDialog", json!({}));
                    }
                    if ui
                        .add(
                            egui::Button::new(crate::rtl::widget(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "  Open  ")).size(14.0)))
                                .corner_radius(16.0)
                                .min_size(vec2(0.0, 32.0)),
                        )
                        .clicked()
                    {
                        let _ = app.run("app.openDialog", json!({}));
                    }
                    if ui
                        .add(
                            egui::Button::new(crate::rtl::widget(
                                ui,
                                egui::RichText::new(crate::i18n::tr(&app.ui.language, "  Open sample magazine  ")).size(14.0),
                            ))
                            .corner_radius(16.0)
                            .min_size(vec2(0.0, 32.0)),
                        )
                        .clicked()
                    {
                        let _ = app.run("file.newSample", json!({}));
                    }
                });
                ui.add_space(20.0);
                community_card(app, ui);
                recent_files(app, ui);
                ui.add_space(24.0);
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Start a new document")).font(semibold(15.0)).color(t.text_strong),
                ));
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    for p in designcraft_doc::build::PRESETS.iter().filter(|p| p.intent == designcraft_doc::Intent::Print) {
                        let (cr, resp) = ui.allocate_exact_size(vec2(120.0, 150.0), Sense::click());
                        let hov = resp.hovered();
                        ui.painter().rect_filled(cr, 6.0, if hov { t.hover } else { t.panel });
                        let k = (80.0 / p.width.max(p.height)) as f32;
                        let pr = egui::Rect::from_center_size(cr.center() - vec2(0.0, 14.0), vec2(p.width as f32 * k, p.height as f32 * k));
                        ui.painter().rect_filled(pr, 0.0, Color32::from_gray(245));
                        ui.painter().text(
                            egui::pos2(cr.center().x, cr.max.y - 26.0),
                            egui::Align2::CENTER_CENTER,
                            p.name,
                            semibold(12.0),
                            t.text_strong,
                        );
                        ui.painter().text(
                            egui::pos2(cr.center().x, cr.max.y - 11.0),
                            egui::Align2::CENTER_CENTER,
                            format!(
                                "{} × {}",
                                designcraft_geom::format_measure(p.width, p.units),
                                designcraft_geom::format_measure(p.height, p.units)
                            ),
                            egui::FontId::proportional(10.0),
                            t.text_dim,
                        );
                        if resp.clicked() {
                            let _ = app.run("file.new", json!({"preset": p.name}));
                        }
                    }
                });
            });
        },
    );
    let _ = Content::Unassigned;
}

/// Recent files shown on the start screen (File › Open Recent has them all).
const START_SCREEN_RECENT: usize = 8;

/// Start screen: the most recent files as cards with the first page's picture (saved in the file)
/// and the name; the full path is the tooltip. A file that is gone is dimmed and says so when
/// clicked. Nothing when the list is empty.
fn recent_files(app: &mut DesignApp, ui: &mut egui::Ui) {
    if app.session.prefs.recent_files.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    ui.add_space(24.0);
    ui.label(crate::rtl::widget(
        ui,
        egui::RichText::new(crate::i18n::tr(&app.ui.language, "Recent files")).font(semibold(15.0)).color(t.text_strong),
    ));
    ui.add_space(8.0);
    let rows: Vec<(usize, String)> = app.session.prefs.recent_files.iter().take(START_SCREEN_RECENT).cloned().enumerate().collect();
    let mut open = None;
    ui.horizontal_wrapped(|ui| {
        for (i, path) in rows {
            let stat = recent_file_stat(ui.ctx(), &path);
            let there = stat.exists;
            let modified = there.then_some(stat.modified);
            let (cr, resp) = ui.allocate_exact_size(vec2(140.0, 170.0), Sense::click());
            let resp = resp.on_hover_text(&path);
            ui.painter().rect_filled(cr, 6.0, if resp.hovered() { t.hover } else { t.panel });
            let area = egui::Rect::from_min_size(cr.min + vec2(20.0, 12.0), vec2(100.0, 108.0));
            match modified.and_then(|m| recent_preview_texture(ui.ctx(), &path, m)) {
                Some(tex) => {
                    let size = tex.size_vec2();
                    let k = (area.width() / size.x).min(area.height() / size.y);
                    let pr = egui::Rect::from_center_size(area.center(), size * k);
                    ui.painter().image(tex.id(), pr, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    ui.painter().rect_stroke(pr, 0.0, Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
                }
                // No picture (saved before files had one) or the file is gone: a blank page.
                None => {
                    let pr = egui::Rect::from_center_size(area.center(), vec2(62.0, 80.0));
                    ui.painter().rect_filled(pr, 0.0, if there { Color32::from_gray(245) } else { Color32::from_gray(110) });
                }
            }
            let color = if there { t.text_strong } else { t.text_dim };
            // The name without `.designcraft`, on up to two lines (broken after a space or dash).
            let label = crate::menus::recent_label(&path);
            let name = label.strip_suffix(".designcraft").unwrap_or(&label).to_string();
            let mut job = egui::text::LayoutJob::simple(name, semibold(12.0), color, cr.width() - 12.0);
            job.wrap.max_rows = 2;
            job.halign = egui::Align::Center;
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(egui::pos2(cr.center().x, area.max.y + 8.0), galley, color);
            if resp.clicked() {
                open = Some(i);
            }
        }
    });
    if let Some(i) = open {
        let _ = app.run("app.openRecent", json!({"index": i}));
    }
}

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

/// Start screen: the ArtCraft community (Discord first) and project links.
fn community_card(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Frame::NONE.fill(t.panel).corner_radius(10.0).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
        // Wide enough for the longer translations of the line under the title, with the button
        // at the right edge; the text wraps before the button's space rather than run under it.
        const BUTTON: egui::Vec2 = vec2(190.0, 38.0);
        ui.set_max_width(760.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
            crate::about::paint_mark(ui, r, crate::about::BRAND);
            ui.add_space(8.0);
            let label = crate::i18n::tr(&app.ui.language, "Join our Discord");
            let button = crate::about::discord_button_size(ui.ctx(), label, BUTTON);
            let text_width = (ui.available_width() - button.x - 16.0).max(120.0);
            ui.vertical(|ui| {
                ui.set_max_width(text_width);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Join the ArtCraft community")).font(semibold(14.0)).color(t.text_strong),
                ));
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Get help, share your layouts and shape what we build next."))
                        .size(12.0)
                        .color(t.text_dim),
                ));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (text, id) in [("DesignCraft page", "help.appPage"), ("GitHub", "help.github"), ("getartcraft.com", "help.website")] {
                        let url = designcraft_engine::links::get(&id[5..]).unwrap_or_default();
                        let l = ui
                            .add(egui::Label::new(egui::RichText::new(text).size(12.0).color(t.accent).underline()).sense(Sense::click()))
                            .on_hover_text(url)
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if l.clicked() {
                            let _ = app.run(id, json!({}));
                        }
                        ui.add_space(8.0);
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::about::discord_button(ui, label, BUTTON) {
                    let _ = app.run("help.discord", json!({}));
                }
            });
        });
    });
}

/// The start screen on the framework's components: the welcome text and the three buttons, the community card, the recent
/// files and the presets as cards, with the same commands.
#[cfg(not(target_arch = "wasm32"))]
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
                                Picture::painter("", aspect, move |cx, rect| cx.painter.rect_filled(rect, 0.0, egui::Color32::from_gray(245)))
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
