use super::{DesignApp, Sense, Tokens, Value, divider, json, sel_info, units, vec2};
use crate::app::file_dialogs::Bound;
use crate::icons;
use crate::widgets::{self, icon_toggle};
use egui::{Rect, Stroke, pos2};

/// Linked stories (Place and Link), above the links: each with its source story, and Update when
/// it is out of date.
fn linked_stories(app: &mut DesignApp, ui: &mut egui::Ui) {
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
                    if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Update"))).clicked() {
                        let _ = app.run("story.updateLink", json!({"story": l["story"]}));
                    }
                }
            });
        }
        ui.separator();
    }
}

/// A Links row's status: nothing when the link is up to date, as in InDesign; otherwise its icon,
/// named, with why a refused file was refused (or the status) on hover.
fn link_status_cell(ui: &mut egui::Ui, r: &Value, status_r: Rect, id: u64, lang: &str, t: &Tokens) {
    let status = r["status"].as_str().unwrap_or("");
    let status_label = match status {
        "missing" => "Missing",
        "modified" => "Modified",
        "embedded" => "Embedded",
        "tooLarge" => "Too large to check",
        _ => return,
    };
    let sr = ui.interact(status_r, ui.scope_id().with(("link_status", id)), Sense::hover());
    sr.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, crate::i18n::tr(lang, status_label)));
    paint_link_status(ui.painter(), status_r, status, t);
    let why = r["refused"].as_str().map(str::to_owned);
    let tip = crate::i18n::tr(lang, status_label).to_string();
    sr.on_hover_ui(|ui| {
        crate::rtl::label(ui, why.unwrap_or(tip));
    });
}

/// Links: status (missing / modified / embedded), page and effective resolution of every placed
/// graphic and linked text file; Relink, Go To Link, Update Link, Embed Link for the chosen row.
pub fn links_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    linked_stories(app, ui);
    let Some(st) = app.session.active() else { return };
    let t = Tokens::get(ui.ctx());
    // The list checks files on disk: refresh it at most twice a second.
    let key = egui::Id::unique("links_panel");
    let now = ui.input(|i| i.time);
    let rev = (st.uid, st.revision);
    let doc = st.doc.clone();
    let cached: Option<(f64, (u64, u64), Vec<Value>)> = ui.data(|d| d.get_temp(key));
    let rows = match cached {
        Some((at, r, rows)) if r == rev && now - at < 0.5 => rows,
        _ => {
            let rows = app.session.execute("links.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
            ui.data_mut(|d| d.insert_temp(key, (now, rev, rows.clone())));
            rows
        }
    };
    if rows.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No placed graphics.")).color(t.text_dim));
        return;
    }
    let lang = app.ui.language.clone();
    let sel_key = egui::Id::unique("links_panel_sel");
    let mut chosen: Option<u64> = ui.data(|d| d.get_temp(sel_key));
    // Asset and story ids come from one counter: a row is chosen by either.
    let row_id = |r: &Value| r["asset"].as_u64().or_else(|| r["story"].as_u64()).unwrap_or(0);
    if chosen.is_some_and(|c| !rows.iter().any(|r| row_id(r) == c)) {
        chosen = None;
    }
    let w = ui.available_width();
    // InDesign's columns: the name, the status and the page, at the right.
    let page_col = 34.0;
    let status_col = 24.0;
    let (hdr, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
    ui.painter().text(
        hdr.left_center() + vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        crate::i18n::tr(&lang, "Name"),
        egui::FontId::proportional(11.5),
        t.text,
    );
    let status_x = hdr.right() - page_col - status_col;
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(status_x + status_col / 2.0, hdr.center().y), vec2(14.0, 14.0)),
        "link-status",
        t.text_dim,
    );
    icons::paint(ui.painter(), Rect::from_center_size(pos2(hdr.right() - page_col / 2.0, hdr.center().y), vec2(14.0, 14.0)), "link-page", t.text_dim);
    ui.painter().line_segment([hdr.left_bottom(), hdr.right_bottom()], Stroke::new(1.0, t.section_divider));
    let mut go_to = None;
    for r in &rows {
        let id = row_id(r);
        let name = r["name"].as_str().unwrap_or("");
        let path = r["path"].as_str().unwrap_or("");
        // Same-named sources show their folder beside the name, so they can be told apart.
        let same_name = rows.iter().filter(|o| o["name"].as_str() == Some(name)).count() > 1;
        let folder = std::path::Path::new(path).parent().and_then(|p| p.file_name()).map(|f| f.to_string_lossy().to_string());
        let label = match folder.filter(|_| same_name && !path.is_empty()) {
            Some(f) => format!("{name} — {f}"),
            None => name.to_string(),
        };
        let (row, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, chosen == Some(id), &label));
        if chosen == Some(id) {
            ui.painter().rect_filled(row, 0.0, t.row_selected);
        } else if resp.hovered() {
            ui.painter().rect_filled(row, 0.0, t.hover);
        }
        // The graphic's thumbnail, as InDesign's rows show it.
        let thumb = Rect::from_min_size(row.min + vec2(4.0, 2.0), vec2(18.0, 18.0));
        match r["asset"].as_u64().and_then(|aid| link_thumb(ui.ctx(), &doc, aid)) {
            Some(tex) => {
                let [tw, th] = tex.size().map(|v| v as f32);
                let k = (thumb.width() / tw).min(thumb.height() / th);
                let r = Rect::from_center_size(thumb.center(), vec2(tw * k, th * k));
                ui.painter().image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
            None => icons::paint(ui.painter(), thumb, if r["story"].is_u64() { "link-text" } else { "link-page" }, t.text_dim),
        }
        let name_w = (status_x - thumb.right() - 10.0).max(20.0);
        let font = egui::FontId::proportional(12.0);
        let shown = middle_ellipsis(ui.painter(), &label, &font, name_w);
        ui.painter().text(pos2(thumb.right() + 6.0, row.center().y), egui::Align2::LEFT_CENTER, shown, font, t.text);
        let status_r = Rect::from_center_size(pos2(status_x + status_col / 2.0, row.center().y), vec2(14.0, 14.0));
        link_status_cell(ui, r, status_r, id, &lang, &t);
        // The page, underlined: a click goes to the graphic.
        let uses = r["uses"].as_array().cloned().unwrap_or_default();
        if let Some(u) = uses.first() {
            let page = u["page"].as_str().unwrap_or("PB").to_string();
            let pr = Rect::from_min_max(pos2(row.right() - page_col, row.min.y), row.max);
            let presp = ui.interact(pr, ui.scope_id().with(("link_page", id)), Sense::click());
            let col = egui::Color32::from_rgb(0xe8, 0xa0, 0x3a);
            let g = ui.painter().layout_no_wrap(page.clone(), egui::FontId::proportional(11.5), col);
            let at = pos2(pr.center().x - g.size().x / 2.0, pr.center().y - g.size().y / 2.0);
            ui.painter().line_segment([pos2(at.x, at.y + g.size().y), pos2(at.x + g.size().x, at.y + g.size().y)], Stroke::new(1.0, col));
            ui.painter().galley(at, g, col);
            if presp.clicked() {
                chosen = Some(id);
                go_to = Some(id);
            }
        }
        if resp.clicked() {
            chosen = Some(id);
        }
        if resp.double_clicked() {
            go_to = Some(id);
        }
    }
    ui.data_mut(|d| match chosen {
        Some(c) => {
            d.insert_temp(sel_key, c);
        }
        None => d.remove::<u64>(sel_key),
    });
    let target_of = |aid: u64| if rows.iter().any(|r| r["story"].as_u64() == Some(aid)) { json!({"story": aid}) } else { json!({"asset": aid}) };
    if let Some(aid) = go_to {
        let _ = app.run("links.goTo", target_of(aid));
    }
    // InDesign's footer: how many are selected, then Relink, Go To Link and Update Link; the
    // panel menu's Embed Link, Relink to Folder and Update All Links behind the last button.
    ui.add_space(2.0);
    let (bar, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
    ui.painter().line_segment([bar.left_top(), bar.right_top()], Stroke::new(1.0, t.section_divider));
    let count = crate::i18n::tr(&lang, "{count} Selected").replace("{count}", &usize::from(chosen.is_some()).to_string());
    let count_r = Rect::from_min_max(bar.min, pos2(bar.min.x + 90.0, bar.max.y));
    ui.interact(count_r, ui.scope_id().with("links_count"), Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &count));
    ui.painter().text(bar.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, &count, egui::FontId::proportional(11.5), t.text_dim);
    let mut action = None;
    let mut more = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(4.0, 2.0))).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let more_resp = icon_toggle(ui, "link-more", false, crate::i18n::tr(&lang, "More Link Options"));
        egui::Popup::menu(&more_resp).show(|ui| {
            let is_text = chosen.is_some_and(|c| rows.iter().any(|r| r["story"].as_u64() == Some(c)));
            if chosen.is_some() && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, if is_text { "Unlink" } else { "Embed Link" }))).clicked()
            {
                more = Some("embed");
                ui.close();
            }
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Relink to Folder…"))).clicked() {
                more = Some("folder");
                ui.close();
            }
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Update All Links"))).clicked() {
                more = Some("updateAll");
                ui.close();
            }
        });
        ui.add_enabled_ui(chosen.is_some(), |ui| {
            for (icon, what, tip) in
                [("link-update", "update", "Update Link"), ("link-goto", "goTo", "Go To Link"), ("link-relink", "relink", "Relink…")]
            {
                if icon_toggle(ui, icon, false, crate::i18n::tr(&lang, tip)).clicked() {
                    action = Some(what);
                }
            }
        });
    });
    if let (Some(what), Some(aid)) = (action, chosen) {
        let target = target_of(aid);
        match what {
            "relink" => {
                // As InDesign's Search for Missing Links in This Folder: the other missing links
                // are looked for in the folder the new file came from. Only placed graphics:
                // `links.relinkFolder` doesn't look for linked text files.
                let others_missing = rows.iter().any(|r| row_id(r) != aid && r["asset"].is_u64() && r["status"] == "missing");
                let asked = app.ask_open("relink", Bound::Document, move |app, path| {
                    let mut p = target;
                    p["path"] = json!(path);
                    if app.run("links.relink", p).is_ok() && others_missing {
                        relink_missing_from_folder(app, &path);
                    }
                    Ok(Value::Null)
                });
                if let Err(e) = asked {
                    app.status_error("Relink", e);
                }
            }
            "goTo" => {
                let _ = app.run("links.goTo", target);
            }
            _ => update_links(app, target),
        }
    }
    match more {
        Some("embed") => {
            if let Some(aid) = chosen {
                let _ = app.run("links.embed", target_of(aid));
            }
        }
        Some("folder") => {
            // Choose any file in the folder: missing links are looked up there by name.
            let asked = app.ask_open("relink", Bound::Document, |app, path| {
                let Some(dir) = std::path::Path::new(&path).parent() else { return Ok(Value::Null) };
                if let Ok(v) = app.run("links.relinkFolder", json!({"dir": dir.to_string_lossy()})) {
                    let n = v["relinked"].as_u64().unwrap_or(0);
                    let message = crate::i18n::tr(&app.ui.language, "Searched this relink directory, and found and relinked {count} missing links.");
                    app.status(message.replace("{count}", &n.to_string()));
                }
                Ok(Value::Null)
            });
            if let Err(e) = asked {
                app.status_error("Relink", e);
            }
        }
        Some("updateAll") => update_links(app, json!({})),
        _ => {}
    }
    // Link Info: the chosen link's details, with ◀ ▶ to step through the links.
    ui.add_space(4.0);
    let mut step: i64 = 0;
    ui.horizontal(|ui| {
        widgets::caption(ui, crate::i18n::tr(&lang, "Link Info"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (dir, tip) in [(1i64, "Next Link"), (-1, "Previous Link")] {
                let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                let tip = crate::i18n::tr(&lang, tip);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
                if resp.hovered() {
                    ui.painter().rect_filled(r, 2.0, t.hover);
                }
                let c = r.center();
                let d = dir as f32 * 3.5;
                ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(d, 0.0), c + vec2(-d, -4.0), c + vec2(-d, 4.0)], t.icon, Stroke::NONE));
                if resp.on_hover_text(tip).clicked() {
                    step = dir;
                }
            }
        });
    });
    if step != 0 && !rows.is_empty() {
        let n = rows.len() as i64;
        let at = chosen.and_then(|c| rows.iter().position(|r| row_id(r) == c)).map_or(if step > 0 { -1 } else { n }, |i| i as i64);
        let next = (at + step).rem_euclid(n) as usize;
        if let Some(r) = rows.get(next) {
            ui.data_mut(|d| d.insert_temp(sel_key, row_id(r)));
            ui.ctx().request_repaint();
        }
    }
    let Some(r) = chosen.and_then(|c| rows.iter().find(|r| row_id(r) == c)) else {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Choose a link.")).color(t.text_dim).size(11.0));
        return;
    };
    let uses = r["uses"].as_array().cloned().unwrap_or_default();
    let status = match r["status"].as_str().unwrap_or("") {
        "missing" => "Missing",
        "modified" => "Modified",
        "embedded" => "Embedded",
        "tooLarge" => "Too large to check",
        _ => "OK",
    };
    let mut fields: Vec<(&str, String)> = vec![
        ("Name", r["name"].as_str().unwrap_or("").to_string()),
        ("Status", crate::i18n::tr(&lang, status).to_string()),
        ("Page", uses.first().and_then(|u| u["page"].as_str()).unwrap_or("PB").to_string()),
    ];
    if let Some(m) = r["mime"].as_str() {
        fields.push(("Format", link_format(m)));
    }
    if let Some(b) = r["bytes"].as_u64().filter(|b| *b > 0) {
        fields.push(("Size", file_size(b)));
    }
    if let Some((pw, ph)) = r["pixels"].as_array().and_then(|a| Some((a.first()?.as_u64()?, a.get(1)?.as_u64()?))) {
        fields.push(("Dimensions", format!("{pw} × {ph}")));
    }
    if let Some(p) = uses.first().and_then(|u| u["ppi"].as_f64()) {
        fields.push(("Effective PPI", format!("{p:.0}")));
    }
    egui::Grid::new("link_info").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
        for (k, v) in fields {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::rtl::label(ui, egui::RichText::new(format!("{}:", crate::i18n::tr(&lang, k))).strong().size(11.0));
            });
            ui.add(egui::Label::new(egui::RichText::new(v).size(11.0)).truncate());
            ui.end_row();
        }
    });
    // The path in full, wrapping inside the panel (it tells same-named sources apart).
    if let Some(path) = r["path"].as_str().filter(|p| !p.is_empty()) {
        ui.horizontal_wrapped(|ui| {
            crate::rtl::label(ui, egui::RichText::new(format!("{}:", crate::i18n::tr(&lang, "Path"))).strong().size(11.0));
            ui.add(egui::Label::new(egui::RichText::new(path).color(t.text_dim).size(11.0)).wrap());
        });
    }
}

/// After a Relink, look for the other missing links, by name, in the folder the new file came
/// from, and say how many were found, as InDesign does with Search for Missing Links in This
/// Folder.
fn relink_missing_from_folder(app: &mut DesignApp, picked: &str) {
    let Some(dir) = std::path::Path::new(picked).parent() else { return };
    let Ok(v) = app.run("links.relinkFolder", json!({"dir": dir.to_string_lossy()})) else { return };
    let n = v["relinked"].as_u64().unwrap_or(0);
    let message = crate::i18n::tr(&app.ui.language, "Searched this relink directory, and found and relinked {count} missing links.")
        .replace("{count}", &n.to_string());
    app.ui.dialog = Some(crate::dialogs::Dialog::new("alert", json!({"title": "Information", "message": message})));
}

/// `text` shortened in the middle to fit `max` points, as InDesign's Links panel shows long
/// names ("AdobeSt…108372417.jpg"): the start and the extension stay readable.
pub(super) fn middle_ellipsis(painter: &egui::Painter, text: &str, font: &egui::FontId, max: f32) -> String {
    let width = |s: &str| painter.layout_no_wrap(s.to_string(), font.clone(), egui::Color32::WHITE).size().x;
    if width(text) <= max {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len();
    while keep > 1 {
        keep -= 1;
        let head = keep.div_ceil(2);
        let tail = keep / 2;
        let s: String = chars.iter().take(head).chain(std::iter::once(&'…')).chain(chars.iter().skip(chars.len() - tail)).collect();
        if width(&s) <= max {
            return s;
        }
    }
    "…".to_string()
}

/// A linked graphic's format, as the Links panel's Link Info names it.
pub(super) fn link_format(mime: &str) -> String {
    match mime {
        "image/jpeg" => "JPEG".into(),
        "image/png" => "PNG".into(),
        "image/tiff" => "TIFF".into(),
        "image/gif" => "GIF".into(),
        "image/webp" => "WebP".into(),
        "image/svg+xml" => "SVG".into(),
        "application/pdf" => "PDF".into(),
        "application/postscript" | "image/x-eps" | "application/eps" => "EPS".into(),
        "image/vnd.adobe.photoshop" => "Photoshop".into(),
        other => other.rsplit('/').next().unwrap_or(other).to_uppercase(),
    }
}

/// A file size as the Links panel shows it: KB below a megabyte, MB above.
pub(super) fn file_size(bytes: u64) -> String {
    if bytes < 1024 * 1024 { format!("{} KB", bytes.div_ceil(1024)) } else { format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)) }
}

/// The status column's glyph: a red stop for a missing file, a yellow warning for a modified or
/// unchecked one, a small picture for an embedded one. Drawn in code.
fn paint_link_status(p: &egui::Painter, r: Rect, status: &str, t: &Tokens) {
    let c = r.center();
    match status {
        "missing" => {
            p.circle_filled(c, 6.5, egui::Color32::from_rgb(214, 38, 38));
            p.text(c, egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(10.0), egui::Color32::WHITE);
        }
        "modified" | "tooLarge" => {
            let tri = vec![c + vec2(0.0, -6.0), c + vec2(6.5, 5.0), c + vec2(-6.5, 5.0)];
            p.add(egui::Shape::convex_polygon(tri, egui::Color32::from_rgb(250, 200, 30), Stroke::NONE));
            p.text(c + vec2(0.0, 1.0), egui::Align2::CENTER_CENTER, "!", egui::FontId::proportional(9.0), egui::Color32::BLACK);
        }
        _ => icons::paint(p, r, "link-embedded", t.text_dim),
    }
}

/// A linked graphic's thumbnail for the Links panel, cached per image data. Data that doesn't
/// decode is cached too (as none), so its row doesn't decode it again every frame.
///
/// The key is the data's address; the entry holds a weak reference to that data, which keeps
/// the address from being reused while the entry lives, and an entry whose data is gone (the
/// link relinked or updated, the document closed) is stale.
pub(super) fn link_thumb(ctx: &egui::Context, doc: &designcraft_doc::Document, aid: u64) -> Option<egui::TextureHandle> {
    let a = doc.assets.get(&designcraft_doc::AssetId(aid))?;
    let key = link_thumb_key(a);
    if let Some((data, t)) = ctx.data(|d| d.get_temp::<LinkThumb>(key))
        && data.upgrade().is_some_and(|data| std::sync::Arc::ptr_eq(&data, &a.data))
    {
        return t;
    }
    let w = a.pixels.map_or(256.0, |(w, _)| f64::from(w).max(1.0));
    let t = designcraft_render::images::mip(&a.data, a.page, w, 36.0 / w).filter(|pm| pm.width() > 0 && pm.height() > 0).map(|pm| {
        let ci = egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data_as_u8_slice());
        let max = ctx.input(|i| i.max_texture_side).min(1024);
        ctx.load_texture("link_thumb", crate::widgets::fit_texture(ci, max), egui::TextureOptions::LINEAR)
    });
    ctx.data_mut(|d| d.insert_temp::<LinkThumb>(key, (std::sync::Arc::downgrade(&a.data), t.clone())));
    t
}

/// A cached Links panel thumbnail: the image data it was made from, and the texture (none when
/// the data didn't decode).
pub(super) type LinkThumb = (std::sync::Weak<Vec<u8>>, Option<egui::TextureHandle>);

/// The cache key of an asset's Links panel thumbnail: its data and page.
pub(super) fn link_thumb_key(a: &designcraft_doc::Asset) -> egui::Id {
    egui::Id::unique(("link_thumb", std::sync::Arc::as_ptr(&a.data) as usize, a.data.len(), a.page))
}

/// Update Link from the Links panel or the menu (`{asset}`, `{story}`, or `{}` for all): asks
/// first when a linked text story edited in the layout would lose those edits, as InDesign's
/// Links panel does. The `links.update` command itself doesn't ask (as in InDesign scripting).
pub fn update_links(app: &mut DesignApp, target: Value) {
    let rows = app.session.execute("links.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let one = target.get("story").and_then(Value::as_u64);
    let all = one.is_none() && target.get("asset").is_none();
    let edited: Vec<&str> = rows
        .iter()
        .filter(|r| (all || (one.is_some() && r["story"].as_u64() == one)) && r["edited"] == true && r["status"] == "modified")
        .filter_map(|r| r["name"].as_str())
        .collect();
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
    let u = units(app);
    let t = Tokens::get(ui.ctx());
    match sel_info(app) {
        Some(i) => {
            let f = |v: f64| designcraft_geom::format_measure(v, u);
            crate::rtl::label(ui, format!("X: {}   Y: {}", f(i.page_rect.x0), f(i.page_rect.y0)));
            crate::rtl::label(ui, format!("W: {}   H: {}", f(i.page_rect.width()), f(i.page_rect.height())));
            crate::rtl::label(
                ui,
                format!("{}: {}   {}: {}", crate::i18n::tr(&app.ui.language, "Fill"), i.fill, crate::i18n::tr(&app.ui.language, "Stroke"), i.stroke),
            );
        }
        None => {
            if let Some(st) = app.session.active() {
                crate::rtl::label(
                    ui,
                    format!(
                        "{} · {} · {}",
                        crate::i18n::count_label(&app.ui.language, "pages", st.doc.page_count()),
                        crate::i18n::count_label(&app.ui.language, "stories", st.doc.stories.len()),
                        crate::i18n::count_label(&app.ui.language, "items", st.doc.all_items().len())
                    ),
                );
            }
        }
    }
    crate::rtl::label(
        ui,
        egui::RichText::new(format!("{} {:.1} ms", crate::i18n::tr(&app.ui.language, "Render"), app.perf.render_ms)).color(t.text_dim),
    );
}

pub fn preflight_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let issues = designcraft_engine::cmd::preflight::check(&app.session, 150.0);
    let errors = issues.iter().filter(|i| i.severity == "error").count();
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(
            r.center(),
            4.5,
            if errors == 0 { egui::Color32::from_rgb(60, 200, 90) } else { egui::Color32::from_rgb(235, 50, 50) },
        );
        crate::rtl::label(
            ui,
            if issues.is_empty() {
                crate::i18n::tr(&app.ui.language, "No errors").to_string()
            } else {
                format!(
                    "{}, {}",
                    crate::i18n::count_label(&app.ui.language, "errors", errors),
                    crate::i18n::count_label(&app.ui.language, "warnings", issues.len() - errors)
                )
            },
        );
    });
    divider(ui);
    for i in issues {
        let page = i.page.filter(|_| app.session.active().is_some()).map(|p| app.session.page_label(p));
        let text = format!("{}  {}", if i.severity == "error" { "⛔" } else { "⚠" }, i.message);
        let resp = ui.add(egui::Button::new(egui::RichText::new(text).size(11.0)).frame(false));
        if let Some(pg) = &page {
            crate::rtl::label(ui, egui::RichText::new(format!("   page {pg}")).size(10.0).color(t.text_dim));
        }
        if resp.clicked() {
            if let Some(id) = i.item {
                let _ = app.run("selection.set", json!({"ids": [id]}));
            }
            if let Some(p) = i.page {
                crate::canvas::go_to_page(app, p);
            }
        }
    }
}
