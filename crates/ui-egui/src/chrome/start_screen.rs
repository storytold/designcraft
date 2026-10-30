use std::sync::Arc;

use super::{Color32, Content, DesignApp, Sense, Stroke, Tokens, json, semibold, vec2};

pub fn start_screen(app: &mut DesignApp, ui: &mut egui::Ui) {
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
            let (cr, resp) = ui.allocate_exact_size(vec2(140.0, 170.0), Sense::click());
            let resp = resp.on_hover_text(&path);
            ui.painter().rect_filled(cr, 6.0, if resp.hovered() { t.hover } else { t.panel });
            let area = egui::Rect::from_min_size(cr.min + vec2(20.0, 12.0), vec2(100.0, 108.0));
            match recent_preview_texture(ui.ctx(), &path, &stat) {
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

/// How often a recent file is read again, in ms: the start screen and the Open Recent menu draw
/// every frame and must not touch the file system each time.
pub(super) const RECENT_STAT_MS: f64 = 3000.0;

/// A recent file as last read: whether it is there, when it last changed, and the first page's
/// picture saved in it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RecentStat {
    pub exists: bool,
    pub modified: Option<std::time::SystemTime>,
    /// When this was read (ms).
    checked: f64,
    /// The picture as straight RGBA, before it becomes a texture (see [`recent_preview_texture`]).
    image: Option<Arc<egui::ColorImage>>,
}

impl RecentStat {
    /// Nothing read yet: not known to be gone (so the card isn't dimmed before the answer), and
    /// old enough that the first look asks for a read.
    fn unread() -> RecentStat {
        // Checked long ago, not at 0: the clock counts from this process's first look, so 0.0
        // would read as "just checked" for the first seconds and leave the cards unread.
        RecentStat { exists: true, modified: None, checked: f64::NEG_INFINITY, image: None }
    }
}

/// Whether recent file `path` is there and when it last changed: read at most every
/// [`RECENT_STAT_MS`] and never on the interface thread (see [`reader`]). A file on a slow disk, or
/// a drive that has to spin up, must not hold the window black.
pub(crate) fn recent_file_stat(ctx: &egui::Context, path: &str) -> RecentStat {
    recent_file_stat_at(ctx, path, crate::now_ms())
}

pub(super) fn recent_file_stat_at(ctx: &egui::Context, path: &str, now: f64) -> RecentStat {
    poll_reads(ctx);
    let key = egui::Id::unique(("recent_stat", path));
    let mut st = ctx.data(|d| d.get_temp::<RecentStat>(key)).unwrap_or_else(RecentStat::unread);
    if (0.0..RECENT_STAT_MS).contains(&(now - st.checked)) {
        return st;
    }
    // Asked for now, so however many frames draw meanwhile only one read is queued.
    st.checked = now;
    ctx.data_mut(|d| d.insert_temp(key, st.clone()));
    #[cfg(not(target_arch = "wasm32"))]
    if !cfg!(test) {
        reader::submit(ctx, path.to_owned());
        return st;
    }
    // The web has no threads, and a test reads the file it made itself.
    let st = read_recent_at(path, now);
    ctx.data_mut(|d| d.insert_temp(key, st.clone()));
    st
}

/// `path` as it is on disk now: what [`reader`]'s thread reads.
fn read_recent_at(path: &str, now: f64) -> RecentStat {
    let meta = designcraft_engine::io::metadata(path).ok().filter(designcraft_engine::io::Metadata::is_file);
    let modified = meta.as_ref().and_then(|m| m.modified().ok());
    let image = modified.and_then(|_| recent_picture(path));
    RecentStat { exists: meta.is_some(), modified, checked: now, image }
}

/// The first page's picture saved in the file at `path`, if it has one.
fn recent_picture(path: &str) -> Option<Arc<egui::ColorImage>> {
    let p = designcraft_engine::cmd::recent_preview(path)?;
    let size = [usize::try_from(p.width).ok()?, usize::try_from(p.height).ok()?];
    (size[0].checked_mul(size[1])?.checked_mul(4)? == p.rgba.len()).then(|| Arc::new(egui::ColorImage::from_rgba_unmultiplied(size, &p.rgba)))
}

/// Take in the answers [`reader`]'s thread has finished.
fn poll_reads(ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    reader::poll(ctx);
    #[cfg(target_arch = "wasm32")]
    let _ = ctx;
}

/// The texture of a recent file's saved first-page picture, uploaded once per file version.
fn recent_preview_texture(ctx: &egui::Context, path: &str, stat: &RecentStat) -> Option<egui::TextureHandle> {
    let image = stat.image.as_ref()?;
    type Cached = (Option<std::time::SystemTime>, egui::TextureHandle);
    let key = egui::Id::unique(("recent_preview", path));
    if let Some((m, tex)) = ctx.data(|d| d.get_temp::<Cached>(key))
        && m == stat.modified
    {
        return Some(tex);
    }
    let tex = ctx.load_texture(format!("recent_preview:{path}"), (**image).clone(), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(key, (stat.modified, tex.clone())));
    Some(tex)
}

/// Background reads of the recent files: one thread, so the first frame draws before their
/// pictures arrive and a file on a slow disk never stalls the interface.
#[cfg(not(target_arch = "wasm32"))]
mod reader {
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Mutex, OnceLock};

    use super::{RecentStat, read_recent_at};

    struct Reader {
        jobs: Sender<String>,
        done: Mutex<Receiver<(String, RecentStat)>>,
    }

    static READER: OnceLock<Option<Reader>> = OnceLock::new();

    fn reader(ctx: &egui::Context) -> Option<&'static Reader> {
        READER
            .get_or_init(|| {
                let (jobs, job_rx) = std::sync::mpsc::channel::<String>();
                let (result_tx, result_rx) = std::sync::mpsc::channel::<(String, RecentStat)>();
                let ctx = ctx.clone();
                let spawned = std::thread::Builder::new().name("recent-files".into()).spawn(move || {
                    while let Ok(path) = job_rx.recv() {
                        let stat = read_recent_at(&path, crate::now_ms());
                        if result_tx.send((path, stat)).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                });
                if spawned.is_err() {
                    return None;
                }
                Some(Reader { jobs, done: Mutex::new(result_rx) })
            })
            .as_ref()
    }

    /// Queue a read of `path`.
    pub(super) fn submit(ctx: &egui::Context, path: String) {
        if let Some(r) = reader(ctx) {
            let _ = r.jobs.send(path);
        }
    }

    /// Take finished reads into the window's state, on the interface thread.
    pub(super) fn poll(ctx: &egui::Context) {
        if READER.get().is_none() {
            return;
        }
        let Some(r) = reader(ctx) else { return };
        let Ok(done) = r.done.lock() else { return };
        while let Ok((path, stat)) = done.try_recv() {
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("recent_stat", path.as_str())), stat));
        }
    }
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
