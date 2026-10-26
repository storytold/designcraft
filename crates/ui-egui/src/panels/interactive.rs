//! Hyperlinks and Bookmarks panels (Window › Interactive).

use serde_json::{Value, json};

use crate::DesignApp;
use crate::theme::Tokens;

fn dest_text(lang: &str, d: &Value) -> String {
    if let Some(u) = d.get("url").and_then(Value::as_str) {
        return u.to_string();
    }
    if let Some(e) = d.get("email").and_then(Value::as_str) {
        return format!("mailto:{e}");
    }
    if let Some(p) = d.get("page").and_then(Value::as_u64) {
        return format!("{} {}", crate::i18n::tr(lang, "Page"), p + 1);
    }
    d.to_string()
}

pub fn hyperlinks(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let id = egui::Id::unique("new_hyperlink_url");
    let mut url: String = ui.data(|d| d.get_temp(id)).unwrap_or_else(|| "https://".into());
    let has_target = app.session.active().is_some_and(|d| d.selection.text.is_some_and(|t| !t.is_caret()) || !d.selection.items.is_empty());
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut url).desired_width(170.0));
        if ui
            .add_enabled(has_target, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New"))))
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Hyperlink from the selected text or frames"));
            })
            .clicked()
        {
            let p = if let Some(m) = url.strip_prefix("mailto:") { json!({"email": m}) } else { json!({"url": url}) };
            if let Err(e) = app.run("hyperlink.create", p) {
                app.status_error("Hyperlinks", e);
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, url));
    ui.separator();
    let list = app.session.execute("hyperlink.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    if list.is_empty() {
        crate::rtl::label(
            ui,
            crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "No hyperlinks. Select text or a frame, type a URL and click New."))
                    .size(11.0)
                    .color(t.text_dim),
            ),
        );
    }
    for h in list {
        let hid = h["id"].clone();
        ui.horizontal(|ui| {
            let r = ui.selectable_label(false, h["name"].as_str().unwrap_or("")).on_hover_text(dest_text(&app.ui.language, &h["dest"]));
            if r.clicked() {
                let _ = app.run("hyperlink.goToSource", json!({"id": hid}));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete"))).clicked() {
                    let _ = app.run("hyperlink.delete", json!({"id": hid}));
                }
            });
        });
    }
}

pub fn bookmarks(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Bookmark for This Page"))).clicked() {
        let page = crate::canvas::current_page(app).unwrap_or(0) + 1;
        let _ = app.run("bookmark.add", json!({"page": page}));
    }
    ui.separator();
    let list = app.session.execute("bookmark.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    if list.is_empty() {
        crate::rtl::label(
            ui,
            crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "No bookmarks. They become the PDF outline.")).size(11.0).color(t.text_dim),
            ),
        );
    }
    for (i, b) in list.iter().enumerate() {
        let page = b["page"].as_u64().unwrap_or(0) as usize;
        ui.horizontal(|ui| {
            if ui
                .selectable_label(false, b["name"].as_str().unwrap_or(""))
                .on_hover_text(format!("{} {}", crate::i18n::tr(&app.ui.language, "Page"), app.session.page_label(page)))
                .clicked()
            {
                crate::canvas::go_to_page(app, page);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete"))).clicked() {
                    let _ = app.run("bookmark.delete", json!({"index": i}));
                }
            });
        });
    }
}

/// Articles panel: reading-order lists for EPUB / HTML export.
pub fn articles(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let has_sel = app.session.active().is_some_and(|d| !d.selection.items.is_empty());
    if ui.add_enabled(has_sel, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Article from Selection")))).clicked() {
        let _ = app.run("article.new", json!({}));
    }
    ui.separator();
    let list = app.session.execute("article.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    if list.is_empty() {
        crate::rtl::label(
            ui,
            crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "No articles: exports follow page order.")).size(11.0).color(t.text_dim),
            ),
        );
    }
    for a in list {
        let name = a["name"].as_str().unwrap_or("").to_string();
        let mut export = a["export"].as_bool().unwrap_or(true);
        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut export, "")
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Include when exporting"));
                })
                .changed()
            {
                let _ = app.run("article.options", json!({"name": name, "export": export}));
            }
            crate::rtl::label(ui, egui::RichText::new(&name).strong());
            crate::rtl::label(
                ui,
                egui::RichText::new(crate::i18n::count_label(&app.ui.language, "objects", a["items"].as_array().map_or(0, Vec::len)))
                    .size(10.5)
                    .color(t.text_dim),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete"))).clicked() {
                    let _ = app.run("article.delete", json!({"name": name}));
                }
                if ui
                    .add_enabled(has_sel, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Add"))).small())
                    .on_hover_ui(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Add the selection"));
                    })
                    .clicked()
                {
                    let _ = app.run("article.add", json!({"name": name}));
                }
            });
        });
    }
}

/// Tags panel and structure: tags (click to tag the selection), the structure in reading order,
/// XML export/import.
pub fn tags(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Ok(info) = app.session.execute("xml.tags", &json!({})) else { return };
    let has_sel = app.session.active().is_some_and(|d| !d.selection.items.is_empty());
    // Selected text is tagged inline instead of its frame.
    let text_sel = app.session.active().and_then(|d| d.selection.text).is_some_and(|t| !t.range().is_empty());
    let id = egui::Id::unique("new_tag_name");
    let mut name: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New tag")))
                .desired_width(140.0),
        );
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New"))).clicked() && !name.trim().is_empty() {
            match app.run("xml.newTag", json!({"name": name.trim()})) {
                Ok(_) => name.clear(),
                Err(e) => app.status_error("Tags", e),
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, name));
    for tag in info["tags"].as_array().cloned().unwrap_or_default() {
        let n = tag["name"].as_str().unwrap_or("").to_string();
        let c = tag["color"].as_array().map(|a| a.iter().map(|v| v.as_u64().unwrap_or(0) as u8).collect::<Vec<_>>()).unwrap_or_default();
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            if c.len() == 3 {
                ui.painter().rect_filled(r, 2.0, egui::Color32::from_rgb(c[0], c[1], c[2]));
            }
            if ui
                .add_enabled(has_sel || text_sel, egui::Button::new(&n).frame(false))
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Tag the selection"));
                })
                .clicked()
            {
                let _ = if text_sel { app.run("xml.tagText", json!({"tag": n})) } else { app.run("xml.tag", json!({"tag": n})) };
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete"))).clicked() {
                    let _ = app.run("xml.deleteTag", json!({"name": n}));
                }
            });
        });
    }
    if (has_sel || text_sel) && ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Untag Selection"))).clicked() {
        let _ = if text_sel { app.run("xml.tagText", json!({"tag": null})) } else { app.run("xml.tag", json!({"tag": null})) };
    }
    let mut markers = app.ui.tag_markers;
    if ui.checkbox(&mut markers, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Show Tag Markers"))).changed() {
        let _ = app.run("view.tagMarkers", json!({"on": markers}));
    }
    ui.separator();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Structure")).strong());
    let st = app.session.execute("xml.structure", &json!({})).unwrap_or_default();
    crate::rtl::label(ui, egui::RichText::new(st["root"].as_str().unwrap_or("Root")).size(11.0).color(t.text_dim));
    for el in st["elements"].as_array().cloned().unwrap_or_default() {
        let text = el["text"].as_str().unwrap_or("").chars().take(40).collect::<String>();
        let label = format!("  <{}> {}", el["tag"].as_str().unwrap_or(""), text);
        if ui.selectable_label(false, egui::RichText::new(label).size(11.0)).clicked() {
            let _ = app.run("selection.set", json!({"ids": [el["id"]]}));
        }
    }
    // DTD: load, validate (problems listed until the next validation), delete.
    let vid = egui::Id::unique("dtd_problems");
    let mut problems: Option<Vec<String>> = ui.data(|d| d.get_temp(vid));
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Load DTD…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("dtd"))
        {
            match app.run("xml.loadDtd", json!({"path": path})) {
                Ok(_) => problems = None,
                Err(e) => app.status_error("Load DTD", e),
            }
        }
        let has_dtd = info["dtd"].as_bool().unwrap_or(false);
        if ui.add_enabled(has_dtd, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Validate")))).clicked()
            && let Ok(v) = app.session.execute("xml.validate", &json!({}))
        {
            problems = Some(
                v["problems"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|p| format!("{}: {}", p["path"].as_str().unwrap_or(""), p["message"].as_str().unwrap_or("")))
                    .collect(),
            );
        }
        if has_dtd && ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete DTD"))).clicked() {
            let _ = app.run("xml.deleteDtd", json!({}));
            problems = None;
        }
    });
    match &problems {
        Some(list) if list.is_empty() => {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "The structure is valid.")).size(11.0).color(t.text_dim));
        }
        Some(list) => {
            for p in list {
                crate::rtl::label(ui, egui::RichText::new(format!("⚠ {p}")).size(11.0));
            }
        }
        None => {}
    }
    ui.data_mut(|d| match problems {
        Some(p) => {
            d.insert_temp(vid, p);
        }
        None => {
            d.remove::<Vec<String>>(vid);
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Export XML…"))).clicked() {
            let _ = app.run("app.exportXml", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Import XML…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("xml"))
            && let Err(e) = app.run("file.importXml", json!({"path": path}))
        {
            app.status_error("Import XML", e);
        }
    });
}

/// Object States panel: the selected multi-state object's states (click to show one).
pub fn states(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let n = app.session.active().map_or(0, |d| d.selection.items.len());
    let info = app.session.execute("states.list", &json!({})).ok();
    let list = info.as_ref().and_then(|i| i["states"].as_array().cloned()).unwrap_or_default();
    if list.is_empty() {
        crate::rtl::label(
            ui,
            crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Select two or more objects (or a group) to make a multi-state object."))
                    .size(11.0)
                    .color(t.text_dim),
            ),
        );
        if ui
            .add_enabled(
                n > 0,
                egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Convert Selection to Multi-State Object"))),
            )
            .clicked()
            && let Err(e) = app.run("states.create", json!({}))
        {
            app.status_error("Object States", e);
        }
        return;
    }
    let active = info.as_ref().and_then(|i| i["active"].as_u64()).unwrap_or(0) as usize;
    for (i, st) in list.iter().enumerate() {
        if ui.selectable_label(i == active, st.as_str().unwrap_or("")).clicked() {
            let _ = app.run("states.show", json!({"index": i}));
        }
    }
    ui.separator();
    if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Release to Objects"))).clicked() {
        let _ = app.run("states.release", json!({}));
    }
}

/// Buttons and Forms panel: the selected objects' On Release action.
pub fn buttons(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(id) = app.session.active().and_then(|d| d.selection.items.first().copied()) else {
        crate::rtl::label(
            ui,
            crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Select an object to make it a button.")).size(11.0).color(t.text_dim),
            ),
        );
        return;
    };
    let cur = app.session.active().and_then(|d| d.doc.item(id)).and_then(|it| it.button.clone());
    use designcraft_doc::ButtonAction as B;
    let (kind, page, url) = match &cur {
        None => ("none", 0, String::new()),
        Some(B::GoToPage { page }) => ("page", *page, String::new()),
        Some(B::GoToFirstPage) => ("firstPage", 0, String::new()),
        Some(B::GoToLastPage) => ("lastPage", 0, String::new()),
        Some(B::GoToNextPage) => ("nextPage", 0, String::new()),
        Some(B::GoToPreviousPage) => ("previousPage", 0, String::new()),
        Some(B::GoToUrl { url }) => ("url", 0, url.clone()),
    };
    let labels = [
        ("none", "Not a button"),
        ("nextPage", "Go To Next Page"),
        ("previousPage", "Go To Previous Page"),
        ("firstPage", "Go To First Page"),
        ("lastPage", "Go To Last Page"),
        ("page", "Go To Page"),
        ("url", "Go To URL"),
    ];
    let mut pick = None;
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "On Release"));
        let combo = egui::ComboBox::from_id_salt("button_action")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, labels.iter().find(|l| l.0 == kind).map_or("", |l| l.1))))
            .show_ui(ui, |ui| {
                for (k, l) in labels {
                    if ui.selectable_label(k == kind, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        pick = Some(k);
                    }
                }
            });
        let _ = crate::widgets::name_combo(combo, crate::i18n::tr(&app.ui.language, "On Release Action"));
    });
    let mut params = None;
    if let Some(k) = pick {
        params = Some(json!({"action": k, "page": page, "url": if url.is_empty() { "https://" } else { &url }}));
    }
    if kind == "page" {
        let total = app.session.active().map_or(1, |d| d.doc.page_count());
        let mut n = page + 1;
        ui.horizontal(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Page"));
            if ui.add(egui::DragValue::new(&mut n).range(1..=total)).changed() {
                params = Some(json!({"action": "page", "page": n - 1}));
            }
        });
    }
    if kind == "url" {
        let key = egui::Id::unique(("button_url", id.0));
        let mut u: String = ui.data(|d| d.get_temp(key)).unwrap_or(url.clone());
        let r = ui.add(egui::TextEdit::singleline(&mut u).desired_width(f32::INFINITY));
        if r.lost_focus() && u != url {
            params = Some(json!({"action": "url", "url": u}));
        }
        ui.data_mut(|d| d.insert_temp(key, u));
    }
    if let Some(p) = params
        && let Err(e) = app.run("button.set", p)
    {
        app.status_error("Buttons", e);
    }
    crate::rtl::label(
        ui,
        crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Buttons act in interactive PDF export.")).size(10.5).color(t.text_dim),
        ),
    );
    // Form field.
    ui.separator();
    let ff = app.session.active().and_then(|d| d.doc.item(id)).and_then(|it| it.form_field.clone());
    use designcraft_doc::FieldKind as K;
    let kinds = [
        (None, "Not a form field"),
        (Some(K::TextField), "Text Field"),
        (Some(K::CheckBox), "Check Box"),
        (Some(K::ComboBox), "Combo Box"),
        (Some(K::ListBox), "List Box"),
        (Some(K::Signature), "Signature Field"),
    ];
    let cur = ff.as_ref().map(|f| f.kind);
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Form Field"));
        let combo = egui::ComboBox::from_id_salt("form_kind")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, kinds.iter().find(|k| k.0 == cur).map_or("", |k| k.1))))
            .show_ui(ui, |ui| {
                for (k, l) in kinds {
                    if ui.selectable_label(k == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        let _ = match k {
                            Some(k) => app.run("form.set", json!({"kind": k})),
                            None => app.run("form.clear", json!({})),
                        };
                    }
                }
            });
        let _ = crate::widgets::name_combo(combo, crate::i18n::tr(&app.ui.language, "Form Field Type"));
    });
    if let Some(f) = ff {
        let key = egui::Id::unique(("form_edit", id.0));
        let (mut name, mut value, mut opts): (String, String, String) =
            ui.data(|d| d.get_temp(key)).unwrap_or((f.name.clone(), f.value.clone(), f.options.join(", ")));
        let mut commit = false;
        ui.horizontal(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Name"));
            commit |= ui.text_edit_singleline(&mut name).lost_focus();
        });
        if f.kind == K::CheckBox {
            let mut on = !f.value.is_empty() && f.value != "Off";
            if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Checked by default"))).changed() {
                let _ = app.run("form.set", json!({"kind": f.kind, "value": if on { "On" } else { "" }}));
            }
        } else if f.kind != K::Signature {
            ui.horizontal(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Default"));
                commit |= ui.text_edit_singleline(&mut value).lost_focus();
            });
        }
        if matches!(f.kind, K::ComboBox | K::ListBox) {
            ui.horizontal(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Choices"));
                commit |= ui
                    .text_edit_singleline(&mut opts)
                    .on_hover_ui(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Separated by commas"));
                    })
                    .lost_focus();
            });
        }
        let mut req = f.required;
        if ui.checkbox(&mut req, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Required"))).changed() {
            let _ = app.run("form.set", json!({"kind": f.kind, "required": req}));
        }
        if f.kind == K::TextField {
            let mut ml = f.multiline;
            if ui.checkbox(&mut ml, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Multiline"))).changed() {
                let _ = app.run("form.set", json!({"kind": f.kind, "multiline": ml}));
            }
        }
        if commit {
            let options: Vec<String> = opts.split(',').map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
            let _ = app.run("form.set", json!({"kind": f.kind, "name": name, "value": value, "options": options}));
            ui.data_mut(|d| d.remove::<(String, String, String)>(key));
        } else {
            ui.data_mut(|d| d.insert_temp(key, (name, value, opts)));
        }
    }
}

/// Liquid page rules in InDesign's order, with their menu labels.
pub const LIQUID_RULES: [(designcraft_doc::LiquidRule, &str); 5] = [
    (designcraft_doc::LiquidRule::Off, "Off"),
    (designcraft_doc::LiquidRule::Scale, "Scale"),
    (designcraft_doc::LiquidRule::ReCenter, "Re-center"),
    (designcraft_doc::LiquidRule::ObjectBased, "Object-based"),
    (designcraft_doc::LiquidRule::GuideBased, "Guide-based"),
];

/// A liquid page rule's menu label.
pub fn liquid_rule_label(rule: designcraft_doc::LiquidRule) -> &'static str {
    LIQUID_RULES.iter().find(|(r, _)| *r == rule).map_or("Off", |(_, l)| l)
}

/// Whether page `abs` follows the object-based rule (its own, or its parent page's).
fn object_based(d: &designcraft_doc::Document, abs: usize) -> bool {
    use designcraft_doc::LiquidRule as L;
    let Some((si, pi)) = d.page_loc(abs) else { return false };
    match d.spreads[si].pages[pi].liquid {
        L::ObjectBased => true,
        L::ControlledByParent => {
            d.parent_page_for(abs).and_then(|(ppi, ppg)| d.parents.get(ppi)?.pages.get(ppg).map(|p| p.liquid == L::ObjectBased)).unwrap_or(false)
        }
        _ => false,
    }
}

/// Liquid Layout panel, laid out as InDesign's: the page's Liquid Page Rule (with "Page: n"),
/// Content › Auto-Fit, and Object Constraints › Resize with Page (Height, Width) and Pin (Top,
/// Left, Bottom, Right) for the selected object on an object-based page. The page's liquid
/// guides follow (Create Alternate Layout is in the Layout menu).
pub fn liquid(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    // The Page tool's page, the page in view, or (before the canvas lays out) the first.
    let Some(page) = app.session.active().map(|s| s.selection.page.or_else(|| crate::canvas::current_page(app)).unwrap_or(0)) else {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "No document.")).size(11.0).color(t.text_dim));
        return;
    };
    let Some(d) = app.session.active().map(|d| d.doc.clone()) else { return };
    let Some((si, pi)) = d.page_loc(page) else { return };
    let pg = d.spreads[si].pages[pi].clone();
    ui.horizontal(|ui| {
        crate::widgets::caption(ui, crate::i18n::tr(&lang, "Liquid Page Rule:"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::widgets::caption(ui, &format!("{} {}", crate::i18n::tr(&lang, "Page:"), d.page_name(page)));
        });
    });
    let combo = egui::ComboBox::from_id_salt("liquid_rule")
        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&lang, liquid_rule_label(pg.liquid))))
        .width(190.0)
        .show_ui(ui, |ui| {
            for (r, label) in LIQUID_RULES {
                if ui.selectable_label(pg.liquid == r, crate::rtl::widget(ui, crate::i18n::tr(&lang, label))).clicked() && r != pg.liquid {
                    let _ = app.run("liquid.pageRule", json!({"rule": r, "pages": [page + 1]}));
                }
            }
        });
    let _ = crate::widgets::name_combo(combo, crate::i18n::tr(&lang, "Liquid Page Rule"));
    ui.separator();
    // The selected object (one on this page).
    let item = app
        .session
        .active()
        .and_then(|s| s.selection.items.first().copied())
        .and_then(|id| d.item(id))
        .filter(|it| d.page_of_item(it.id) == Some(page));
    // Content: Auto-Fit (graphic frames).
    crate::widgets::caption(ui, crate::i18n::tr(&lang, "Content:"));
    let graphic = item.and_then(|it| if let designcraft_doc::Content::Graphic(g) = &it.content { Some(g.auto_fit) } else { None });
    let mut auto = graphic.is_some_and(|f| f != designcraft_doc::Fitting::None);
    ui.indent("liquid_content", |ui| {
        let r = ui.add_enabled(graphic.is_some(), egui::Checkbox::new(&mut auto, crate::rtl::widget(ui, crate::i18n::tr(&lang, "Auto-Fit"))));
        if r.changed() {
            let _ = app.run("object.fittingOptions", json!({"autoFit": auto}));
        }
    });
    // Object Constraints: for an object on an object-based page.
    crate::widgets::caption(ui, crate::i18n::tr(&lang, "Object Constraints:"));
    let on = item.is_some() && object_based(&d, page);
    let l = item
        .filter(|_| on)
        .map(|it| it.liquid.unwrap_or(designcraft_doc::ObjectLiquid { resize_width: true, resize_height: true, ..Default::default() }))
        .unwrap_or_default();
    ui.indent("liquid_constraints", |ui| {
        ui.add_enabled_ui(on, |ui| {
            let rows: [(&str, [(&str, &str, bool); 2]); 2] = [
                ("Resize with Page:", [("Height", "resizeHeight", l.resize_height), ("Width", "resizeWidth", l.resize_width)]),
                ("Pin:", [("Top", "pinTop", l.pin_top), ("Left", "pinLeft", l.pin_left)]),
            ];
            for (title, boxes) in rows {
                crate::widgets::caption(ui, crate::i18n::tr(&lang, title));
                let mut grid_boxes = vec![boxes.to_vec()];
                if title == "Pin:" {
                    grid_boxes.push(vec![("Bottom", "pinBottom", l.pin_bottom), ("Right", "pinRight", l.pin_right)]);
                }
                egui::Grid::new(("liquid_grid", title)).num_columns(2).min_col_width(90.0).show(ui, |ui| {
                    for row in grid_boxes {
                        for (label, key, value) in row {
                            let mut v = value;
                            if ui.checkbox(&mut v, crate::rtl::widget(ui, crate::i18n::tr(&lang, label))).changed() {
                                let _ = app.run("liquid.object", json!({key: v}));
                            }
                        }
                        ui.end_row();
                    }
                });
            }
        });
    });
    if !pg.guides.is_empty() {
        ui.separator();
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Liquid guides")).size(11.0).color(t.text_dim));
        for (gi, g) in pg.guides.iter().enumerate() {
            let mut on = g.liquid;
            let label = format!("{:?} at {:.1}", g.orientation, g.position);
            if ui.checkbox(&mut on, label).changed() {
                let _ = app.run("guide.liquid", json!({"spread": si, "page": pi, "index": gi, "on": on}));
            }
        }
    }
}

/// Media panel: the selected video or sound's playback options and poster.
pub fn media(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Ok(m) = app.session.execute("media.get", &json!({})) else {
        crate::rtl::label(
            ui,
            crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Select a placed video or sound (File › Place a .mp4, .mov, .mp3, .wav …)."))
                    .size(11.0)
                    .color(t.text_dim),
            ),
        );
        return;
    };
    crate::rtl::label(ui, egui::RichText::new(format!("{} — {}", m["kind"].as_str().unwrap_or(""), m["name"].as_str().unwrap_or(""))).strong());
    for (key, label) in [("playOnPageLoad", "Play on Page Load"), ("loop", "Loop"), ("controls", "Show Controls")] {
        let mut on = m[key].as_bool().unwrap_or(false);
        if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).changed() {
            let _ = app.run("media.options", json!({key: on}));
        }
    }
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Poster"));
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Choose Image…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("place"))
            && let Err(e) = app.run("media.options", json!({"poster": {"path": path}}))
        {
            app.status_error("Media", e);
        }
        if !m["poster"].is_null() && ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "None"))).clicked() {
            let _ = app.run("media.options", json!({"poster": null}));
        }
    });
    crate::rtl::label(
        ui,
        crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Plays in EPUB export; print and PDF show the poster."))
                .size(10.5)
                .color(t.text_dim),
        ),
    );
}

/// Page Transitions panel: the current spread's transition for interactive PDF.
pub fn transitions(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(page) = crate::canvas::current_page(app) else { return };
    let Some(d) = app.session.active().map(|d| d.doc.clone()) else { return };
    let Some((si, _)) = d.page_loc(page) else { return };
    let cur = d.spreads[si].pages.first().and_then(|p| p.transition);
    use designcraft_doc::TransitionKind as K;
    crate::rtl::label(ui, egui::RichText::new(format!("{} {}", crate::i18n::tr(&app.ui.language, "Spread"), si + 1)).strong());
    let mut kind = cur.map(|c| c.kind);
    let combo = egui::ComboBox::from_id_salt("transition_kind")
        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, kind.map_or("None", K::label))))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut kind, None, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "None")));
            for k in K::ALL {
                ui.selectable_value(&mut kind, Some(k), crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, k.label())));
            }
        });
    let _ = crate::widgets::name_combo(combo, crate::i18n::tr(&app.ui.language, "Transition"));
    let mut duration = cur.map_or(1.0, |c| c.duration);
    let mut horizontal = cur.is_some_and(|c| c.horizontal);
    let mut changed = kind != cur.map(|c| c.kind);
    if kind.is_some() {
        ui.horizontal(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Speed"));
            changed |= ui.add(egui::DragValue::new(&mut duration).range(0.1..=10.0).speed(0.1).suffix(" s")).changed();
        });
        if matches!(kind, Some(K::Blinds | K::Box | K::Split)) {
            changed |= ui
                .checkbox(
                    &mut horizontal,
                    crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, if kind == Some(K::Box) { "Inward" } else { "Horizontal" })),
                )
                .changed();
        }
    }
    let params = |all: bool| {
        let k = kind.map_or(json!("none"), |k| json!(k));
        if all {
            json!({"all": true, "kind": k, "duration": duration, "horizontal": horizontal})
        } else {
            json!({"spread": si, "kind": k, "duration": duration, "horizontal": horizontal})
        }
    };
    if changed {
        let _ = app.run("page.transition", params(false));
    }
    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Apply to All Spreads"))).clicked() {
        let _ = app.run("page.transition", params(true));
    }
    crate::rtl::label(
        ui,
        crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Transitions play in interactive PDF (full-screen mode)."))
                .size(10.5)
                .color(t.text_dim),
        ),
    );
}

/// Track Changes panel: tracking on/off and each change with Accept / Reject.
pub fn track_changes(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(on) = app.session.active().map(|d| d.doc.settings.track_changes) else { return };
    let mut track = on;
    if ui.checkbox(&mut track, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Track Changes in All Stories"))).changed() {
        let _ = app.run("changes.track", json!({"on": track}));
    }
    let list = app.session.execute("changes.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    ui.separator();
    if list.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No changes.")).size(11.0).color(t.text_dim));
        return;
    }
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for c in &list {
            let (story, start) = (c["story"].clone(), c["start"].clone());
            let kind = c["kind"].as_str().unwrap_or("");
            let text: String = c["text"].as_str().unwrap_or("").chars().take(40).collect();
            ui.horizontal(|ui| {
                let label = egui::RichText::new(format!(
                    "{} “{}”",
                    crate::i18n::tr(&app.ui.language, if kind == "inserted" { "Added" } else { "Deleted" }),
                    text.replace('\n', "¶")
                ))
                .size(11.0);
                let label = if kind == "deleted" { label.strikethrough() } else { label };
                if ui
                    .selectable_label(false, label)
                    .on_hover_ui(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Show in the text"));
                    })
                    .clicked()
                {
                    let end = c["end"].clone();
                    let _ = app.run("text.select", json!({"story": story, "anchor": start, "focus": end}));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Reject"))).clicked() {
                        let _ = app.run("changes.reject", json!({"story": story, "start": start}));
                    }
                    if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Accept"))).clicked() {
                        let _ = app.run("changes.accept", json!({"story": story, "start": start}));
                    }
                });
            });
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Accept All"))).clicked() {
            let _ = app.run("changes.acceptAll", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Reject All"))).clicked() {
            let _ = app.run("changes.rejectAll", json!({}));
        }
    });
}

#[cfg(test)]
mod liquid_panel_tests {
    use egui_kittest::{Harness, kittest::Queryable};
    use serde_json::json;

    use super::*;

    #[test]
    fn object_constraints_set_the_selected_objects_pins_and_resizing() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(260.0, 420.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let key = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(key, true));
                    return;
                }
                liquid(app, ui);
            },
            app,
        );
        h.run_steps(3);
        let l = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().liquid;
        // With the rule off the constraints don't apply.
        h.get_by_label("Right").click();
        h.run_steps(2);
        assert_eq!(l(&h), None);
        h.state_mut().run("liquid.pageRule", json!({"rule": "objectBased"})).unwrap();
        h.run_steps(2);
        h.get_by_label("Right").click();
        h.run_steps(2);
        h.get_by_label("Width").click();
        h.run_steps(2);
        let v = l(&h).unwrap();
        assert!(v.pin_right && !v.resize_width && v.resize_height, "{v:?}");
        let _ = h.get_by_label("Auto-Fit");
    }
}
