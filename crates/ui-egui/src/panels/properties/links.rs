use super::{DesignApp, Sense, Tokens, Value, divider, json, sel_info, units, vec2};
use crate::app::file_dialogs::Bound;

/// Links: status (missing / modified / embedded), page and effective resolution of every placed
/// graphic and linked text file; Relink, Go To Link, Update Link, Embed Link for the chosen row.
pub fn links_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
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
                    if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Update"))).clicked() {
                        let _ = app.run("story.updateLink", json!({"story": l["story"]}));
                    }
                }
            });
        }
        ui.separator();
    }
    let Some(st) = app.session.active() else { return };
    let t = Tokens::get(ui.ctx());
    // The list checks files on disk: refresh it at most twice a second.
    let key = egui::Id::unique("links_panel");
    let now = ui.input(|i| i.time);
    let rev = (st.uid, st.revision);
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
    let sel_key = egui::Id::unique("links_panel_sel");
    let mut chosen: Option<u64> = ui.data(|d| d.get_temp(sel_key));
    // Leave room for Status and Page/PPI instead of widening the grid for long source paths.
    let name_width = (ui.available_width() - 160.0).max(50.0);
    egui::Grid::new("links_grid").num_columns(3).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Name")).color(t.text_dim).size(11.0));
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Status")).color(t.text_dim).size(11.0));
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page · PPI")).color(t.text_dim).size(11.0));
        ui.end_row();
        for r in &rows {
            // Asset and story ids come from one counter: a row is chosen by either.
            let aid = r["asset"].as_u64().or_else(|| r["story"].as_u64()).unwrap_or(0);
            let name = r["name"].as_str().unwrap_or("");
            let response = ui
                .vertical(|ui| {
                    ui.set_width(name_width);
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
                .inner;
            if response.clicked() {
                chosen = Some(aid);
            }
            let (txt, col) = match r["status"].as_str().unwrap_or("") {
                "missing" => ("\u{26A0} Missing", egui::Color32::from_rgb(0xe5, 0x4b, 0x4b)),
                "modified" => ("\u{25B2} Modified", egui::Color32::from_rgb(0xe8, 0xb3, 0x2c)),
                "embedded" => ("Embedded", t.text_dim),
                "tooLarge" => ("Too large to check", egui::Color32::from_rgb(0xe8, 0xb3, 0x2c)),
                _ => ("OK", t.text_dim),
            };
            let status = crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, txt)).color(col).size(11.0));
            // Why a link isn't followed (a path on another computer, a device).
            if let Some(why) = r["refused"].as_str() {
                status.on_hover_text(why);
            }
            let uses = r["uses"].as_array().cloned().unwrap_or_default();
            let info = match uses.first() {
                Some(u) => {
                    let page = u["page"].as_str().unwrap_or("PB");
                    let ppi = u["ppi"].as_f64().map(|p| format!(" · {p:.0} ppi")).unwrap_or_default();
                    let more = if uses.len() > 1 { format!(" (+{})", uses.len() - 1) } else { String::new() };
                    format!("{page}{ppi}{more}")
                }
                None => crate::i18n::tr(&app.ui.language, "unused").into(),
            };
            // Low effective resolution for print is flagged like the Preflight check.
            let low = uses.first().and_then(|u| u["ppi"].as_f64()).is_some_and(|p| p < 150.0);
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
    ui.horizontal_wrapped(|ui| {
        let Some(aid) = chosen else {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Choose a link.")).color(t.text_dim).size(11.0));
            return;
        };
        // A linked text file is addressed by its story.
        let is_text = rows.iter().any(|r| r["story"].as_u64() == Some(aid));
        let target = if is_text { json!({"story": aid}) } else { json!({"asset": aid}) };
        let with = |k: &str, v: Value| {
            let mut p = target.clone();
            p[k] = v;
            p
        };
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Relink…"))).clicked() {
            let target = with("path", Value::Null);
            let asked = app.ask_open("relink", Bound::Document, move |app, path| {
                let _ = app.run("links.relink", crate::app::file_dialogs::with_path(&target, "path", path));
                Ok(Value::Null)
            });
            if let Err(e) = asked {
                app.status_error("Relink", e);
            }
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Go To"))).clicked() {
            let _ = app.run("links.goTo", target.clone());
        }
        let edited = rows.iter().any(|r| r["story"].as_u64() == Some(aid) && r["edited"].as_bool() == Some(true));
        let update = ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Update")));
        let update = if edited {
            update.on_hover_ui(|ui| {
                crate::rtl::label(
                    ui,
                    crate::i18n::tr(&app.ui.language, "This story was edited in the layout: you'll be asked before those edits are replaced"),
                );
            })
        } else {
            update
        };
        if update.clicked() {
            update_links(app, target.clone());
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, if is_text { "Unlink" } else { "Embed" }))).clicked() {
            let _ = app.run("links.embed", target.clone());
        }
    });
    ui.horizontal(|ui| {
        // Choose any file in the folder: missing links are looked up there by name.
        if ui
            .button(crate::i18n::tr(&app.ui.language, "Relink to Folder…"))
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Pick a file in the folder holding the moved graphics"));
            })
            .clicked()
        {
            let asked = app.ask_open("relink", Bound::Document, |app, path| {
                let Some(dir) = std::path::Path::new(&path).parent() else { return Ok(Value::Null) };
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
