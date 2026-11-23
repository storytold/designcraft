use super::{DesignApp, FIELD_H, Sense, Tokens, Value, block, divider, json, sel_info, sub, units, vec2};
use crate::app::file_dialogs::Bound;

/// Only Links actions opt into the already-installed native runtime. A context without it (and
/// the web build) must keep working buttons rather than paint an inert SDK component.
struct LinkButton {
    response: egui::Response,
    clicked: bool,
}

/// A Links action: `(kind, id, action)` names its target (an asset, a story, the document) and what it does.
type LinkAction<'a> = (&'a str, u64, &'a str);

/// A Links action button labelled `label` (translated here), tipped `tip` (translated by the caller) when given.
fn link_button(app: &DesignApp, ui: &mut egui::Ui, key: LinkAction<'_>, label: &str, tip: Option<&str>, enabled: bool, small: bool) -> LinkButton {
    let (kind, target, action) = key;
    // Keep document and actual asset/story identity explicit, independent of translated labels.
    let uid = app.session.active().map(|st| st.uid);
    let id = ui.make_persistent_id(("links_action", uid, (kind, target), action));
    let label = crate::i18n::tr(&app.ui.language, label);
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::contract::{Props as _, Show as _};
        let mut button = craft_ui_widgets::buttons::Button::new(label.to_owned()).id(id).enabled(enabled);
        if let Some(tip) = tip {
            button = button.tooltip(tip.to_owned());
        }
        let out = button.show(ui);
        // SDK activation is authoritative: egui's extra click rule need not match it.
        let clicked = out.clicked();
        return LinkButton { response: out.response, clicked };
    }
    let response = ui
        .push_id(id, |ui| {
            let button = egui::Button::new(crate::rtl::widget(ui, label));
            ui.add_enabled(enabled, if small { button.small() } else { button })
        })
        .inner;
    let response = match tip {
        Some(tip) => response.on_hover_ui(|ui| {
            crate::rtl::label(ui, tip);
        }),
        None => response,
    };
    let clicked = response.clicked();
    LinkButton { response, clicked }
}

/// Links: status (missing / modified / embedded), page and effective resolution of every placed
/// graphic and linked text file; Relink, Go To Link, Update Link, Embed Link for the chosen row.
pub fn links_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    // The installed runtime owns Escape and exposes it as an untaken window Cancel, so
    // egui's raw key_pressed no longer sees it. Give only our existing confirmation that
    // real event; never invent a key, bypass a popup, or confirm/replace the story here.
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed && app.ui.dialog.as_ref().is_some_and(|d| d.id == "updateLink") && !egui::Popup::is_any_open(ui.ctx()) {
        use craft_ui_widgets::runtime::keys::{Intent, with_window_intents};
        if with_window_intents(ui.ctx(), crate::i18n::is_rtl(&app.ui.language), |keys| keys.take(Intent::Cancel)) {
            crate::dialogs::cancel(app);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::links_body(app, ui);
    }
    // Linked stories (Place and Link).
    let stories = app.session.execute("story.links", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    if !stories.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Linked Stories")).strong());
        for l in &stories {
            let out = l["outOfDate"].as_bool().unwrap_or(false);
            ui.horizontal(|ui| {
                crate::rtl::label(
                    ui,
                    crate::i18n::tr(&app.ui.language, "Story {story} ← story {parent}")
                        .replace("{story}", &l["story"].to_string())
                        .replace("{parent}", &l["parent"].to_string()),
                );
                if out {
                    crate::rtl::label(
                        ui,
                        crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, "⚠ out of date")).color(egui::Color32::from_rgb(230, 160, 40)),
                        ),
                    );
                    if link_button(app, ui, ("linked_story", l["story"].as_u64().unwrap_or(0), "update"), "Update", None, true, true).clicked {
                        let _ = app.run("story.updateLink", json!({"story": l["story"]}));
                    }
                }
            });
        }
        ui.separator();
    }
    let t = Tokens::get(ui.ctx());
    let Some((rows, unavailable)) = link_rows(app, ui) else {
        return;
    };
    if unavailable && !rows.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Link status is unavailable.")).color(t.text_dim).size(11.0));
    }
    if rows.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No placed graphics.")).color(t.text_dim));
        return;
    }
    let sel_key = egui::Id::unique("links_panel_sel");
    let mut chosen: Option<u64> = ui.data(|d| d.get_temp(sel_key));
    // Leave room for Status and Page/PPI instead of widening the grid for long source paths.
    let name_width = (ui.available_width() - 160.0).max(50.0);
    egui::Grid::new("links_grid").num_columns(3).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Name")).color(t.text_dim).size(11.0));
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Status")).color(t.text_dim).size(11.0));
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page · PPI")).color(t.text_dim).size(11.0));
        ui.end_row();
        for r in rows.iter() {
            // Asset and story ids come from one counter: a row is chosen by either.
            let aid = r["asset"].as_u64().or_else(|| r["story"].as_u64()).unwrap_or(0);
            let name = r["name"].as_str().unwrap_or("");
            let storage = r["storage"].as_str().unwrap_or("");
            let thumb = if r["story"].is_null() { app.session.active().and_then(|st| proxy_thumb(ui.ctx(), st.uid, &st.doc, aid)) } else { None };
            let response = ui
                .horizontal(|ui| {
                    if let Some(tex) = &thumb {
                        ui.add(egui::Image::new(tex).fit_to_exact_size(vec2(THUMB, THUMB)));
                    }
                    ui.vertical(|ui| {
                        ui.set_width(name_width - if thumb.is_some() { THUMB + 6.0 } else { 0.0 });
                        let name = ui.add(egui::Button::selectable(chosen == Some(aid), name).truncate());
                        match r["path"].as_str().filter(|path| !path.is_empty()) {
                            Some(path) => name.union(
                                ui.add(
                                    egui::Label::new(egui::RichText::new(path).color(t.text_dim).size(11.0))
                                        .truncate()
                                        .sense(Sense::click())
                                        .show_tooltip_when_elided(false),
                                )
                                .on_hover_text(path),
                            ),
                            None => name,
                        }
                    })
                    .inner
                })
                .inner;
            if response.clicked() {
                chosen = Some(aid);
            }
            let (txt, tone) = status_look(r["status"].as_str().unwrap_or(""));
            let col = match tone {
                StatusTone::Bad => egui::Color32::from_rgb(0xe5, 0x4b, 0x4b),
                StatusTone::Warn => egui::Color32::from_rgb(0xe8, 0xb3, 0x2c),
                StatusTone::Dim => t.text_dim,
                StatusTone::Good => egui::Color32::from_rgb(60, 200, 90),
            };
            let status = ui
                .horizontal(|ui| {
                    if !storage.is_empty() {
                        storage_icon(ui, storage == "linked", t.text_dim, &app.ui.language);
                    }
                    badge(ui, crate::i18n::tr(&app.ui.language, txt), col)
                })
                .inner;
            // Why a link isn't followed (a path on another computer, a device).
            if let Some(why) = r["refused"].as_str() {
                status.on_hover_text(why);
            }
            let (info, low) = use_info(&app.ui.language, r);
            crate::rtl::label(
                ui,
                egui::RichText::new(info).size(11.0).color(if low { egui::Color32::from_rgb(0xe8, 0xb3, 0x2c) } else { t.text_dim }),
            );
            ui.end_row();
        }
    });
    if let Some(c) = chosen {
        ui.data_mut(|d| d.insert_temp(sel_key, c));
    }
    ui.add_space(6.0);
    if let Some(path) = rows
        .iter()
        .find(|r| r["asset"].as_u64().or_else(|| r["story"].as_u64()) == chosen)
        .and_then(|r| r["path"].as_str())
        .filter(|path| !path.is_empty())
    {
        // A selected link stays distinguishable even if two rows truncate to the same prefix.
        ui.add(egui::Label::new(egui::RichText::new(path).color(t.text_dim).size(11.0)).wrap());
        ui.add_space(6.0);
    }
    link_actions(app, ui, &rows, chosen);
}

/// The placed graphics and linked text files the Links panel lists, and whether their statuses could not be checked;
/// `None` without a document.
fn link_rows(app: &mut DesignApp, ui: &egui::Ui) -> Option<(std::sync::Arc<Vec<Value>>, bool)> {
    let st = app.session.active()?;
    // Whether a linked file is still there and unchanged is asked of the file system, and a
    // network path may take an operating-system timeout to answer: native builds list the links
    // from the document and take their statuses from a check on a background worker.
    #[cfg(not(target_arch = "wasm32"))]
    let refresh = {
        let doc = std::sync::Arc::clone(&st.doc);
        let snapshot = crate::links_worker::Snapshot {
            key: crate::links_worker::Key::new(st.uid, &doc, st.path.as_deref()),
            doc: std::sync::Arc::clone(&doc),
            cache: std::sync::Arc::clone(&app.session.cache),
            document: st.path.as_ref().map(std::path::PathBuf::from),
        };
        let refresh = app.links_worker.rows(ui.ctx(), snapshot);
        // A file the check read and found unchanged isn't read again, in this session or the next.
        if !refresh.verified.is_empty() {
            designcraft_engine::cmd::remember_link_files(&mut app.session, &doc, &refresh.verified);
        }
        refresh
    };
    #[cfg(not(target_arch = "wasm32"))]
    let rows = std::sync::Arc::clone(&refresh.values);
    // Web builds have no filesystem to wait for: the list is refreshed at most twice a second.
    #[cfg(target_arch = "wasm32")]
    let rows = {
        let key = egui::Id::unique("links_panel");
        let now = ui.input(|i| i.time);
        let rev = (st.uid, st.revision);
        let cached: Option<(f64, (u64, u64), Vec<Value>)> = ui.data(|d| d.get_temp(key));
        match cached {
            Some((at, r, rows)) if r == rev && now - at < 0.5 => rows,
            _ => {
                let rows = app.session.execute("links.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
                ui.data_mut(|d| d.insert_temp(key, (now, rev, rows.clone())));
                rows
            }
        }
    };
    #[cfg(target_arch = "wasm32")]
    let rows = std::sync::Arc::new(rows);
    #[cfg(not(target_arch = "wasm32"))]
    let unavailable = refresh.unavailable;
    #[cfg(target_arch = "wasm32")]
    let unavailable = false;
    Some((rows, unavailable))
}

/// The Links actions under the list: Relink…, Go To, Update and Embed (or Unlink, Unembed…) for the chosen row, then
/// Relink to Folder….
fn link_actions(app: &mut DesignApp, ui: &mut egui::Ui, rows: &[Value], chosen: Option<u64>) {
    ui.horizontal_wrapped(|ui| {
        let Some(aid) = chosen else {
            note(app, ui, crate::i18n::tr(&app.ui.language, "Choose a link."));
            return;
        };
        // A linked text file is addressed by its story.
        let is_text = rows.iter().any(|r| r["story"].as_u64() == Some(aid));
        let kind = if is_text { "story" } else { "asset" };
        let target = if is_text { json!({"story": aid}) } else { json!({"asset": aid}) };
        let with = |k: &str, v: Value| {
            let mut p = target.clone();
            p[k] = v;
            p
        };
        if link_button(app, ui, (kind, aid, "relink"), "Relink…", None, true, false).clicked {
            let target = with("path", Value::Null);
            let asked = app.ask_open("relink", Bound::Document, move |app, path| {
                let _ = app.run("links.relink", crate::app::file_dialogs::with_path(&target, "path", path));
                Ok(Value::Null)
            });
            if let Err(e) = asked {
                app.status_error("Relink", e);
            }
        }
        if link_button(app, ui, (kind, aid, "go_to"), "Go To", None, true, false).clicked {
            let _ = app.run("links.goTo", target.clone());
        }
        let row = rows.iter().find(|r| r["asset"].as_u64() == Some(aid));
        let status = row.and_then(|r| r["status"].as_str()).unwrap_or("");
        let edited = rows.iter().any(|r| r["story"].as_u64() == Some(aid) && r["edited"].as_bool() == Some(true));
        let edited_tip = crate::i18n::tr(&app.ui.language, "This story was edited in the layout: you'll be asked before those edits are replaced");
        // A graphic is updated only when its file changed; a linked story can always be imported again.
        let update = link_button(app, ui, (kind, aid, "update"), "Update", edited.then_some(edited_tip), is_text || status == "modified", false);
        if update.clicked {
            update_links(app, target.clone());
        }
        if is_text {
            if link_button(app, ui, ("story", aid, "unlink"), "Unlink", None, true, false).clicked {
                let _ = app.run("links.embed", target.clone());
            }
        } else if status == "embedded" {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let tip = crate::i18n::tr(&app.ui.language, "Keep the graphic in a file and link to it");
                if link_button(app, ui, ("asset", aid, "unembed"), "Unembed\u{2026}", Some(tip), true, false).clicked {
                    app.unembed_link(aid);
                }
            }
        } else {
            let tip = crate::i18n::tr(&app.ui.language, "Copy the file into the document");
            let embed = link_button(app, ui, ("asset", aid, "embed"), "Embed", Some(tip), !matches!(status, "missing" | "modified"), false);
            if embed.clicked
                && let Err(e) = app.run("links.embed", target.clone())
            {
                app.status_error("Embed", e);
            }
        }
    });
    ui.horizontal(|ui| {
        // Choose any file in the folder: missing links are looked up there by name.
        let tip = crate::i18n::tr(&app.ui.language, "Pick a file in the folder holding the moved graphics");
        if link_button(app, ui, ("document", 0, "relink_folder"), "Relink to Folder…", Some(tip), true, false).clicked {
            let asked = app.ask_open("relink", Bound::Document, |app, path| {
                let Some(dir) = std::path::Path::new(&path).parent() else {
                    return Ok(Value::Null);
                };
                if let Ok(v) = app.run("links.relinkFolder", json!({"dir": dir.to_string_lossy()})) {
                    app.status(crate::i18n::tr(&app.ui.language, "Relinked {count}").replace("{count}", &v["relinked"].to_string()));
                }
                Ok(Value::Null)
            });
            if let Err(e) = asked {
                app.status_error("Relink", e);
            }
        }
    });
}

/// A small dim note ("Choose a link."): the framework's faint text once it is installed.
fn note(app: &DesignApp, ui: &mut egui::Ui, text: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::contract::{Props as _, Show as _};
        craft_ui_widgets::display::Text::new(text.to_owned()).shade(craft_ui_widgets::display::Shade::Faint).show(ui);
        return;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = app;
    let t = Tokens::get(ui.ctx());
    crate::rtl::label(ui, egui::RichText::new(text).color(t.text_dim).size(11.0));
}

/// How a link's status reads: its words and its tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusTone {
    Bad,
    Warn,
    Dim,
    Good,
}

/// The words (to translate) and tone of the link status `status`.
fn status_look(status: &str) -> (&'static str, StatusTone) {
    match status {
        "missing" => ("\u{26A0} Missing", StatusTone::Bad),
        "modified" => ("\u{25B2} Modified", StatusTone::Warn),
        "embedded" => ("Embedded", StatusTone::Dim),
        "checking" => ("Checking\u{2026}", StatusTone::Dim),
        "tooLarge" => ("Too large to check", StatusTone::Warn),
        _ => ("OK", StatusTone::Good),
    }
}

/// A link row's first use, as the Page · PPI column shows it ("3 · 240 ppi (+1)", or "unused"), and whether its
/// effective resolution is low for print (flagged like the Preflight check).
fn use_info(language: &str, r: &Value) -> (String, bool) {
    let uses = r["uses"].as_array().cloned().unwrap_or_default();
    let info = match uses.first() {
        Some(u) => {
            let page = u["page"].as_str().unwrap_or("PB");
            let ppi = u["ppi"].as_f64().map(|p| format!(" · {p:.0} ppi")).unwrap_or_default();
            let more = if uses.len() > 1 { format!(" (+{})", uses.len() - 1) } else { String::new() };
            format!("{page}{ppi}{more}")
        }
        None => crate::i18n::tr(language, "unused").into(),
    };
    let low = uses.first().and_then(|u| u["ppi"].as_f64()).is_some_and(|p| p < 150.0);
    (info, low)
}

/// Thumbnail edge in the Links panel, in points.
const THUMB: f32 = 28.0;

/// A small rounded label in the status colour.
fn badge(ui: &mut egui::Ui, text: &str, color: egui::Color32) -> egui::Response {
    egui::Frame::NONE
        .fill(color.gamma_multiply(0.18))
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| crate::rtl::label(ui, egui::RichText::new(text).color(color).size(11.0)))
        .response
}

/// Chain links for a linked graphic, a page with a folded corner for an embedded one.
fn storage_icon(ui: &mut egui::Ui, linked: bool, color: egui::Color32, language: &str) {
    let (rect, response) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    paint_storage(ui.painter(), rect, linked, color);
    response.on_hover_text(crate::i18n::tr(
        language,
        if linked { "Linked: the file stays outside the document" } else { "Embedded: the document holds the file" },
    ));
}

/// Paints in `rect` chain links for a linked graphic, a page with a folded corner for an embedded one.
fn paint_storage(p: &egui::Painter, rect: egui::Rect, linked: bool, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.3, color);
    let c = rect.center();
    if linked {
        let link = |center: egui::Pos2| egui::Rect::from_center_size(center, vec2(8.0, 5.0));
        p.rect_stroke(link(c + vec2(-2.0, 2.0)), 2.5, stroke, egui::StrokeKind::Inside);
        p.rect_stroke(link(c + vec2(2.0, -2.0)), 2.5, stroke, egui::StrokeKind::Inside);
    } else {
        let body = egui::Rect::from_center_size(c, vec2(9.0, 12.0));
        let fold = 3.5;
        let pts =
            vec![body.left_top(), body.right_top() + vec2(-fold, 0.0), body.right_top() + vec2(0.0, fold), body.right_bottom(), body.left_bottom()];
        p.add(egui::Shape::closed_line(pts, stroke));
        p.line_segment([body.right_top() + vec2(-fold, 0.0), body.right_top() + vec2(-fold, fold)], stroke);
        p.line_segment([body.right_top() + vec2(-fold, fold), body.right_top() + vec2(0.0, fold)], stroke);
    }
}

/// The proxy of a linked graphic, drawn small. `None` when the graphic has no proxy or it can't be
/// decoded; the answer is kept per proxy so a failed decode isn't retried every frame.
fn proxy_thumb(ctx: &egui::Context, uid: u64, doc: &designcraft_doc::Document, asset: u64) -> Option<egui::TextureHandle> {
    let proxy = doc.assets.get(&designcraft_doc::AssetId(asset))?.proxy.as_ref()?;
    let key = egui::Id::NULL.with(("links_thumb", uid, asset, proxy.data.key().0));
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(key)) {
        return cached;
    }
    let tex = designcraft_render::images::decoded(&proxy.data, 0).and_then(|pm| {
        let (w, h) = (pm.width() as usize, pm.height() as usize);
        if w == 0 || h == 0 {
            return None;
        }
        // Nearest-neighbour reduction to the thumbnail's size at twice the density.
        let k = f64::from(2.0 * THUMB) / w.max(h) as f64;
        let (tw, th) = (((w as f64 * k) as usize).clamp(1, w), ((h as f64 * k) as usize).clamp(1, h));
        let src = pm.data_as_u8_slice();
        let mut px: Vec<u8> = Vec::with_capacity(tw * th * 4);
        for y in 0..th {
            for x in 0..tw {
                let i = ((y * h / th) * w + x * w / tw) * 4;
                let c = src.get(i..i + 4)?;
                px.extend_from_slice(c);
            }
        }
        Some(ctx.load_texture(format!("links-thumb-{asset}"), egui::ColorImage::from_rgba_premultiplied([tw, th], &px), egui::TextureOptions::LINEAR))
    });
    ctx.data_mut(|d| d.insert_temp(key, tex.clone()));
    tex
}

impl DesignApp {
    /// Unembed Link: the graphic is linked to its original file when that still holds the same
    /// bytes, else to a copy written into a folder the user picks.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn unembed_link(&mut self, asset: u64) {
        if self.run("links.unembed", json!({"asset": asset})).is_ok() {
            return;
        }
        let asked = self.ask_folder("Choose a folder for the linked file", Bound::Document, move |app, dir| {
            app.run("links.unembed", json!({"asset": asset, "dir": dir})).inspect_err(|e| app.status_error("Unembed", e))
        });
        if let Err(e) = asked {
            self.status_error("Unembed", e);
        }
    }
}

/// Update Link from the Links panel or the menu (`{asset}`, `{story}`, or `{}` for all): asks
/// first when a linked text story edited in the layout would lose those edits, as InDesign's
/// Links panel does. The `links.update` command itself doesn't ask (as in InDesign scripting).
pub fn update_links(app: &mut DesignApp, target: Value) {
    let one = target.get("story").and_then(Value::as_u64);
    let all = one.is_none() && target.get("asset").is_none();
    let wanted = |r: &Value| (all || (one.is_some() && r["story"].as_u64() == one)) && r["edited"] == true;
    // Which linked stories were edited in the layout is in the document. Only when one was are
    // the files asked whether it changed too: `links.list` asks every linked file, on this
    // thread, and a share that doesn't answer would hold up every other update.
    let any_edited = app.session.active().is_some_and(|st| {
        let document = st.path.as_deref().map(std::path::Path::new);
        designcraft_engine::cmd::preview_document_links(&st.doc, &app.session.cache, document).iter().any(wanted)
    });
    let rows = if any_edited {
        app.session.execute("links.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default()
    } else {
        Vec::new()
    };
    let edited: Vec<&str> = rows.iter().filter(|r| wanted(r) && r["status"] == "modified").filter_map(|r| r["name"].as_str()).collect();
    if edited.is_empty() {
        let _ = run_update_links(app, target);
    } else {
        let files = edited.join(", ");
        app.ui.dialog = Some(crate::dialogs::Dialog::new("updateLink", json!({"target": target, "files": files})));
    }
}

/// Run `links.update`; its first warning goes to the status bar.
pub(crate) fn run_update_links(app: &mut DesignApp, target: Value) -> Result<Value, String> {
    let r = app.run("links.update", target)?;
    if let Some(w) = r["warnings"].as_array().and_then(|w| w.first()).and_then(Value::as_str) {
        app.status(w.to_string());
    }
    Ok(r)
}

pub fn info_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::info_panel(app, ui);
    }
    let t = Tokens::get(ui.ctx());
    for line in info_lines(app) {
        crate::rtl::label(ui, line);
    }
    crate::rtl::label(ui, egui::RichText::new(render_line(app)).color(t.text_dim));
}

/// The Info panel's lines: the selection's position, size, fill and stroke, or the document's page, story and item
/// counts (none without a document).
fn info_lines(app: &mut DesignApp) -> Vec<String> {
    let u = units(app);
    match sel_info(app) {
        Some(i) => {
            let f = |v: f64| designcraft_geom::format_measure(v, u);
            vec![
                format!("X: {}   Y: {}", f(i.page_rect.x0), f(i.page_rect.y0)),
                format!("W: {}   H: {}", f(i.page_rect.width()), f(i.page_rect.height())),
                format!("{}: {}   {}: {}", crate::i18n::tr(&app.ui.language, "Fill"), i.fill, crate::i18n::tr(&app.ui.language, "Stroke"), i.stroke),
            ]
        }
        None => app
            .session
            .active()
            .map(|st| {
                format!(
                    "{} · {} · {}",
                    crate::i18n::count_label(&app.ui.language, "pages", st.doc.page_count()),
                    crate::i18n::count_label(&app.ui.language, "stories", st.doc.stories.len()),
                    crate::i18n::count_label(&app.ui.language, "items", st.doc.all_items().len())
                )
            })
            .into_iter()
            .collect(),
    }
}

/// The Info panel's last line: how long the canvas took to render.
fn render_line(app: &DesignApp) -> String {
    format!("{} {:.1} ms", crate::i18n::tr(&app.ui.language, "Render"), app.perf.render_ms)
}

// PREFLIGHT_ADAPTER_BEGIN
/// A result is a navigation target, never a row index or whichever item happens to be selected.
/// Keep the worker's last-known issues visible, but a held press cannot follow a new document
/// incarnation, revision, item, page, kind or message. No SDK runtime means a working egui row.
fn preflight_issue_button(app: &DesignApp, ui: &mut egui::Ui, issue: &designcraft_engine::cmd::preflight::Issue, page: Option<&str>) -> bool {
    let owner = app.session.active().map(|st| (st.uid, st.identity_epoch, st.revision));
    let id = ui.make_persistent_id(("preflight_issue", owner, issue.severity, issue.kind, &issue.message, issue.item, issue.page));
    let target_exists = app.session.active().is_some_and(|st| {
        issue.item.is_none_or(|id| st.doc.item(designcraft_doc::ItemId(id)).is_some()) && issue.page.is_none_or(|p| p < st.doc.page_count())
    });
    let text = format!("{}  {}", if issue.severity == "error" { "⛔" } else { "⚠" }, issue.message);
    // Page and item context distinguish identical messages and expose the complete action name.
    let name = match (page, issue.item) {
        (Some(page), Some(item)) => format!("{text} · page {page} · item {item}"),
        (Some(page), None) => format!("{text} · page {page}"),
        (None, Some(item)) => format!("{text} · item {item}"),
        (None, None) => text,
    };
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_theme::ControlSize;
        use craft_ui_widgets::contract::{Props as _, Show as _};
        use craft_ui_widgets::style::Variant;
        let out =
            craft_ui_widgets::buttons::Button::new(name.clone()).id(id).enabled(target_exists).size(ControlSize::Sm).variant(Variant::GHOST).show(ui);
        // TEMPORARY (whole-app Preflight click diagnosis, remove once the red is understood).
        #[cfg(test)]
        eprintln!(
            "PREFLIGHT-DEBUG button {name:?}: id={id:?} rect={:?} clip={:?} layer={:?} enabled={target_exists} ui_enabled={} hovered={} \
             contains_pointer={} pointer_down_on={} clicked={} clicks={} sizing={}",
            out.response.rect,
            ui.clip_rect(),
            ui.layer_id(),
            ui.is_enabled(),
            out.response.hovered(),
            out.response.contains_pointer(),
            out.response.is_pointer_button_down_on(),
            out.clicked(),
            out.clicks,
            ui.is_sizing_pass(),
        );
        return out.clicked();
    }
    ui.push_id(id, |ui| ui.add_enabled(target_exists, egui::Button::new(egui::RichText::new(name).size(11.0)).frame(false))).inner.clicked()
}
// PREFLIGHT_ADAPTER_END

pub fn preflight_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework::preflight_panel(app, ui);
    }
    let t = Tokens::get(ui.ctx());
    let found = crate::preflight_worker::issues(app, ui.ctx());
    // The active document's first check hasn't ended.
    let checking = found.is_none() && app.session.active().is_some();
    let issues = found.unwrap_or_default();
    let errors = issues.iter().filter(|i| i.severity == "error").count();
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 4.5, preflight_dot(&t, checking, errors));
        crate::rtl::label(ui, preflight_status(app, checking, &issues, errors));
    });
    divider(ui);
    for i in issues.iter() {
        let page = i.page.filter(|_| app.session.active().is_some()).map(|p| app.session.page_label(p));
        let clicked = preflight_issue_button(app, ui, i, page.as_deref());
        if let Some(pg) = &page {
            crate::rtl::label(ui, egui::RichText::new(format!("   page {pg}")).size(10.0).color(t.text_dim));
        }
        if clicked {
            preflight_go_to(app, i);
        }
    }
}

/// The Preflight panel's status: Checking, No errors, or the error and warning counts.
fn preflight_status(app: &DesignApp, checking: bool, issues: &[designcraft_engine::cmd::preflight::Issue], errors: usize) -> String {
    if checking {
        crate::i18n::tr(&app.ui.language, "Checking").to_string()
    } else if issues.is_empty() {
        crate::i18n::tr(&app.ui.language, "No errors").to_string()
    } else {
        format!(
            "{}, {}",
            crate::i18n::count_label(&app.ui.language, "errors", errors),
            crate::i18n::count_label(&app.ui.language, "warnings", issues.len() - errors)
        )
    }
}

/// The colour of the Preflight status dot: dim while checking, green without errors, red with them.
fn preflight_dot(t: &Tokens, checking: bool, errors: usize) -> egui::Color32 {
    if checking {
        t.text_dim
    } else if errors == 0 {
        egui::Color32::from_rgb(60, 200, 90)
    } else {
        egui::Color32::from_rgb(235, 50, 50)
    }
}

/// Goes to a Preflight issue: selects its item and shows its page.
fn preflight_go_to(app: &mut DesignApp, issue: &designcraft_engine::cmd::preflight::Issue) {
    if let Some(id) = issue.item {
        let _ = app.run("selection.set", json!({"ids": [id]}));
    }
    if let Some(p) = issue.page {
        crate::canvas::go_to_page(app, p);
    }
}

/// The Links, Info and Preflight panels on the craft UI framework: the same rows and actions as the native bodies above,
/// every control and label a framework component. The action buttons and issue rows were already the framework's
/// (`link_button`, `preflight_issue_button`).
#[cfg(not(target_arch = "wasm32"))]
mod framework {
    use super::*;
    use craft_theme::variant::Tone;
    use craft_theme::{ControlSize, Emphasis};
    use craft_ui_widgets::buttons::Button;
    use craft_ui_widgets::contract::{Props as _, Show as _};
    use craft_ui_widgets::display::{Badge, Divider, Elide, Icon, Shade, Text};
    use craft_ui_widgets::primitives::{Glyph, IconSpec};
    use craft_ui_widgets::style::{TypeRole, UiTokens, Variant};
    use egui::{Rect, pos2};

    /// A linked graphic's proxy, drawn small, as a framework glyph.
    #[derive(craft_ui::contract::MemoProps)]
    struct Thumb(#[memo_props(hash = "a texture id is a plain key; the handle stays in the context's cache")] egui::TextureId);

    impl Glyph for Thumb {
        fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, _color: egui::Color32, _mirrored: bool) {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(self.0, rect, uv, egui::Color32::WHITE);
        }
    }

    /// Chain links for a linked graphic, a page with a folded corner for an embedded one, as a framework glyph.
    #[derive(craft_ui::contract::MemoProps)]
    struct Storage(bool);

    impl Glyph for Storage {
        fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, color: egui::Color32, _mirrored: bool) {
            paint_storage(painter, rect, self.0, color);
        }
    }

    /// The framework tone of a link status.
    fn tone(t: StatusTone) -> Tone {
        match t {
            StatusTone::Bad => Tone::Bad,
            StatusTone::Warn => Tone::Warn,
            StatusTone::Dim => Tone::Neutral,
            StatusTone::Good => Tone::Good,
        }
    }

    /// Small secondary text at `at` (or in the flow).
    fn small(ui: &mut egui::Ui, at: Option<Rect>, text: &str, shade: Shade, tone: Option<Tone>) {
        let mut shown = Text::new(text.to_owned()).role(TypeRole::Caption).shade(shade);
        if let Some(tone) = tone {
            shown = shown.tone(tone);
        }
        match at {
            Some(at) => shown.elide(Elide::End).show_at(ui, at),
            None => shown.show(ui),
        };
    }

    pub(super) fn links_body(app: &mut DesignApp, ui: &mut egui::Ui) {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
        // Linked stories (Place and Link).
        let stories = app.session.execute("story.links", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if !stories.is_empty() {
            Text::new(tr("Linked Stories")).role(TypeRole::BodyStrong).show(ui);
            for l in &stories {
                ui.horizontal(|ui| {
                    let label = tr("Story {story} ← story {parent}")
                        .replace("{story}", &l["story"].to_string())
                        .replace("{parent}", &l["parent"].to_string());
                    Text::new(label).show(ui);
                    if l["outOfDate"].as_bool().unwrap_or(false) {
                        Badge::new(tr("⚠ out of date")).variant(Variant::look(Emphasis::Soft, Tone::Warn)).show(ui);
                        if link_button(app, ui, ("linked_story", l["story"].as_u64().unwrap_or(0), "update"), "Update", None, true, true).clicked {
                            let _ = app.run("story.updateLink", json!({"story": l["story"]}));
                        }
                    }
                });
            }
            Divider::horizontal().show(ui);
        }
        let Some((rows, unavailable)) = link_rows(app, ui) else {
            return;
        };
        if unavailable && !rows.is_empty() {
            small(ui, None, &tr("Link status is unavailable."), Shade::Muted, None);
        }
        if rows.is_empty() {
            Text::new(tr("No placed graphics.")).shade(Shade::Muted).show(ui);
            return;
        }
        let sel_key = egui::Id::unique("links_panel_sel");
        let mut chosen: Option<u64> = ui.data(|d| d.get_temp(sel_key));
        // Leave room for Status and Page/PPI instead of widening the rows for long source paths.
        let width = ui.available_width();
        let name_width = (width - 160.0).max(50.0);
        let status_x = name_width + 10.0;
        let info_x = status_x + 90.0;
        let info_w = (width - info_x).max(40.0);
        let b = block(ui, 16.0);
        small(ui, Some(sub(b, 0.0, 0.0, name_width, 16.0)), &tr("Name"), Shade::Muted, None);
        small(ui, Some(sub(b, status_x, 0.0, 80.0, 16.0)), &tr("Status"), Shade::Muted, None);
        small(ui, Some(sub(b, info_x, 0.0, info_w, 16.0)), &tr("Page · PPI"), Shade::Muted, None);
        let uid = app.session.active().map(|st| st.uid);
        for r in rows.iter() {
            // Asset and story ids come from one counter: a row is chosen by either.
            let aid = r["asset"].as_u64().or_else(|| r["story"].as_u64()).unwrap_or(0);
            let name = r["name"].as_str().unwrap_or("");
            let storage = r["storage"].as_str().unwrap_or("");
            let path = r["path"].as_str().filter(|path| !path.is_empty());
            let thumb = if r["story"].is_null() { app.session.active().and_then(|st| proxy_thumb(ui.ctx(), st.uid, &st.doc, aid)) } else { None };
            let b = block(ui, if path.is_some() { 38.0 } else { THUMB });
            let mut x = 0.0;
            if let Some(tex) = thumb {
                let glyph = Thumb(tex.id());
                Icon::new(IconSpec::glyph(&glyph)).show_at(ui, sub(b, 0.0, 0.0, THUMB, THUMB));
                x = THUMB + 6.0;
            }
            let id = ui.make_persistent_id(("links_row", uid, aid));
            let picked = Button::new(name.to_owned())
                .id(id)
                .variant(Variant::GHOST)
                .size(ControlSize::Sm)
                .selected(chosen == Some(aid))
                .show_at(ui, sub(b, x, 0.0, name_width - x, 20.0))
                .clicked();
            let picked_path = path.is_some_and(|path| {
                Button::new(path.to_owned())
                    .id(id.with("path"))
                    .variant(Variant::GHOST)
                    .size(ControlSize::Sm)
                    .tooltip(path.to_owned())
                    .show_at(ui, sub(b, x, 20.0, name_width - x, 16.0))
                    .clicked()
            });
            if picked || picked_path {
                chosen = Some(aid);
            }
            let (words, look) = status_look(r["status"].as_str().unwrap_or(""));
            let mut sx = status_x;
            if !storage.is_empty() {
                let linked = storage == "linked";
                let glyph = Storage(linked);
                let tip = tr(if linked { "Linked: the file stays outside the document" } else { "Embedded: the document holds the file" });
                Icon::new(IconSpec::glyph(&glyph)).label(tip).show_at(ui, sub(b, sx, 3.0, 14.0, 14.0));
                sx += 18.0;
            }
            let mut badge = Badge::new(tr(words)).variant(Variant::look(Emphasis::Soft, tone(look)));
            // Why a link isn't followed (a path on another computer, a device).
            if let Some(why) = r["refused"].as_str() {
                badge = badge.tooltip(why.to_owned());
            }
            badge.show_at(ui, sub(b, sx, 1.0, (info_x - 4.0 - sx).max(30.0), 18.0));
            let (info, low) = use_info(&lang, r);
            small(ui, Some(sub(b, info_x, 2.0, info_w, 16.0)), &info, Shade::Muted, low.then_some(Tone::Warn));
            ui.add_space(4.0);
        }
        if let Some(c) = chosen {
            ui.data_mut(|d| d.insert_temp(sel_key, c));
        }
        ui.add_space(6.0);
        if let Some(path) = rows
            .iter()
            .find(|r| r["asset"].as_u64().or_else(|| r["story"].as_u64()) == chosen)
            .and_then(|r| r["path"].as_str())
            .filter(|path| !path.is_empty())
        {
            // A selected link stays distinguishable even if two rows truncate to the same prefix.
            small(ui, None, path, Shade::Muted, None);
            ui.add_space(6.0);
        }
        link_actions(app, ui, &rows, chosen);
    }

    pub(super) fn info_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
        for line in info_lines(app) {
            Text::new(line).show(ui);
        }
        Text::new(render_line(app)).shade(Shade::Muted).show(ui);
    }

    pub(super) fn preflight_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
        let found = crate::preflight_worker::issues(app, ui.ctx());
        // The active document's first check hasn't ended.
        let checking = found.is_none() && app.session.active().is_some();
        let issues = found.unwrap_or_default();
        let errors = issues.iter().filter(|i| i.severity == "error").count();
        let b = block(ui, FIELD_H);
        // The status dot is drawing only; the status text after it is what is read out.
        let t = Tokens::get(ui.ctx());
        ui.painter().circle_filled(sub(b, 0.0, 6.0, 10.0, 10.0).center(), 4.5, preflight_dot(&t, checking, errors));
        Text::new(preflight_status(app, checking, &issues, errors)).show_at(ui, sub(b, 16.0, 0.0, (b.width() - 16.0).max(40.0), FIELD_H));
        divider(ui);
        for i in issues.iter() {
            let page = i.page.filter(|_| app.session.active().is_some()).map(|p| app.session.page_label(p));
            let clicked = preflight_issue_button(app, ui, i, page.as_deref());
            if let Some(pg) = &page {
                small(ui, None, &format!("   page {pg}"), Shade::Muted, None);
            }
            if clicked {
                preflight_go_to(app, i);
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui223_links_tests {
    use super::*;
    use designcraft_doc::{Asset, AssetData, AssetId, Content, Graphic, ItemId, Storage, StoryId, TextFileLink};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{Harness, kittest::Queryable};
    use std::sync::Arc;

    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#4488cc"/></svg>"##;

    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"title": "Links actions"})).unwrap();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(None));
        app
    }

    /// Relative sources in an unsaved document are refused before filesystem access. The SVG
    /// bytes and all document content are in memory; none of these fixtures writes a file.
    fn graphics() -> (DesignApp, Vec<ItemId>, Vec<AssetId>) {
        let mut app = app();
        let mut items = Vec::new();
        let mut assets = Vec::new();
        for (index, path) in [Some("studio-a/photo.svg"), Some("studio-b/photo.svg"), None].into_iter().enumerate() {
            let x = 72 + index * 100;
            let item = ItemId(app.run("frame.create", json!({"rect": [x, 72, x + 72, 144]})).unwrap()["id"].as_u64().unwrap());
            let doc = Arc::make_mut(&mut app.session.doc_mut().unwrap().doc);
            let asset = AssetId(doc.alloc());
            doc.assets.insert(
                asset,
                Arc::new(Asset {
                    id: asset,
                    name: if path.is_some() { "photo.svg" } else { "embedded.svg" }.into(),
                    mime: "image/svg+xml".into(),
                    link: path.map(str::to_owned),
                    storage: if path.is_some() { Storage::Linked } else { Storage::Embedded },
                    data: AssetData::embedded(SVG.to_vec()),
                    pixels: Some((32, 32)),
                    ..Default::default()
                }),
            );
            doc.item_mut(item).unwrap().content = Content::Graphic(Graphic {
                layer_options: None,
                asset,
                size: (72.0, 72.0),
                xf: designcraft_geom::Affine::IDENTITY,
                clip: None,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: Default::default(),
            });
            items.push(item);
            assets.push(asset);
        }
        (app, items, assets)
    }

    fn linked_stories() -> (DesignApp, StoryId, StoryId, StoryId) {
        let mut app = app();
        let parent = StoryId(
            app.run("frame.create", json!({"rect": [72, 72, 200, 140], "content": "text", "text": "Original"})).unwrap()["story"].as_u64().unwrap(),
        );
        let a = StoryId(app.run("story.placeAndLink", json!({"story": parent.0, "rect": [72, 160, 200, 230]})).unwrap()["story"].as_u64().unwrap());
        let b = StoryId(app.run("story.placeAndLink", json!({"story": parent.0, "rect": [72, 250, 200, 320]})).unwrap()["story"].as_u64().unwrap());
        app.run("story.setText", json!({"story": parent.0, "text": "Revised"})).unwrap();
        (app, parent, a, b)
    }

    fn region() -> egui::Id {
        egui::Id::NULL.with("ui223_links_test_region")
    }

    /// A real installed Arabic face is an explicit native fixture precondition, not an OS
    /// package install or a synthetic glyph map. The app's current SYSTEM_UI_FONTS list lacks
    /// Arab without optional CRAFT_FONTS; retain all its current definitions and add this face
    /// using the app's existing FontDb and baseline-aware fallback API.
    fn install_arabic_fixture(ctx: &egui::Context, language: &str) {
        #[cfg(target_os = "macos")]
        let family = "Geeza Pro";
        #[cfg(target_os = "windows")]
        let family = "Segoe UI";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let family = "DejaVu Sans";
        let db = designcraft_fonts::FontDb::global();
        assert!(db.has_family(family), "the native Arabic fixture requires the already-installed {family}; do not skip Arabic or substitute tofu");
        let face = db.face(family, "Regular");
        assert!(face.family.eq_ignore_ascii_case(family), "the named fixture must resolve to {family}, not {}", face.family);
        assert!(!face.is_placeholder() && face.covers('\u{0627}'), "the Arabic face must contain a real alef glyph");
        for key in [
            "Name",
            "Status",
            "Page · PPI",
            "Choose a link.",
            "Relink…",
            "Go To",
            "Update",
            "Unlink",
            "Embed",
            "Unembed…",
            "Relink to Folder…",
            "Embedded",
            "⚠ Missing",
            "OK",
            "unused",
            "No placed graphics.",
            "Copy the file into the document",
            "Keep the graphic in a file and link to it",
        ] {
            for c in crate::i18n::tr(language, key).chars().filter(|c| designcraft_fonts::is_rtl(*c) && c.is_alphabetic()) {
                assert!(face.covers(c), "{family} lacks the real glyph U+{:04X} from {key}", u32::from(c));
            }
        }
        let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        let fallback = crate::theme::SystemFont::new("Arab", face.family.clone(), face.data().to_vec(), face.index());
        crate::theme::add_system_fallbacks(&mut definitions, &[fallback], language);
        ctx.set_fonts(definitions);
        eprintln!(
            "Arabic fixture: real existing {} {}, face {}, {} font bytes, explicit Links alphabetic-glyph coverage checked",
            face.family,
            face.style,
            face.index(),
            face.data().len()
        );
    }

    /// Real production panel, runtime hooks and input hook, but no desktop window or native chooser.
    fn harness(mut app: DesignApp, sdk: bool) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        if sdk {
            app.framework_opacity = true;
            app.install_framework(&ctx);
            assert!(app.framework_installed);
        }
        let mut ready = false;
        let mut locale_fonts_ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(500.0, 700.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                if !locale_fonts_ready {
                    locale_fonts_ready = true;
                    if app.ui.language == "ar" {
                        install_arabic_fixture(ui.ctx(), &app.ui.language);
                        return;
                    }
                }
                let _ = craft_ui_widgets::runtime::inspect::record(ui, region(), |ui| links_panel(app, ui));
                crate::dialogs::show(app, ui.ctx());
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 3);
        h
    }

    fn frame(h: &mut Harness<'_, DesignApp>, event: Option<Event>) {
        let mut raw = std::mem::take(h.input_mut());
        if let Some(event) = event {
            raw.events.push(event);
        }
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    fn idle(h: &mut Harness<'_, DesignApp>, frames: usize) {
        for _ in 0..frames {
            frame(h, None);
        }
    }

    fn click_at(h: &mut Harness<'_, DesignApp>, rect: egui::Rect) {
        let pos = rect.center();
        frame(h, Some(Event::PointerMoved(pos)));
        for pressed in [true, false] {
            frame(h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
        }
        idle(h, 3);
    }

    fn click(h: &mut Harness<'_, DesignApp>, label: &str) {
        let rect = h.get_by_label(label).rect();
        click_at(h, rect);
    }

    fn key(h: &mut Harness<'_, DesignApp>, key: Key) {
        for pressed in [true, false] {
            frame(h, Some(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }));
        }
        idle(h, 3);
    }

    fn button(h: &Harness<'_, DesignApp>, label: &str) -> craft_ui_widgets::runtime::inspect::Element {
        let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, region()).unwrap();
        record.elements.iter().find(|e| e.component == "Button" && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(label)).unwrap().clone()
    }

    fn undo(app: &DesignApp) -> usize {
        app.session.active().unwrap().history.undo.len()
    }
    fn text(app: &DesignApp, story: StoryId) -> String {
        app.session.active().unwrap().doc.story(story).unwrap().text.clone()
    }
    fn command_count(app: &DesignApp, command: &str) -> usize {
        app.session.journal.iter().filter(|(id, _)| id == command).count()
    }

    fn assert_last_command(app: &DesignApp, command: &str, payload: Value) {
        assert_eq!(app.session.journal.last(), Some(&(command.to_owned(), payload)));
    }

    #[test]
    fn ui223_links_pointer_go_to_preserves_same_named_asset_identity() {
        let (app, items, assets) = graphics();
        let mut h = harness(app, true);
        click(&mut h, "studio-b/photo.svg");
        let steps = undo(h.state());
        let count = command_count(h.state(), "links.goTo");
        let go = button(&h, "Go To");
        assert!(!go.facts.as_ref().unwrap().disabled);
        click_at(&mut h, go.rect);
        assert_last_command(h.state(), "links.goTo", json!({"asset": assets[1].0}));
        assert_eq!(h.state().session.active().unwrap().selection.items, vec![items[1]]);
        assert_eq!(command_count(h.state(), "links.goTo"), count + 1);
        assert_eq!(undo(h.state()), steps, "navigation is not a document edit");
        let journal = h.state().session.journal.clone();
        idle(&mut h, 5);
        assert_eq!(h.state().session.journal, journal, "idle never replays the click");
        click(&mut h, "studio-a/photo.svg");
        click(&mut h, "Go To");
        assert_eq!(h.state().session.active().unwrap().selection.items, vec![items[0]]);
    }

    #[test]
    fn ui223_links_missing_graphic_update_and_embed_are_inert() {
        let (app, _, _) = graphics();
        let mut h = harness(app, true);
        click(&mut h, "studio-b/photo.svg");
        let before = Arc::clone(&h.state().session.active().unwrap().doc);
        let journal = h.state().session.journal.clone();
        let steps = undo(h.state());
        for label in ["Update", "Embed"] {
            let disabled = button(&h, label);
            assert!(disabled.facts.as_ref().unwrap().disabled, "{label}");
            click_at(&mut h, disabled.rect);
        }
        idle(&mut h, 5);
        assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &before));
        assert_eq!(undo(h.state()), steps);
        assert_eq!(h.state().session.journal, journal);
    }

    #[test]
    fn ui223_links_pointer_story_update_targets_one_child_and_undo_restores_it() {
        let (app, parent, a, b) = linked_stories();
        let frames = app.session.active().unwrap().doc.story(b).unwrap().frames.clone();
        let mut h = harness(app, true);
        let label = format!("Story {} ← story {}", b.0, parent.0);
        let y = h.get_by_label(&label).rect().center().y;
        let rect = h.get_all_by_label("Update").find(|n| n.rect().y_range().contains(y)).unwrap().rect();
        let steps = undo(h.state());
        let count = command_count(h.state(), "story.updateLink");
        click_at(&mut h, rect);
        assert_last_command(h.state(), "story.updateLink", json!({"story": b.0}));
        assert_eq!(text(h.state(), b), "Revised");
        assert_eq!(text(h.state(), a), "Original");
        assert_eq!(h.state().session.active().unwrap().doc.story(b).unwrap().frames, frames);
        assert_eq!(undo(h.state()), steps + 1);
        idle(&mut h, 5);
        assert_eq!(command_count(h.state(), "story.updateLink"), count + 1);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(text(h.state(), b), "Original");
        assert_eq!(text(h.state(), a), "Original");
        assert_eq!(h.state().session.active().unwrap().doc.story(b).unwrap().frames, frames);
    }

    #[test]
    fn ui223_links_natural_tab_and_enter_update_story_once() {
        let (mut app, _, a, b) = linked_stories();
        // Only one out-of-date child, so the natural Tab target is unambiguous.
        app.run("story.updateLink", json!({"story": b.0})).unwrap();
        let mut h = harness(app, true);
        let update = button(&h, "Update");
        for _ in 0..8 {
            key(&mut h, Key::Tab);
            if h.ctx.memory(|m| m.focused()) == Some(update.id) {
                break;
            }
        }
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(update.id), "Tab must naturally reach the real SDK Button");
        assert_eq!(text(h.state(), a), "Original", "Tab does not activate");
        let count = command_count(h.state(), "story.updateLink");
        let steps = undo(h.state());
        key(&mut h, Key::Enter);
        assert_eq!(text(h.state(), a), "Revised");
        assert_last_command(h.state(), "story.updateLink", json!({"story": a.0}));
        assert_eq!(undo(h.state()), steps + 1);
        idle(&mut h, 5);
        assert_eq!(command_count(h.state(), "story.updateLink"), count + 1);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(text(h.state(), a), "Original");
    }

    #[test]
    fn ui223_links_file_story_unlink_uses_story_payload_and_one_undo() {
        let mut app = app();
        let mut stories = Vec::new();
        for (index, path) in ["studio-a/chapter.txt", "studio-b/chapter.txt"].into_iter().enumerate() {
            let story = StoryId(
                app.run("frame.create", json!({"rect": [72, 72 + 100 * index, 300, 140 + 100 * index], "content": "text", "text": "Layout edits"}))
                    .unwrap()["story"]
                    .as_u64()
                    .unwrap(),
            );
            Arc::make_mut(&mut app.session.doc_mut().unwrap().doc).story_mut(story).unwrap().file_link =
                Some(TextFileLink { path: path.into(), ..Default::default() });
            stories.push(story);
        }
        let mut h = harness(app, true);
        click(&mut h, "studio-b/chapter.txt");
        assert!(!button(&h, "Update").facts.as_ref().unwrap().disabled, "text can always be imported again");
        let before_update = undo(h.state());
        click(&mut h, "Update");
        assert_last_command(h.state(), "links.update", json!({"story": stories[1].0}));
        assert_eq!(undo(h.state()), before_update, "a missing source is not a document edit");
        assert_eq!(text(h.state(), stories[1]), "Layout edits");
        assert!(h.state().ui.dialog.is_none(), "a missing source does not replace edited text");
        let steps = undo(h.state());
        let count = command_count(h.state(), "links.embed");
        click(&mut h, "Unlink");
        assert_last_command(h.state(), "links.embed", json!({"story": stories[1].0}));
        let doc = &h.state().session.active().unwrap().doc;
        assert!(doc.story(stories[0]).unwrap().file_link.is_some());
        assert!(doc.story(stories[1]).unwrap().file_link.is_none());
        assert_eq!(text(h.state(), stories[1]), "Layout edits");
        assert_eq!(undo(h.state()), steps + 1);
        idle(&mut h, 5);
        assert_eq!(command_count(h.state(), "links.embed"), count + 1);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.active().unwrap().doc.story(stories[1]).unwrap().file_link.as_ref().unwrap().path, "studio-b/chapter.txt");
    }

    #[test]
    fn ui223_links_cancelled_relink_folder_and_unembed_leave_document_unchanged() {
        let (app, _, assets) = graphics();
        let mut h = harness(app, true);
        click(&mut h, "embedded.svg");
        for label in ["Relink…", "Relink to Folder…", "Unembed…"] {
            let before = Arc::clone(&h.state().session.active().unwrap().doc);
            let journal = h.state().session.journal.clone();
            let status = h.state().ui.status.clone();
            let steps = undo(h.state());
            click(&mut h, label);
            assert_eq!(h.state().file_dialogs.queued(), 1, "{label} asks, not writes");
            let asked_status = h.state().ui.status.clone();
            if label == "Unembed…" {
                // Existing app.run reports the failed original-file attempt before asking for
                // a folder. Cancel must not erase that truthful message or report a success.
                assert!(asked_status.contains("its original file is missing or has changed"), "{asked_status}");
            } else {
                assert_eq!(asked_status, status);
            }
            h.state_mut().settle_dialogs();
            idle(&mut h, 5);
            assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &before), "{label}");
            assert_eq!(undo(h.state()), steps, "{label}");
            assert_eq!(h.state().session.journal, journal, "cancel runs no continuation: {label}");
            assert_eq!(h.state().ui.status, asked_status);
            assert_eq!(h.state().session.active().unwrap().doc.assets[&assets[2]].storage, Storage::Embedded);
        }
    }

    #[test]
    fn ui223_links_cancel_edited_story_question_preserves_text_and_history() {
        let mut app = app();
        let story = StoryId(
            app.run("frame.create", json!({"rect": [72, 72, 300, 140], "content": "text", "text": "Layout edits"})).unwrap()["story"]
                .as_u64()
                .unwrap(),
        );
        // Seed the existing question as an in-memory fixture; positive file-change detection is
        // not claimed here, since that requires real source files. Render and cancel the real dialog.
        app.ui.dialog = Some(crate::dialogs::Dialog::new("updateLink", json!({"target": {"story": story.0}, "files": "chapter.txt"})));
        let mut h = harness(app, true);
        let journal = h.state().session.journal.clone();
        let steps = undo(h.state());
        key(&mut h, Key::Escape);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(text(h.state(), story), "Layout edits");
        assert_eq!(undo(h.state()), steps);
        idle(&mut h, 5);
        assert_eq!(h.state().session.journal, journal);
    }

    #[test]
    fn ui223_links_without_runtime_retains_working_native_actions() {
        let (app, items, _) = graphics();
        let mut h = harness(app, false);
        assert!(!h.state().framework_installed);
        click(&mut h, "studio-b/photo.svg");
        let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, region()).unwrap();
        assert!(record.elements.iter().all(|e| e.component != "Button"), "fallback must not paint inert SDK controls");
        let steps = undo(h.state());
        click(&mut h, "Go To");
        assert_eq!(h.state().session.active().unwrap().selection.items, vec![items[1]]);
        assert_eq!(undo(h.state()), steps);
    }
    #[test]
    fn ui223_links_localized_actions_keep_asset_identity() {
        for language in ["fr", "ar"] {
            let (mut app, items, assets) = graphics();
            app.ui.language = language.into();
            let label = crate::i18n::tr(language, "Go To").to_owned();
            let mut h = harness(app, true);
            click(&mut h, "studio-b/photo.svg");
            let go = button(&h, &label);
            click_at(&mut h, go.rect);
            assert_last_command(h.state(), "links.goTo", json!({"asset": assets[1].0}));
            assert_eq!(h.state().session.active().unwrap().selection.items, vec![items[1]]);
        }
    }
}

// PREFLIGHT_TESTS_BEGIN
#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui_preflight_navigation_tests {
    use super::*;
    use designcraft_doc::{Document, ItemId, Selection};
    use designcraft_engine::cmd::preflight::Issue;
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::sync::Arc;

    type History = Vec<(String, Arc<Document>, Selection)>;
    struct QuietDoc {
        uid: u64,
        epoch: u64,
        document: Arc<Document>,
        saved: Arc<Document>,
        undo: History,
        redo: History,
        saved_revision: u64,
        history_limit: usize,
        dirty: bool,
        selection: Selection,
    }

    fn quiet(app: &DesignApp) -> Vec<QuietDoc> {
        let history = |entries: &[designcraft_engine::HistoryEntry]| -> History {
            entries.iter().map(|entry| (entry.label.clone(), entry.doc.clone(), entry.selection.clone())).collect()
        };
        app.session
            .documents()
            .iter()
            .map(|st| QuietDoc {
                uid: st.uid,
                epoch: st.identity_epoch,
                document: Arc::clone(&st.doc),
                saved: Arc::clone(&st.saved_doc),
                undo: history(&st.history.undo),
                redo: history(&st.history.redo),
                saved_revision: st.saved_revision,
                history_limit: st.history.limit,
                dirty: st.is_dirty(),
                selection: st.selection.clone(),
            })
            .collect()
    }

    fn assert_quiet(app: &DesignApp, before: &[QuietDoc], navigation_uid: Option<u64>) {
        let now = quiet(app);
        assert_eq!(now.len(), before.len());
        for (now, before) in now.iter().zip(before) {
            assert_eq!(
                (now.uid, now.epoch, now.saved_revision, now.history_limit, now.dirty),
                (before.uid, before.epoch, before.saved_revision, before.history_limit, before.dirty)
            );
            assert!(Arc::ptr_eq(&now.document, &before.document), "navigation preserves each document allocation");
            assert!(Arc::ptr_eq(&now.saved, &before.saved), "navigation preserves each saved allocation");
            assert_eq!(now.document.as_ref(), before.document.as_ref());
            assert_eq!(now.saved.as_ref(), before.saved.as_ref());
            assert_eq!(now.undo, before.undo);
            assert_eq!(now.redo, before.redo);
            if Some(now.uid) != navigation_uid {
                assert_eq!(now.selection, before.selection, "another document is not a navigation target");
            }
        }
    }

    fn fixture() -> (DesignApp, Vec<u64>) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 3, "facingPages": false})).unwrap();
        let mut items = Vec::new();
        for page in 0..3 {
            let spread = app.session.active().unwrap().doc.page_loc(page).unwrap().0;
            let text = if page == 2 { String::new() } else { "Words that do not fit the frame. ".repeat(40) };
            let result = app.run("frame.create", json!({"spread": spread, "rect": [72, 72, 200, 100], "content": "text", "text": text})).unwrap();
            items.push(result["id"].as_u64().unwrap());
        }
        app.run("selection.set", json!({"ids": []})).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0)));
        app.view_mut().unwrap().zoom = 1.0;
        (app, items)
    }

    fn issue(app: &mut DesignApp, ctx: &egui::Context, item: u64, kind: &str) -> Issue {
        // This official test path executes the REAL check_document inline once per state.
        // No injected issues, invented worker result or claim about background-thread completion.
        crate::preflight_worker::issues(app, ctx).unwrap().iter().find(|issue| issue.item == Some(item) && issue.kind == kind).unwrap().clone()
    }

    fn name(app: &DesignApp, issue: &Issue) -> String {
        let text = format!("{}  {}", if issue.severity == "error" { "⛔" } else { "⚠" }, issue.message);
        match (issue.page.map(|p| app.session.page_label(p)), issue.item) {
            (Some(page), Some(item)) => format!("{text} · page {page} · item {item}"),
            (Some(page), None) => format!("{text} · page {page}"),
            (None, Some(item)) => format!("{text} · item {item}"),
            (None, None) => text,
        }
    }

    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        if installed {
            app.framework_opacity = true;
            app.install_framework(&ctx);
            assert!(app.framework_installed);
        }
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(640.0, 700.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                let _ =
                    craft_ui_widgets::runtime::inspect::record(ui, egui::Id::NULL.with("preflight_issue_test_region"), |ui| preflight_panel(app, ui));
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 3);
        h
    }

    fn frame(h: &mut Harness<'_, DesignApp>, event: Option<Event>) {
        let mut raw = std::mem::take(h.input_mut());
        if let Some(event) = event {
            raw.events.push(event);
        }
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, DesignApp>, count: usize) {
        for _ in 0..count {
            frame(h, None);
        }
    }
    fn pointer(h: &mut Harness<'_, DesignApp>, pos: egui::Pos2, pressed: bool) {
        frame(h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
    }
    fn click(h: &mut Harness<'_, DesignApp>, label: &str) {
        let pos = h.get_by_role_and_label(egui::accesskit::Role::Button, label).rect().center();
        frame(h, Some(Event::PointerMoved(pos)));
        pointer(h, pos, true);
        pointer(h, pos, false);
        idle(h, 3);
    }
    fn key_event(h: &mut Harness<'_, DesignApp>, key: Key, pressed: bool) {
        frame(h, Some(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }));
    }
    fn key(h: &mut Harness<'_, DesignApp>, key: Key) {
        key_event(h, key, true);
        key_event(h, key, false);
        idle(h, 3);
    }
    fn tab_to(h: &mut Harness<'_, DesignApp>, label: &str) {
        let count = h.query_all_by_role(egui::accesskit::Role::Button).count();
        for _ in 0..count + 1 {
            if h.get_by_role_and_label(egui::accesskit::Role::Button, label).is_focused() {
                return;
            }
            key(h, Key::Tab);
        }
        assert!(h.get_by_role_and_label(egui::accesskit::Role::Button, label).is_focused(), "natural Tab reaches the exact real issue row");
    }
    fn sdk_row(h: &Harness<'_, DesignApp>, label: &str) {
        let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with("preflight_issue_test_region")).unwrap();
        let matches: Vec<_> = record
            .elements
            .iter()
            .filter(|element| element.component == "Button" && element.facts.as_ref().and_then(|facts| facts.name.as_deref()) == Some(label))
            .collect();
        assert_eq!(matches.len(), 1, "the complete actual issue name resolves to one real SDK Button");
        assert!(!matches[0].facts.as_ref().unwrap().disabled);
        assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::Button, label).count(), 1);
    }

    fn assert_navigation(h: &Harness<'_, DesignApp>, item: u64, page: usize, before: &[QuietDoc], journal_len: usize) {
        let app = h.state();
        let st = app.session.active().unwrap();
        assert_eq!(st.selection.items, vec![ItemId(item)]);
        assert_eq!(crate::canvas::current_page(app), Some(page));
        assert_eq!(app.session.journal.len(), journal_len + 1, "exactly one real selection command");
        assert_eq!(app.session.journal.last(), Some(&("selection.set".to_owned(), json!({"ids": [item]}))));
        assert_quiet(app, before, Some(st.uid));
    }

    #[test]
    fn ui_preflight_sdk_pointer_targets_duplicate_messages_on_distinct_pages_without_editing() {
        let (mut app, items) = fixture();
        let ctx = egui::Context::default();
        let a = issue(&mut app, &ctx, items[0], "overset");
        let b = issue(&mut app, &ctx, items[1], "overset");
        assert_eq!(a.message, b.message, "fixture exercises duplicate real messages");
        assert_eq!((a.page, b.page), (Some(0), Some(1)));
        let names = [name(&app, &a), name(&app, &b)];
        assert_ne!(names[0], names[1]);
        let mut h = harness(app, true);
        let before = quiet(h.state());
        for (at, page) in [(1, 1), (0, 0)] {
            assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::Button, &names[at]).count(), 1);
            sdk_row(&h, &names[at]);
            let journal = h.state().session.journal.len();
            click(&mut h, &names[at]);
            assert_navigation(&h, items[at], page, &before, journal);
            let after = h.state().session.journal.clone();
            idle(&mut h, 3);
            assert_eq!(h.state().session.journal, after, "idle never repeats navigation");
        }
    }

    #[test]
    fn ui_preflight_sdk_natural_tab_enter_and_space_target_actual_error_and_warning() {
        let (mut app, items) = fixture();
        let ctx = egui::Context::default();
        let error = issue(&mut app, &ctx, items[1], "overset");
        let warning = issue(&mut app, &ctx, items[2], "emptyText");
        assert_eq!((error.severity, warning.severity), ("error", "warning"));
        let targets = [(name(&app, &error), items[1], 1, Key::Enter), (name(&app, &warning), items[2], 2, Key::Space)];
        let mut h = harness(app, true);
        let before = quiet(h.state());
        for (label, item, page, activate) in targets {
            sdk_row(&h, &label);
            let journal = h.state().session.journal.clone();
            let selection = h.state().session.active().unwrap().selection.clone();
            tab_to(&mut h, &label);
            assert_eq!(h.state().session.journal, journal, "Tab is navigation only, not activation");
            assert_eq!(h.state().session.active().unwrap().selection, selection);
            let focused = h.get_by_role_and_label(egui::accesskit::Role::Button, &label).accesskit_node().id();
            let actual_focus: Vec<_> = h.query_all_by(|n| n.is_focused()).map(|n| n.accesskit_node().id()).collect();
            assert_eq!(actual_focus, vec![focused], "one actual AX-focused issue stop before activation");
            key(&mut h, activate);
            assert_navigation(&h, item, page, &before, journal.len());
        }
    }

    #[test]
    fn ui_preflight_status_counts_and_no_document_and_uninstalled_fallback_remain_real() {
        let (mut app, items) = fixture();
        let ctx = egui::Context::default();
        let found = crate::preflight_worker::issues(&mut app, &ctx).unwrap();
        let errors = found.iter().filter(|i| i.severity == "error").count();
        let status = format!(
            "{}, {}",
            crate::i18n::count_label(&app.ui.language, "errors", errors),
            crate::i18n::count_label(&app.ui.language, "warnings", found.len() - errors)
        );
        assert!(errors >= 2 && found.len() - errors >= 1);
        let warning = issue(&mut app, &ctx, items[2], "emptyText");
        let label = name(&app, &warning);
        let mut h = harness(app, false);
        assert!(!h.state().framework_installed);
        assert!(h.query_by_label(&status).is_some());
        let before = quiet(h.state());
        let journal = h.state().session.journal.len();
        click(&mut h, &label);
        assert_navigation(&h, items[2], 2, &before, journal);
        h.state_mut().run("file.new", json!({"title": "Clean Preflight"})).unwrap();
        idle(&mut h, 3);
        assert!(h.query_by_label("No errors").is_some());
        assert_eq!(h.query_all_by_role(egui::accesskit::Role::Button).count(), 0);
        while !h.state().session.documents().is_empty() {
            h.state_mut().session.execute("file.close", &json!({"force": true})).unwrap();
        }
        let journal = h.state().session.journal.clone();
        idle(&mut h, 3);
        assert!(h.query_by_label("No errors").is_some());
        assert_eq!(h.query_all_by_role(egui::accesskit::Role::Button).count(), 0);
        assert_eq!(h.state().session.journal, journal);
    }

    #[test]
    fn ui_preflight_held_pointer_cannot_follow_switched_replaced_or_closed_document_and_recovers() {
        for change in ["switch", "replace", "close"] {
            let (mut app, items) = fixture();
            let ctx = egui::Context::default();
            let target = issue(&mut app, &ctx, items[1], "overset");
            let label = name(&app, &target);
            let mut h = harness(app, true);
            let pos = h.get_by_role_and_label(egui::accesskit::Role::Button, &label).rect().center();
            frame(&mut h, Some(Event::PointerMoved(pos)));
            pointer(&mut h, pos, true);
            let old_uid = h.state().session.active().unwrap().uid;
            let old_epoch = h.state().session.active().unwrap().identity_epoch;
            if change == "switch" {
                // The new document repeats the REAL item IDs/message/page: uid, not a row index,
                // distinguishes its actions from the old press.
                let doc = h.state().session.active().unwrap().doc.as_ref().clone();
                h.state_mut().run("file.new", json!({})).unwrap();
                let current = h.state_mut().session.active_mut().unwrap();
                current.doc = Arc::new(doc);
                current.bump_revision();
            } else if change == "replace" {
                let doc = h.state().session.active().unwrap().doc.as_ref().clone();
                let mut replacement = designcraft_engine::DocState::new(doc, None);
                replacement.uid = old_uid;
                assert_ne!(replacement.identity_epoch, old_epoch);
                *h.state_mut().session.active_mut().unwrap() = replacement;
            } else {
                h.state_mut().session.execute("file.close", &json!({"force": true})).unwrap();
            }
            frame(&mut h, None);
            let before = quiet(h.state());
            let journal = h.state().session.journal.clone();
            pointer(&mut h, pos, false);
            idle(&mut h, 3);
            assert_eq!(h.state().session.journal, journal, "old release cannot navigate a new incarnation or absent document: {change}");
            assert_quiet(h.state(), &before, None);
            if change == "close" {
                continue;
            }
            let current = issue(h.state_mut(), &ctx, items[1], "overset");
            let fresh = name(h.state(), &current);
            let journal = h.state().session.journal.len();
            click(&mut h, &fresh);
            assert_navigation(&h, items[1], 1, &before, journal);
        }
    }

    #[test]
    fn ui_preflight_held_enter_cannot_follow_document_switch_and_fresh_keyboard_recovers() {
        let (mut app, items) = fixture();
        let ctx = egui::Context::default();
        let target = issue(&mut app, &ctx, items[0], "overset");
        let label = name(&app, &target);
        let mut h = harness(app, true);
        tab_to(&mut h, &label);
        key_event(&mut h, Key::Enter, true);
        let doc = h.state().session.active().unwrap().doc.as_ref().clone();
        h.state_mut().run("file.new", json!({})).unwrap();
        let current = h.state_mut().session.active_mut().unwrap();
        current.doc = Arc::new(doc);
        current.bump_revision();
        frame(&mut h, None);
        let before = quiet(h.state());
        let journal = h.state().session.journal.clone();
        key_event(&mut h, Key::Enter, false);
        idle(&mut h, 3);
        assert_eq!(h.state().session.journal, journal, "held Enter cannot retarget the new document");
        assert_quiet(h.state(), &before, None);
        tab_to(&mut h, &label);
        let journal = h.state().session.journal.len();
        key(&mut h, Key::Enter);
        assert_navigation(&h, items[0], 0, &before, journal);
    }

    #[test]
    fn ui_preflight_held_pointer_drops_removed_issue_then_fresh_target_recovers() {
        let (mut app, items) = fixture();
        let ctx = egui::Context::default();
        let old = issue(&mut app, &ctx, items[1], "overset");
        let old_label = name(&app, &old);
        let mut h = harness(app, true);
        let pos = h.get_by_role_and_label(egui::accesskit::Role::Button, &old_label).rect().center();
        frame(&mut h, Some(Event::PointerMoved(pos)));
        pointer(&mut h, pos, true);
        h.state_mut().run("selection.set", json!({"ids": [items[1]]})).unwrap();
        h.state_mut().run("edit.clear", json!({})).unwrap();
        frame(&mut h, None);
        assert!(h.query_by_role_and_label(egui::accesskit::Role::Button, &old_label).is_none());
        let before = quiet(h.state());
        let journal = h.state().session.journal.clone();
        pointer(&mut h, pos, false);
        idle(&mut h, 3);
        assert_eq!(h.state().session.journal, journal, "the old press cannot follow the row now occupying that position");
        assert_quiet(h.state(), &before, None);
        let fresh = issue(h.state_mut(), &ctx, items[0], "overset");
        let fresh_label = name(h.state(), &fresh);
        let journal = h.state().session.journal.len();
        click(&mut h, &fresh_label);
        assert_navigation(&h, items[0], 0, &before, journal);
    }

    #[test]
    fn ui_preflight_whole_app_parent_opens_real_panel_and_navigates_exact_issue() {
        let (mut app, items) = fixture();
        let ctx = egui::Context::default();
        let target = issue(&mut app, &ctx, items[1], "overset");
        let label = name(&app, &target);
        app.framework_opacity = true;
        let mut h = crate::test_window::open(app, vec2(1300.0, 850.0));
        let ctx = h.ctx.clone();
        h.state_mut().app.install_framework(&ctx);
        assert!(h.state().app.framework_installed);
        h.state_mut().app.run("window.panel", json!({"panel": "preflight"})).unwrap();
        fn step(h: &mut Harness<'_, crate::test_window::Window>, event: Option<Event>) {
            let mut raw = std::mem::take(h.input_mut());
            if let Some(event) = event {
                raw.events.push(event);
            }
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
        for _ in 0..3 {
            step(&mut h, None);
        }
        assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::Button, &label).count(), 1);
        let before = quiet(&h.state().app);
        let journal = h.state().app.session.journal.len();
        let pos = h.get_by_role_and_label(egui::accesskit::Role::Button, &label).rect().center();
        // TEMPORARY (whole-app Preflight click diagnosis, remove once the red is understood).
        eprintln!(
            "PREFLIGHT-DEBUG test: label={label:?} pos={pos:?} layer_at={:?} screen={:?} flyout={:?}",
            h.ctx.layer_id_at(pos),
            h.ctx.content_rect(),
            h.state().app.ui.docking_flyout,
        );
        step(&mut h, Some(Event::PointerMoved(pos)));
        for pressed in [true, false] {
            eprintln!("PREFLIGHT-DEBUG test: pointer {}", if pressed { "pressed" } else { "released" });
            step(&mut h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
        }
        for _ in 0..3 {
            step(&mut h, None);
        }
        eprintln!(
            "PREFLIGHT-DEBUG test: after the click focused={:?} journal_tail={:?} selection={:?}",
            h.ctx.memory(|m| m.focused()),
            &h.state().app.session.journal[journal..],
            h.state().app.session.active().map(|st| st.selection.items.clone()),
        );
        let app = &h.state().app;
        assert_eq!(app.session.active().unwrap().selection.items, vec![ItemId(items[1])]);
        assert_eq!(crate::canvas::current_page(app), Some(1));
        assert_eq!(app.session.journal.len(), journal + 1);
        assert_eq!(app.session.journal.last(), Some(&("selection.set".to_owned(), json!({"ids": [items[1]]}))));
        assert_quiet(app, &before, Some(app.session.active().unwrap().uid));
        let journal = app.session.journal.clone();
        for _ in 0..3 {
            step(&mut h, None);
        }
        assert_eq!(h.state().app.session.journal, journal);
    }
}
// PREFLIGHT_TESTS_END
