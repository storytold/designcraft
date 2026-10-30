use super::{DesignApp, Rect, Sense, Stroke, Tokens, Value, divider, json, pos2, sel_info, units, vec2, widgets};
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
    let lang = app.ui.language.clone();
    // On and the profile, as InDesign's top row (one built-in profile).
    ui.horizontal(|ui| {
        let mut on = !app.ui.preflight_off;
        if widgets::checkbox(ui, &mut on, crate::i18n::tr(&lang, "On")).clicked() {
            app.ui.preflight_off = !on;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "[Basic] (working)")).size(11.5));
            widgets::caption(ui, crate::i18n::tr(&lang, "Profile:"));
        });
    });
    let issues = if app.ui.preflight_off { Vec::new() } else { designcraft_engine::cmd::preflight::check(&app.session, 150.0) };
    let errors = issues.iter().filter(|i| i.severity == "error").count();
    let w = ui.available_width();
    let page_col = 40.0;
    let (hdr, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
    ui.painter().text(
        hdr.left_center() + vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        crate::i18n::tr(&lang, "Error"),
        egui::FontId::proportional(11.5),
        t.text,
    );
    ui.painter().text(
        pos2(hdr.right() - page_col + 4.0, hdr.center().y),
        egui::Align2::LEFT_CENTER,
        crate::i18n::tr(&lang, "Page"),
        egui::FontId::proportional(11.5),
        t.text,
    );
    ui.painter().line_segment([hdr.left_bottom(), hdr.right_bottom()], Stroke::new(1.0, t.section_divider));
    // The issues as InDesign's tree: category, then rule, then each object.
    let mut tree: Vec<(&'static str, Vec<(&'static str, Vec<usize>)>)> = Vec::new();
    for (k, i) in issues.iter().enumerate() {
        let (cat, rule) = preflight_rule(i.kind);
        let c = match tree.iter().position(|(n, _)| *n == cat) {
            Some(c) => c,
            None => {
                tree.push((cat, Vec::new()));
                tree.len() - 1
            }
        };
        let Some((_, rules)) = tree.get_mut(c) else { continue };
        match rules.iter_mut().find(|(n, _)| *n == rule) {
            Some((_, v)) => v.push(k),
            None => rules.push((rule, vec![k])),
        }
    }
    // The chosen issue by what it is (kind, object, message), not its row: the list is checked
    // again every frame, and an edit can remove the rows before it.
    let sel_key = egui::Id::unique("preflight_sel");
    let identity = |i: &designcraft_engine::cmd::preflight::Issue| (i.kind.to_string(), i.item, i.message.clone());
    let mut chosen: Option<usize> =
        ui.data(|d| d.get_temp::<(String, Option<u64>, String)>(sel_key)).and_then(|want| issues.iter().position(|i| identity(i) == want));
    let mut go: Option<usize> = None;
    let row = |ui: &mut egui::Ui, depth: f32, label: &str, open: Option<bool>, selected: bool| {
        let (r, resp) = ui.allocate_exact_size(vec2(w, 21.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
        if selected {
            ui.painter().rect_filled(r, 0.0, t.row_selected);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, t.hover);
        }
        let x = r.min.x + 8.0 + 16.0 * depth;
        if let Some(open) = open {
            let c = pos2(x + 4.0, r.center().y);
            let tri = if open {
                vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
            } else {
                vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
            };
            ui.painter().add(egui::Shape::convex_polygon(tri, t.icon, Stroke::NONE));
        }
        let text_w = (r.right() - page_col - x - 16.0).max(20.0);
        let shown = middle_ellipsis(ui.painter(), label, &egui::FontId::proportional(12.0), text_w);
        ui.painter().text(pos2(x + 14.0, r.center().y), egui::Align2::LEFT_CENTER, shown, egui::FontId::proportional(12.0), t.text);
        (r, resp)
    };
    egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
        for (cat, rules) in &tree {
            let n: usize = rules.iter().map(|(_, v)| v.len()).sum();
            let cat_id = egui::Id::unique(("preflight_open", *cat));
            let cat_open = ui.data(|d| d.get_temp::<bool>(cat_id)).unwrap_or(true);
            let (_, resp) = row(ui, 0.0, &format!("{} ({n})", crate::i18n::tr(&lang, cat)), Some(cat_open), false);
            if resp.clicked() {
                ui.data_mut(|d| d.insert_temp(cat_id, !cat_open));
            }
            if !cat_open {
                continue;
            }
            for (rule, ks) in rules {
                let rule_id = egui::Id::unique(("preflight_open", *cat, *rule));
                let rule_open = ui.data(|d| d.get_temp::<bool>(rule_id)).unwrap_or(true);
                let (_, resp) = row(ui, 1.0, &format!("{} ({})", crate::i18n::tr(&lang, rule), ks.len()), Some(rule_open), false);
                if resp.clicked() {
                    ui.data_mut(|d| d.insert_temp(rule_id, !rule_open));
                }
                if !rule_open {
                    continue;
                }
                for &k in ks {
                    let Some(i) = issues.get(k) else { continue };
                    let (r, resp) = row(ui, 2.0, &preflight_label(i), None, chosen == Some(k));
                    if resp.clicked() {
                        chosen = Some(k);
                    }
                    if resp.double_clicked() {
                        go = Some(k);
                    }
                    // The page, in InDesign's orange: a click goes to the object.
                    if let Some(p) = i.page.filter(|_| app.session.active().is_some()) {
                        let label = app.session.page_label(p);
                        let pr = Rect::from_min_max(pos2(r.right() - page_col, r.min.y), r.max);
                        let presp = ui.interact(pr, ui.scope_id().with(("preflight_page", k)), Sense::click());
                        presp.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} {label}", crate::i18n::tr(&lang, "Page")))
                        });
                        ui.painter().text(
                            pr.left_center() + vec2(4.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            &label,
                            egui::FontId::proportional(11.5),
                            egui::Color32::from_rgb(0xe8, 0xa0, 0x3a),
                        );
                        if presp.clicked() {
                            chosen = Some(k);
                            go = Some(k);
                        }
                    }
                }
            }
        }
    });
    if let Some(i) = chosen.and_then(|k| issues.get(k)) {
        ui.data_mut(|d| d.insert_temp(sel_key, identity(i)));
    }
    if let Some(i) = go.and_then(|k| issues.get(k)) {
        if let Some(id) = i.item {
            let _ = app.run("selection.set", json!({"ids": [id]}));
        }
        if let Some(p) = i.page {
            crate::canvas::go_to_page(app, p);
        }
    }
    // Info: what the problem is and how to fix it, for the chosen row.
    divider(ui);
    widgets::caption(ui, crate::i18n::tr(&lang, "Info"));
    if let Some(i) = chosen.and_then(|k| issues.get(k)) {
        let (problem, fix) = preflight_help(i.kind);
        for (k, v) in [("Problem:", crate::i18n::tr(&lang, problem).to_string()), ("Fix:", crate::i18n::tr(&lang, fix).to_string())] {
            ui.horizontal_wrapped(|ui| {
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, k)).strong().size(11.0));
                ui.add(egui::Label::new(egui::RichText::new(v).size(11.0)).wrap());
            });
        }
        // The message in full (a path can be long): under its caption, wrapping in the panel.
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Details:")).strong().size(11.0));
        ui.add(egui::Label::new(egui::RichText::new(&i.message).size(11.0).color(t.text_dim)).wrap());
    }
    // The count and the pages checked, as InDesign's footer.
    divider(ui);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
        let col = if app.ui.preflight_off {
            egui::Color32::from_gray(140)
        } else if errors == 0 {
            egui::Color32::from_rgb(60, 200, 90)
        } else {
            egui::Color32::from_rgb(235, 50, 50)
        };
        ui.painter().circle_filled(r.center(), 4.5, col);
        let text = if app.ui.preflight_off {
            crate::i18n::tr(&lang, "Preflight off").to_string()
        } else if issues.is_empty() {
            crate::i18n::tr(&lang, "No errors").to_string()
        } else {
            let warnings = issues.len() - errors;
            let e = crate::i18n::count_label(&lang, "errors", errors);
            if warnings == 0 { e } else { format!("{e}, {}", crate::i18n::count_label(&lang, "warnings", warnings)) }
        };
        crate::rtl::label(ui, text);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "All")).size(11.0));
            widgets::caption(ui, crate::i18n::tr(&lang, "Pages:"));
        });
    });
}

/// `text` shortened in the middle to fit `max` points, keeping its start and its end.
fn middle_ellipsis(painter: &egui::Painter, text: &str, font: &egui::FontId, max: f32) -> String {
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

/// The category and rule a preflight issue is listed under, as InDesign's [Basic] profile
/// groups them.
fn preflight_rule(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "missingLink" => ("LINKS", "Missing link"),
        "lowResolution" => ("IMAGES and OBJECTS", "Image resolution"),
        "rgbColor" => ("COLOR", "RGB swatch"),
        "overset" => ("TEXT", "Overset text"),
        "unplacedStory" => ("TEXT", "Unplaced text"),
        "missingFont" => ("TEXT", "Font missing"),
        "missingFontStyle" => ("TEXT", "Font style missing"),
        "missingGlyphs" => ("TEXT", "Missing glyph"),
        "emptyText" => ("TEXT", "Empty text frame"),
        "unsupportedTypography" => ("TEXT", "Unsupported typography"),
        _ => ("DOCUMENT", "Other"),
    }
}

/// A preflight issue's row: the object it's about, by name where the message has one (a
/// file, a font, a swatch), as InDesign lists them.
fn preflight_label(i: &designcraft_engine::cmd::preflight::Issue) -> String {
    let m = i.message.as_str();
    let after = |prefix: &str| m.strip_prefix(prefix).map(str::trim);
    let file = |p: &str| std::path::Path::new(p).file_name().map_or_else(|| p.to_string(), |f| f.to_string_lossy().to_string());
    match i.kind {
        "missingLink" => after("Missing link:").map(file).unwrap_or_else(|| m.to_string()),
        "lowResolution" => m.split(':').next().unwrap_or(m).to_string(),
        "rgbColor" => m.rsplit(": ").next().unwrap_or(m).to_string(),
        "missingFont" => after("Missing font:").unwrap_or(m).to_string(),
        "missingFontStyle" => after("Missing font style:").map(|r| r.split(" (").next().unwrap_or(r).to_string()).unwrap_or_else(|| m.to_string()),
        "missingGlyphs" => after("Missing glyphs in").map(|r| r.split(':').next().unwrap_or(r).to_string()).unwrap_or_else(|| m.to_string()),
        "overset" | "emptyText" => "Text Frame".to_string(),
        _ => m.to_string(),
    }
}

/// Preflight panel › Info: what a rule's problem is and how to fix it.
fn preflight_help(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "missingLink" => ("The linked file can't be found.", "Relink it in the Links panel."),
        "lowResolution" => ("The image shows at a lower resolution than the profile asks for.", "Place a sharper image, or show this one smaller."),
        "rgbColor" => ("An RGB swatch in a document meant for print.", "Change the swatch to CMYK in Swatch Options."),
        "overset" => ("The text doesn't fit in its frames.", "Enlarge the frame, thread it to another one, or shorten the text."),
        "unplacedStory" => ("A story isn't in any text frame.", "Place it in a frame, or delete it."),
        "missingFont" | "missingFontStyle" => ("The font isn't available.", "Install it, or choose another font in Type › Find Font."),
        "missingGlyphs" => ("Some characters have no glyph in their font.", "Choose a font that has them."),
        "emptyText" => ("A text frame holds no text.", "Add text, or delete the frame."),
        _ => ("DesignCraft can't set some of this text yet.", "Check how it looks in Preview."),
    }
}
