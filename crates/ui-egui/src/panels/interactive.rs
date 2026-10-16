//! Hyperlinks and Bookmarks panels (Window › Interactive).

use serde_json::{Value, json};

use crate::DesignApp;
use crate::theme::Tokens;

fn dest_text(d: &Value) -> String {
    if let Some(u) = d.get("url").and_then(Value::as_str) {
        return u.to_string();
    }
    if let Some(e) = d.get("email").and_then(Value::as_str) {
        return format!("mailto:{e}");
    }
    if let Some(p) = d.get("page").and_then(Value::as_u64) {
        return format!("Page {}", p + 1);
    }
    d.to_string()
}

pub fn hyperlinks(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let id = egui::Id::new("new_hyperlink_url");
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
                app.status(format!("Hyperlinks: {e}"));
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
            let r = ui.selectable_label(false, h["name"].as_str().unwrap_or("")).on_hover_text(dest_text(&h["dest"]));
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
            if ui.selectable_label(false, b["name"].as_str().unwrap_or("")).on_hover_text(format!("Page {}", app.session.page_label(page))).clicked()
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
                .push_id(("article_export", &name), |ui| crate::widgets::native_checkbox(ui, &mut export, ""))
                .inner
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
    let id = egui::Id::new("new_tag_name");
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
                Err(e) => app.status(format!("Tags: {e}")),
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
    if crate::widgets::native_checkbox(ui, &mut markers, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Show Tag Markers"))).changed() {
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
    let vid = egui::Id::new("dtd_problems");
    let mut problems: Option<Vec<String>> = ui.data(|d| d.get_temp(vid));
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Load DTD…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("dtd"))
        {
            match app.run("xml.loadDtd", json!({"path": path})) {
                Ok(_) => problems = None,
                Err(e) => app.status(format!("Load DTD: {e}")),
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
            app.status(format!("Import XML: {e}"));
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
            app.status(format!("Object States: {e}"));
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
        egui::ComboBox::from_id_salt("button_action")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, labels.iter().find(|l| l.0 == kind).map_or("", |l| l.1))))
            .show_ui(ui, |ui| {
                for (k, l) in labels {
                    if ui.selectable_label(k == kind, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        pick = Some(k);
                    }
                }
            });
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
        let key = egui::Id::new(("button_url", id.0));
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
        app.status(format!("Buttons: {e}"));
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
        egui::ComboBox::from_id_salt("form_kind")
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
    });
    if let Some(f) = ff {
        let key = egui::Id::new(("form_edit", id.0));
        let (mut name, mut value, mut opts): (String, String, String) =
            ui.data(|d| d.get_temp(key)).unwrap_or((f.name.clone(), f.value.clone(), f.options.join(", ")));
        let mut commit = false;
        ui.horizontal(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Name"));
            commit |= ui.text_edit_singleline(&mut name).lost_focus();
        });
        if f.kind == K::CheckBox {
            let mut on = !f.value.is_empty() && f.value != "Off";
            if crate::widgets::native_checkbox(ui, &mut on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Checked by default"))).changed()
            {
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
        if crate::widgets::native_checkbox(ui, &mut req, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Required"))).changed() {
            let _ = app.run("form.set", json!({"kind": f.kind, "required": req}));
        }
        if f.kind == K::TextField {
            let mut ml = f.multiline;
            if crate::widgets::native_checkbox(ui, &mut ml, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Multiline"))).changed() {
                let _ = app.run("form.set", json!({"kind": f.kind, "multiline": ml}));
            }
        }
        if commit {
            let options: Vec<String> = opts.split(',').map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
            // The Name cache may predate a checkbox interaction. Its focus-loss
            // commit must preserve the value already stored by the checkbox.
            let params = if f.kind == K::CheckBox {
                json!({"kind": f.kind, "name": name, "options": options})
            } else {
                json!({"kind": f.kind, "name": name, "value": value, "options": options})
            };
            let _ = app.run("form.set", params);
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
    egui::ComboBox::from_id_salt("liquid_rule")
        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&lang, liquid_rule_label(pg.liquid))))
        .width(190.0)
        .show_ui(ui, |ui| {
            for (r, label) in LIQUID_RULES {
                if ui.selectable_label(pg.liquid == r, crate::rtl::widget(ui, crate::i18n::tr(&lang, label))).clicked() && r != pg.liquid {
                    let _ = app.run("liquid.pageRule", json!({"rule": r, "pages": [page + 1]}));
                }
            }
        });
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
        let r = ui
            .add_enabled_ui(graphic.is_some(), |ui| {
                crate::widgets::native_checkbox(ui, &mut auto, crate::rtl::widget(ui, crate::i18n::tr(&lang, "Auto-Fit")))
            })
            .inner;
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
                            if crate::widgets::native_checkbox(ui, &mut v, crate::rtl::widget(ui, crate::i18n::tr(&lang, label))).changed() {
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
            if ui.push_id(("liquid_guide", pg.id, gi), |ui| crate::widgets::native_checkbox(ui, &mut on, label)).inner.changed() {
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
        if crate::widgets::native_checkbox(ui, &mut on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).changed() {
            let _ = app.run("media.options", json!({key: on}));
        }
    }
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Poster"));
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Choose Image…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("place"))
            && let Err(e) = app.run("media.options", json!({"poster": {"path": path}}))
        {
            app.status(format!("Media: {e}"));
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
    egui::ComboBox::from_id_salt("transition_kind")
        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, kind.map_or("None", K::label))))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut kind, None, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "None")));
            for k in K::ALL {
                ui.selectable_value(&mut kind, Some(k), crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, k.label())));
            }
        });
    let mut duration = cur.map_or(1.0, |c| c.duration);
    let mut horizontal = cur.is_some_and(|c| c.horizontal);
    let mut changed = kind != cur.map(|c| c.kind);
    if kind.is_some() {
        ui.horizontal(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Speed"));
            changed |= ui.add(egui::DragValue::new(&mut duration).range(0.1..=10.0).speed(0.1).suffix(" s")).changed();
        });
        if matches!(kind, Some(K::Blinds | K::Box | K::Split)) {
            changed |= crate::widgets::native_checkbox(
                ui,
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
    if crate::widgets::native_checkbox(ui, &mut track, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Track Changes in All Stories")))
        .changed()
    {
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
mod tests {
    use super::*;

    fn buttons_frame(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<(String, egui::Rect)> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
            ui.set_max_width(360.0);
            buttons(app, ui);
        });
        output.textures_delta.clear();
        fn collect(shape: &egui::Shape, clip: egui::Rect, labels: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(text) => {
                    let rect = text.visual_bounding_rect();
                    if clip.contains_rect(rect) {
                        labels.push((text.galley.job.text.trim().to_owned(), rect));
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, clip, labels);
                    }
                }
                _ => {}
            }
        }
        let mut labels = Vec::new();
        for shape in output.shapes {
            collect(&shape.shape, shape.clip_rect.intersect(screen), &mut labels);
        }
        labels
    }

    fn center(labels: &[(String, egui::Rect)], name: &str) -> egui::Pos2 {
        labels.iter().find(|(label, _)| label == name).unwrap_or_else(|| panic!("{name} is visibly painted")).1.center()
    }

    fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    fn field(app: &DesignApp, id: designcraft_doc::ItemId) -> designcraft_doc::FormField {
        app.session.active().unwrap().doc.item(id).unwrap().form_field.clone().unwrap()
    }

    #[test]
    fn checkbox_held_article_origin_cannot_follow_queued_delete() {
        use designcraft_doc::Article;
        use designcraft_engine::{DocState, HistoryEntry};
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };
        use std::sync::Arc;

        fn same(app: &DesignApp, e: &DocState, prefs: &Value, ui_prefs: &Value) {
            assert_eq!(app.session.documents().len(), 1);
            assert_eq!(app.session.active_index(), Some(0));
            let a = app.session.active().unwrap();
            macro_rules! fields {
            ($($f:ident),*) => {$(assert_eq!(a.$f, e.$f, stringify!($f));)*};
        }
            fields!(doc, selection, uid, revision, saved_revision, saved_doc, active_layer, path, editing_parents, preview_stash, preview_record);
            assert_eq!(a.is_dirty(), e.is_dirty());
            assert!(a.interaction.is_none() && e.interaction.is_none());
            assert!(a.fonts.is_none() && e.fonts.is_none());
            assert_eq!((a.history.undo.len(), a.history.redo.len(), a.history.limit), (e.history.undo.len(), e.history.redo.len(), e.history.limit));
            for (x, y) in a.history.undo.iter().chain(&a.history.redo).zip(e.history.undo.iter().chain(&e.history.redo)) {
                assert_eq!((&x.label, x.doc.as_ref(), &x.selection), (&y.label, y.doc.as_ref(), &y.selection));
            }
            assert_eq!(&serde_json::to_value(&app.session.prefs).unwrap(), prefs);
            assert_eq!(&serde_json::to_value(&app.ui).unwrap(), ui_prefs);
        }
        fn row(h: &Harness<'_, DesignApp>, name: &str) -> (egui::accesskit::NodeId, egui::Rect) {
            let label = h.get_by_label(name).rect();
            let found: Vec<_> = h
                .root()
                .children_recursive()
                .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::CheckBox)
                .filter(|n| {
                    let r = n.rect();
                    r.right() <= label.left() && r.top() <= label.center().y && r.bottom() >= label.center().y
                })
                .map(|n| {
                    assert!(!n.accesskit_node().is_disabled());
                    (n.accesskit_node().locate().0, n.rect())
                })
                .collect();
            assert_eq!(found.len(), 1, "one actual unlabeled checkbox beside article {name}");
            found[0]
        }
        fn occupied(h: &Harness<'_, DesignApp>) -> Vec<egui::Rect> {
            use egui::accesskit::Role;
            h.root()
                .children_recursive()
                .filter(|n| matches!(n.accesskit_node().role(), Role::CheckBox | Role::Button | Role::TextInput | Role::ScrollBar | Role::Label))
                .filter(|n| n.accesskit_node().bounding_box().is_some())
                .map(|n| n.rect())
                .collect()
        }
        fn blank_guard(h: &Harness<'_, DesignApp>, p: egui::Pos2, layer: egui::LayerId) {
            assert!(h.ctx.content_rect().contains(p));
            assert_eq!(h.ctx.layer_id_at(p), Some(layer));
            assert!(h.ctx.memory(|m| m.allows_interaction(layer)));
            assert!(occupied(h).iter().all(|r| !r.expand(8.0).contains(p)), "blank must remain outside all current row/header controls");
        }
        fn step(h: &mut Harness<'_, DesignApp>, events: Vec<egui::Event>) {
            assert!(h.input().events.is_empty());
            let frame = h.ctx.cumulative_frame_nr();
            h.input_mut().events.extend(events);
            h.run_steps(1);
            assert_eq!(h.ctx.cumulative_frame_nr(), frame + 1);
        }

        // Preserve both original cases, then reuse the deleted origin name before redraw.
        for (delete, reuse) in [(false, false), (true, false), (true, true)] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.ui.language = "en".into();
            app.run("file.new", json!({"facingPages":false,"primaryTextFrame":false})).unwrap();
            for name in ["A", "B"] {
                app.run("article.new", json!({"name":name,"ids":[]})).unwrap();
            }
            // A real Redo entry must be cleared by the intended origin edit, not by B.
            let restored = app.session.active().unwrap().doc.clone();
            app.run("layout.documentSetup", json!({"facingPages":true})).unwrap();
            assert_ne!(app.session.active().unwrap().doc, restored);
            app.run("edit.undo", json!({})).unwrap();
            assert_eq!(app.session.active().unwrap().doc, restored);
            assert_eq!(app.session.active().unwrap().history.redo.len(), 1);
            app.ui.docking = None;
            app.ui.floating = vec![("articles".into(), [60.0, 60.0])];
            let (sender, receiver) = std::sync::mpsc::channel();
            app.control_rx = Some(receiver);
            let mut ready = false;
            let mut h = Harness::builder().with_size(egui::vec2(1000.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
                move |ui, app: &mut DesignApp| {
                    if !ready {
                        ready = true;
                        return;
                    }
                    let ctx = ui.ctx().clone();
                    app.logic(&ctx);
                    if !app.fonts_ready {
                        return;
                    }
                    egui::CentralPanel::default().show(ui, |_| {});
                    crate::dock::floating(app, &ctx);
                },
                app,
            );
            h.input_mut().max_texture_side = Some(8192);
            h.run_steps(8);
            let before = h.state().session.active().unwrap().clone();
            let prefs = serde_json::to_value(&h.state().session.prefs).unwrap();
            let ui_prefs = serde_json::to_value(&h.state().ui).unwrap();
            assert_eq!(
                before.doc.articles,
                vec![Article { name: "A".into(), items: vec![], export: true }, Article { name: "B".into(), items: vec![], export: true },]
            );
            assert!(before.history.undo.len() + 2 <= before.history.limit.max(1));
            if reuse {
                assert!(before.history.undo.len() + 3 <= before.history.limit.max(1));
            }
            assert!(before.selection.items.is_empty() && before.selection.text.is_none());
            assert!(h.state().ui.dialog.is_none() && h.state().ui.palette.is_none());
            let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new(("floating_panel", "articles")));
            let (a_id, a_rect) = row(&h, "A");
            let (b_id, b_rect) = row(&h, "B");
            assert_ne!(a_id, b_id);
            assert!(a_rect.bottom() <= b_rect.top());
            for r in [a_rect, b_rect] {
                assert!(h.ctx.content_rect().contains_rect(r));
                assert_eq!(h.ctx.layer_id_at(r.center()), Some(layer));
            }
            let p = a_rect.center();
            let area = h.ctx.memory(|m| m.area_rect(layer.id)).unwrap();
            let rects = occupied(&h);
            let blank = (1..area.width() as usize)
                .map(|dx| egui::pos2(area.left() + dx as f32, p.y))
                .find(|q| p.distance(*q) > 32.0 && h.ctx.layer_id_at(*q) == Some(layer) && rects.iter().all(|r| !r.expand(8.0).contains(*q)))
                .expect("PREREQUISITE: blank gap on first article row, beyond every control");
            blank_guard(&h, blank, layer);
            same(h.state(), &before, &prefs, &ui_prefs);

            // Literal origin oracle: exactly one Article Options edit, no production command replay.
            let mut origin_only = before.clone();
            Arc::make_mut(&mut origin_only.doc).articles =
                vec![Article { name: "A".into(), items: vec![], export: false }, Article { name: "B".into(), items: vec![], export: true }];
            origin_only.revision += 1;
            origin_only.history.undo.push(HistoryEntry {
                label: "Article Options".into(),
                doc: before.doc.clone(),
                selection: before.selection.clone(),
            });
            origin_only.history.redo.clear();

            step(&mut h, vec![egui::Event::PointerMoved(p), pointer(p, true)]);
            assert!(h.ctx.input(|i| i.pointer.primary_pressed()
                && i.pointer.primary_down()
                && !i.pointer.primary_released()
                && i.pointer.interact_pos() == Some(p)));
            same(h.state(), &origin_only, &prefs, &ui_prefs);
            step(&mut h, vec![egui::Event::PointerMoved(blank)]);
            assert!(h.ctx.input(|i| i.pointer.primary_down()
                && i.pointer.is_decidedly_dragging()
                && !i.pointer.primary_pressed()
                && !i.pointer.primary_released()));
            assert_eq!(row(&h, "A"), (a_id, a_rect));
            let origin = h.ctx.dragged_id().expect("PREREQUISITE: actual checkbox retains capture");
            assert_eq!(origin.accesskit_id(), a_id);
            let response = h.ctx.read_response(origin).unwrap();
            assert!(response.enabled() && response.is_pointer_button_down_on());
            assert_eq!(response.layer_id, layer);
            assert!(response.interact_rect.contains(p) && !response.interact_rect.contains(blank));
            blank_guard(&h, blank, layer);
            same(h.state(), &origin_only, &prefs, &ui_prefs);

            let mut expected = origin_only.clone();
            if delete {
                Arc::make_mut(&mut expected.doc).articles = vec![Article { name: "B".into(), items: vec![], export: true }];
                expected.revision += 1; // Exactly one intentional deletion after the origin edit.
                expected.history.undo.push(HistoryEntry {
                    label: "Delete Article".into(),
                    doc: origin_only.doc.clone(),
                    selection: origin_only.selection.clone(),
                });
            }
            if reuse {
                // The surviving B is intentionally renamed A: a separate literal edit/Undo entry.
                let deleted_doc = expected.doc.clone();
                Arc::make_mut(&mut expected.doc).articles = vec![Article { name: "A".into(), items: vec![], export: true }];
                expected.revision += 1;
                expected.history.undo.push(HistoryEntry {
                    label: "Article Options".into(),
                    doc: deleted_doc,
                    selection: origin_only.selection.clone(),
                });
            }
            let reply = if delete {
                let (request, reply) = crate::ControlRequest::new("engine.execute", json!({"command":"article.delete","params":{"name":"A"}}));
                sender.send(request).unwrap();
                Some(reply)
            } else {
                None
            };
            let rename_reply = if reuse {
                let (request, reply) =
                    crate::ControlRequest::new("engine.execute", json!({"command":"article.options","params":{"name":"B","to":"A"}}));
                sender.send(request).unwrap();
                Some(reply)
            } else {
                None
            };
            // Both requests are queued, in order, before ONE real app.logic/layout frame.
            step(&mut h, vec![]);
            if let Some(reply) = reply {
                let result = reply.try_recv().expect("queued deletion completes in this frame");
                assert_eq!(result["ok"], json!(true), "{result}");
                if !reuse {
                    assert!(h.query_by_label("A").is_none());
                }
            }
            if let Some(reply) = rename_reply {
                let result = reply.try_recv().expect("queued rename completes in the same frame");
                assert_eq!(result["ok"], json!(true), "{result}");
                assert!(h.query_by_label("B").is_none(), "surviving B is now labelled A");
            }
            assert!(h.ctx.input(|i| i.pointer.primary_down()
                && i.pointer.is_decidedly_dragging()
                && !i.pointer.primary_pressed()
                && !i.pointer.primary_released()
                && i.pointer.interact_pos() == Some(blank)
                && !i.events.iter().any(|e| matches!(e, egui::Event::Key { .. }))));
            let (current_id, current_rect) = row(&h, if delete && !reuse { "B" } else { "A" });
            assert_eq!(current_rect, a_rect, "PREREQUISITE: live replacement occupies original row");
            assert!(h.ctx.content_rect().contains_rect(current_rect) && !current_rect.contains(blank));
            assert_eq!(h.ctx.layer_id_at(current_rect.center()), Some(layer));
            blank_guard(&h, blank, layer);
            let alias = current_id == origin.accesskit_id();
            eprintln!("delete={delete} reuse={reuse} alias={alias} origin={origin:?} current={current_id:?} blank={blank:?}");
            if alias {
                assert_eq!(h.ctx.dragged_id(), Some(origin));
                let current = h.ctx.read_response(origin).unwrap();
                assert!(current.enabled() && current.is_pointer_button_down_on());
                assert_eq!(current.layer_id, layer);
                assert!(!current.interact_rect.contains(blank));
            } else {
                // Only actual capture/current Node IDs; never construct an egui ID from a Node ID.
                assert_ne!(h.ctx.dragged_id().map(|id| id.accesskit_id()), Some(current_id));
            }
            // All fixture guards precede the first product assertion.
            let survivor = if reuse { "A" } else { "B" };
            assert!(
                h.state().session.active().unwrap().doc.articles.iter().find(|a| a.name == survivor).unwrap().export,
                "surviving B (now {survivor}) must stay exportable; delete={delete} reuse={reuse}"
            );
            same(h.state(), &expected, &prefs, &ui_prefs);
            for events in [vec![], vec![pointer(blank, false)], vec![], vec![]] {
                step(&mut h, events);
                same(h.state(), &expected, &prefs, &ui_prefs);
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum HeldGuideMutation {
        Delete,
        DeleteAllThenAdd,
        CreateGuidesThenAdd,
    }

    #[test]
    fn checkbox_held_liquid_guide_origin_cannot_follow_queued_delete() {
        held_liquid_guide_origin_case(HeldGuideMutation::Delete);
    }

    #[test]
    fn checkbox_held_liquid_guide_origin_cannot_follow_queued_delete_all_then_add() {
        held_liquid_guide_origin_case(HeldGuideMutation::DeleteAllThenAdd);
    }

    #[test]
    fn checkbox_held_liquid_guide_origin_cannot_follow_queued_create_guides_then_add() {
        held_liquid_guide_origin_case(HeldGuideMutation::CreateGuidesThenAdd);
    }

    fn liquid_guide_checkbox_count(h: &egui_kittest::Harness<'_, DesignApp>) -> usize {
        use egui::accesskit::Role;
        use egui_kittest::kittest::{NodeT, Queryable};

        // The d07 panel keeps these object controls visible without a selection.
        // Exclude only their actual nodes, while retaining every guide row.
        let object_controls = ["Auto-Fit", "Height", "Width", "Top", "Left", "Bottom", "Right"].map(|label| {
            let node = h.get_by_role_and_label(Role::CheckBox, label);
            assert!(node.accesskit_node().is_disabled(), "{label} must stay disabled without a selected object");
            node.accesskit_node().id()
        });
        h.root()
            .children_recursive()
            .filter(|node| node.accesskit_node().role() == Role::CheckBox)
            .filter(|node| !object_controls.contains(&node.accesskit_node().id()))
            .count()
    }

    fn held_liquid_guide_origin_case(mutation: HeldGuideMutation) {
        use designcraft_doc::{Guide, LiquidRule, Orientation, Selection};
        use designcraft_engine::{DocState, HistoryEntry};
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };
        use std::sync::Arc;

        fn same(a: &DocState, e: &DocState) {
            macro_rules! fields {
            ($($f:ident),*) => {$(assert_eq!(a.$f, e.$f, stringify!($f));)*}
        }
            fields!(doc, selection, uid, revision, saved_revision, saved_doc, active_layer, path, editing_parents, preview_stash, preview_record);
            assert_eq!(a.is_dirty(), e.is_dirty());
            assert!(a.interaction.is_none() && e.interaction.is_none());
            assert!(a.fonts.is_none() && e.fonts.is_none());
            assert_eq!((a.history.undo.len(), a.history.redo.len(), a.history.limit), (e.history.undo.len(), e.history.redo.len(), e.history.limit));
            for (x, y) in a.history.undo.iter().chain(&a.history.redo).zip(e.history.undo.iter().chain(&e.history.redo)) {
                assert_eq!((&x.label, x.doc.as_ref(), &x.selection), (&y.label, y.doc.as_ref(), &y.selection));
            }
        }
        fn state(h: &Harness<'_, DesignApp>, expected: &DocState, prefs: &Value, ui_prefs: &Value) {
            let app = h.state();
            assert_eq!(app.session.documents().len(), 1);
            assert_eq!(app.session.active_index(), Some(0));
            assert_eq!(crate::canvas::current_page(app), Some(0));
            assert_eq!(app.session.active().unwrap().selection, Selection::default());
            same(app.session.active().unwrap(), expected);
            assert_eq!(&serde_json::to_value(&app.session.prefs).unwrap(), prefs);
            assert_eq!(&serde_json::to_value(&app.ui).unwrap(), ui_prefs);
        }
        // No production command/helper computes an expected document or history entry.
        fn literal_edit(before: &DocState, guides: Vec<Guide>, label: &str) -> DocState {
            let mut e = before.clone();
            let d = Arc::make_mut(&mut e.doc);
            Arc::make_mut(&mut d.spreads[0]).pages[0].guides = guides;
            e.revision += 1;
            e.history.undo.push(HistoryEntry { label: label.into(), doc: before.doc.clone(), selection: before.selection.clone() });
            e.history.redo.clear();
            e
        }
        fn step(h: &mut Harness<'_, DesignApp>, events: Vec<egui::Event>) {
            let frame = h.ctx.cumulative_frame_nr();
            assert!(h.input().events.is_empty());
            h.input_mut().events.extend(events);
            h.run_steps(1);
            assert_eq!(h.ctx.cumulative_frame_nr(), frame + 1);
        }
        fn measure(h: &mut Harness<'_, DesignApp>, label: &str) -> egui::Response {
            let n = h.get_by_label(label);
            assert_eq!(n.accesskit_node().role(), egui::accesskit::Role::CheckBox);
            let local = n.accesskit_node().locate().0;
            let composite = n.accesskit_node().id();
            let rect = n.rect();
            assert!(h.ctx.content_rect().contains_rect(rect));
            let layer = h.ctx.layer_id_at(rect.center()).expect("uncovered actual panel row");
            assert!(!h.ctx.input(|i| i.pointer.any_down()));
            step(h, vec![egui::Event::PointerMoved(rect.center())]);
            assert_eq!(h.get_by_label(label).accesskit_node().id(), composite);
            assert_eq!(h.get_by_label(label).rect(), rect);
            let id = h
                .ctx
                .interaction_snapshot(|s| s.hovered.iter().copied().find(|id| id.accesskit_id() == local))
                .expect("PREREQUISITE: actual checkbox receives hover");
            let r = h.ctx.read_response(id).unwrap();
            assert!(r.enabled() && r.interact_rect.contains(rect.center()));
            assert_eq!(r.layer_id, layer);
            r
        }
        fn blank_guard(h: &Harness<'_, DesignApp>, p: egui::Pos2, layer: egui::LayerId) {
            use egui::accesskit::Role;
            assert!(h.ctx.content_rect().contains(p));
            assert_eq!(h.ctx.layer_id_at(p), Some(layer));
            assert!(
                h.root()
                    .children_recursive()
                    .filter(|n| matches!(n.accesskit_node().role(), Role::CheckBox | Role::Button | Role::TextInput | Role::ScrollBar | Role::Label))
                    .filter(|n| n.accesskit_node().bounding_box().is_some())
                    .all(|n| !n.rect().expand(8.0).contains(p)),
                "PREREQUISITE: actual blank outside controls"
            );
        }
        fn quiet_held(h: &Harness<'_, DesignApp>, blank: egui::Pos2) {
            assert!(h.ctx.input(|i| i.pointer.primary_down()
                && !i.pointer.primary_pressed()
                && !i.pointer.primary_released()
                && i.pointer.is_decidedly_dragging()
                && i.pointer.interact_pos() == Some(blank)
                && !i.events.iter().any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::PointerButton { .. } | egui::Event::Text(_)))));
            assert!(!h.ctx.text_edit_focused());
        }

        for delete in [false, true] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.ui.language = "en".into();
            app.run("file.new", json!({"pages":1, "facingPages":false, "primaryTextFrame":false})).unwrap();
            app.run("style.character.create", json!({"name":"Guide sentinel", "chars":{"fontStyle":"Italic"}})).unwrap();
            app.run("liquid.pageRule", json!({"rule":"guideBased", "pages":[1]})).unwrap();
            for (index, position) in [100.0, 200.0].into_iter().enumerate() {
                assert_eq!(
                    app.run(
                        "guide.add",
                        json!({
                            "spread":0, "page":0, "orientation":"vertical", "position":position, "spreadGuide":false
                        })
                    )
                    .unwrap(),
                    json!({"page":0, "index":index})
                );
            }
            let layer = app.session.active().unwrap().active_layer;
            let a = Guide { orientation: Orientation::Vertical, position: 100.0, spread: false, locked: false, layer: Some(layer), liquid: false };
            let b = Guide { position: 200.0, ..a.clone() };
            let restored = app.session.active().unwrap().clone();
            assert_eq!(restored.doc.page_count(), 1);
            assert_eq!(restored.doc.page_loc(0), Some((0, 0)));
            assert_eq!(restored.doc.spreads[0].pages[0].liquid, LiquidRule::GuideBased);
            assert_eq!(restored.doc.spreads[0].pages[0].guides, vec![a.clone(), b.clone()]);
            assert!(a.editable_in(&restored.doc) && b.editable_in(&restored.doc));
            assert_eq!(restored.selection, Selection::default());
            app.run("layout.documentSetup", json!({"facingPages":true})).unwrap();
            assert_ne!(app.session.active().unwrap().doc, restored.doc);
            app.run("edit.undo", json!({})).unwrap();
            assert_eq!(app.session.active().unwrap().doc, restored.doc);
            assert_eq!(app.session.active().unwrap().selection, Selection::default());
            let before = app.session.active().unwrap().clone();
            assert!(!before.history.undo.is_empty());
            assert_eq!(before.history.redo.len(), 1);
            let prefs = serde_json::to_value(&app.session.prefs).unwrap();
            let ui_prefs = serde_json::to_value(&app.ui).unwrap();
            // Liquid reads the real current-page view, not Selection. Establish only view geometry.
            app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)));
            crate::canvas::go_to_page(&mut app, 0);
            assert_eq!(crate::canvas::current_page(&app), Some(0));
            let (tx, rx) = std::sync::mpsc::channel();
            app.control_rx = Some(rx);
            let mut ready = false;
            let mut h = Harness::builder().with_size(egui::vec2(800.0, 600.0)).with_step_dt(1.0 / 60.0).build_ui_state(
                move |ui, app: &mut DesignApp| {
                    if !ready {
                        ready = true;
                        return;
                    }
                    let ctx = ui.ctx().clone();
                    app.logic(&ctx); // Actual queue drain before the actual Liquid panel.
                    if !app.fonts_ready {
                        return;
                    }
                    egui::CentralPanel::default().show(ui, |ui| {
                        ui.set_max_width(360.0);
                        liquid(app, ui);
                    });
                },
                app,
            );
            h.input_mut().max_texture_side = Some(8192);
            h.run_steps(8);
            state(&h, &before, &prefs, &ui_prefs);
            let destination = measure(&mut h, "Vertical at 200.0");
            let origin = measure(&mut h, "Vertical at 100.0");
            assert_ne!(origin.id, destination.id);
            assert_eq!(origin.layer_id, destination.layer_id);
            assert!(origin.rect.bottom() <= destination.rect.top());
            state(&h, &before, &prefs, &ui_prefs);
            let p = origin.interact_rect.center();
            let blank = egui::pos2(h.ctx.content_rect().right() - 24.0, p.y);
            assert!(p.distance(blank) > 32.0);
            for r in [&origin, &destination] {
                assert!(!r.interact_rect.expand(8.0).contains(blank));
            }
            blank_guard(&h, blank, origin.layer_id);
            let mut on_a = a.clone();
            on_a.liquid = true;
            let first = literal_edit(&before, vec![on_a, b.clone()], "Convert to Liquid Guide");
            // Expected documents/history are literal, never obtained by running a command.
            // The New Guide Undo snapshot must be the complete intermediate empty document.
            let deleted = match mutation {
                HeldGuideMutation::Delete => literal_edit(&first, vec![b.clone()], "Delete Guide"),
                HeldGuideMutation::DeleteAllThenAdd => {
                    let cleared = literal_edit(&first, vec![], "Delete All Guides on Spread");
                    literal_edit(&cleared, vec![b.clone()], "New Guide")
                }
                HeldGuideMutation::CreateGuidesThenAdd => {
                    let cleared = literal_edit(&first, vec![], "Create Guides…");
                    literal_edit(&cleared, vec![b.clone()], "New Guide")
                }
            };
            let (revision_delta, undo_delta) = match mutation {
                HeldGuideMutation::Delete => (2, 2),
                HeldGuideMutation::DeleteAllThenAdd | HeldGuideMutation::CreateGuidesThenAdd => (3, 3),
            };
            assert_eq!((first.revision, first.history.undo.len(), first.history.redo.len()), (before.revision + 1, before.history.undo.len() + 1, 0));
            assert_eq!(
                (deleted.revision, deleted.history.undo.len(), deleted.history.redo.len()),
                (before.revision + revision_delta, before.history.undo.len() + undo_delta, 0)
            );
            step(&mut h, vec![egui::Event::PointerMoved(p), pointer(p, true)]);
            assert!(h.ctx.input(|i| i.pointer.primary_pressed()
                && i.pointer.primary_down()
                && !i.pointer.primary_released()
                && i.pointer.interact_pos() == Some(p)));
            state(&h, &first, &prefs, &ui_prefs);
            step(&mut h, vec![egui::Event::PointerMoved(blank)]);
            quiet_held(&h, blank);
            assert_eq!(h.get_by_label("Vertical at 100.0").accesskit_node().locate().0, origin.id.accesskit_id());
            assert_eq!(h.ctx.dragged_id(), Some(origin.id), "PREREQUISITE: actual origin capture");
            assert!(h.ctx.read_response(origin.id).unwrap().is_pointer_button_down_on());
            blank_guard(&h, blank, origin.layer_id);
            state(&h, &first, &prefs, &ui_prefs);

            if delete {
                match mutation {
                    HeldGuideMutation::Delete => {
                        let (req, reply) =
                            crate::ControlRequest::new("engine.execute", json!({"command":"guide.delete", "params":{"spread":0,"page":0,"index":0}}));
                        tx.send(req).unwrap();
                        step(&mut h, vec![]);
                        assert_eq!(reply.try_recv().unwrap()["ok"], json!(true), "actual queued delete succeeded");
                    }
                    HeldGuideMutation::DeleteAllThenAdd | HeldGuideMutation::CreateGuidesThenAdd => {
                        let (command, params, clear_result) = if matches!(mutation, HeldGuideMutation::DeleteAllThenAdd) {
                            ("guide.deleteAll", json!({"spread":0}), json!({"deleted":2}))
                        } else {
                            ("layout.createGuides", json!({"rows":0,"columns":0,"removeExisting":true,"spread":0,"page":0}), json!({"guides":0}))
                        };
                        let (clear_req, clear_reply) = crate::ControlRequest::new("engine.execute", json!({"command":command,"params":params}));
                        let (add_req, add_reply) = crate::ControlRequest::new(
                            "engine.execute",
                            json!({
                                "command":"guide.add", "params":{
                                    "spread":0,"page":0,"orientation":"vertical","position":200.0,"spreadGuide":false
                                }
                            }),
                        );
                        // Both requests enter the same real FIFO before ANY next frame.
                        tx.send(clear_req).unwrap();
                        tx.send(add_req).unwrap();
                        step(&mut h, vec![]); // One app.logic drain, then one actual Liquid layout.
                        let cleared = clear_reply.try_recv().expect("clear completed before layout");
                        let added = add_reply.try_recv().expect("replacement add completed in that SAME frame");
                        assert_eq!(cleared["ok"], json!(true), "{mutation:?}: clear succeeded: {cleared:?}");
                        assert_eq!(cleared["result"], clear_result, "{mutation:?}: exact clear result");
                        assert_eq!(added["ok"], json!(true), "{mutation:?}: add succeeded: {added:?}");
                        assert_eq!(added["result"], json!({"page":0,"index":0}), "replacement B is the first guide");
                    }
                }
            } else {
                step(&mut h, vec![]);
            }
            quiet_held(&h, blank);
            blank_guard(&h, blank, origin.layer_id);
            assert_eq!(liquid_guide_checkbox_count(&h), if delete { 1 } else { 2 });
            let current = h.get_by_label(if delete { "Vertical at 200.0" } else { "Vertical at 100.0" });
            assert_eq!(current.accesskit_node().role(), egui::accesskit::Role::CheckBox);
            let current_id = current.accesskit_node().locate().0;
            let rect = current.rect();
            assert_eq!(
                (rect.left(), rect.top()),
                (origin.rect.left(), origin.rect.top()),
                "PREREQUISITE: actual current guide occupies the former first row"
            );
            assert!(h.ctx.content_rect().contains_rect(rect) && !rect.expand(8.0).contains(blank));
            assert_eq!(h.ctx.layer_id_at(rect.center()), Some(origin.layer_id));
            assert!(!h.ctx.interaction_snapshot(|s| s.contains_pointer.iter().any(|id| id.accesskit_id() == current_id)));
            let alias = current_id == origin.id.accesskit_id();
            if alias {
                assert_eq!(h.ctx.dragged_id(), Some(origin.id));
                assert!(h.ctx.read_response(origin.id).unwrap().is_pointer_button_down_on());
            } else {
                assert_ne!(
                    h.ctx.dragged_id().map(|id| id.accesskit_id()),
                    Some(current_id),
                    "a newly identified B must not inherit A's native capture"
                );
            }
            let actual = h.state().session.active().unwrap();
            let guides = &actual.doc.spreads[0].pages[0].guides;
            assert_eq!(guides.iter().map(|g| g.position).collect::<Vec<_>>(), if delete { vec![200.0] } else { vec![100.0, 200.0] });
            eprintln!(
                "mutation={mutation:?} delete={delete} alias={alias} origin={:?} current={current_id:?} liquid={:?} revision={} undo={} redo={}",
                origin.id,
                guides.iter().map(|g| g.liquid).collect::<Vec<_>>(),
                actual.revision,
                actual.history.undo.len(),
                actual.history.redo.len()
            );
            let expected = if delete { &deleted } else { &first };
            // First disputed assertion: only intended conversion/mutation, exact literal B and history.
            state(&h, expected, &prefs, &ui_prefs);
            step(&mut h, vec![]);
            quiet_held(&h, blank);
            state(&h, expected, &prefs, &ui_prefs);
            step(&mut h, vec![pointer(blank, false)]);
            assert!(h.ctx.input(|i| i.pointer.primary_released() && !i.pointer.primary_down()));
            state(&h, expected, &prefs, &ui_prefs);
            for _ in 0..2 {
                step(&mut h, vec![]);
                assert!(!h.ctx.input(|i| i.pointer.any_down()));
                state(&h, expected, &prefs, &ui_prefs);
            }
            eprintln!("completed positive/isolation branch mutation={mutation:?} delete={delete}");
        }
    }

    #[test]
    fn checkbox_drag_in_buttons_panel_survives_name_focus_loss_and_later_name_commit() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = designcraft_doc::ItemId(app.run("frame.create", json!({"rect": [20, 20, 100, 40]})).unwrap()["id"].as_u64().unwrap());
        app.run("selection.set", json!({"ids": [id.0]})).unwrap();
        app.run("form.set", json!({"kind": "checkBox", "name": "Flag", "value": "", "required": false, "fontSize": 11})).unwrap();
        let mut expected = field(&app, id);
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        crate::theme::apply(&ctx, &Tokens::for_brightness(crate::theme::Brightness::default()));
        for _ in 0..3 {
            buttons_frame(&mut app, &ctx, vec![]);
        }
        // Default egui focus surrender happens on completed clicks. Check the
        // value after release and the following focus-loss frames, not just press.
        for target in [true, false] {
            let labels = buttons_frame(&mut app, &ctx, vec![]);
            let name = center(&labels, &expected.name);
            for pressed in [true, false] {
                buttons_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(name), pointer(name, pressed)]);
            }
            assert!(ctx.text_edit_focused(), "Name owns focus before the ordinary checkbox click");
            let labels = buttons_frame(&mut app, &ctx, vec![]);
            let checked = center(&labels, "Checked by default");
            buttons_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(checked), pointer(checked, true)]);
            expected.value = if target { "On".into() } else { String::new() };
            assert_eq!(field(&app, id), expected, "the checkbox command changes value on press");
            buttons_frame(&mut app, &ctx, vec![pointer(checked, false)]);
            for _ in 0..2 {
                buttons_frame(&mut app, &ctx, vec![]);
            }
            assert!(!ctx.text_edit_focused(), "the completed outside click surrenders Name focus");
            assert_eq!(field(&app, id), expected, "completed-click Name commits must not replay the cached checkbox value");
        }

        for target in [true, false] {
            let labels = buttons_frame(&mut app, &ctx, vec![]);
            let name = center(&labels, &expected.name);
            for pressed in [true, false] {
                buttons_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(name), pointer(name, pressed)]);
            }
            assert!(ctx.text_edit_focused(), "the real Name field must own focus before the checkbox press");
            let labels = buttons_frame(&mut app, &ctx, vec![]);
            let checked = center(&labels, "Checked by default");
            let required = center(&labels, "Required");
            buttons_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(checked), pointer(checked, true)]);
            expected.value = if target { "On".into() } else { String::new() };
            assert_eq!(field(&app, id), expected, "the drag origin changes value on press without assuming Name focus loss");
            buttons_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(required)]);
            expected.required = target;
            assert_eq!(field(&app, id), expected);
            for events in [vec![], vec![pointer(required, false)], vec![]] {
                buttons_frame(&mut app, &ctx, events);
                assert_eq!(field(&app, id), expected, "dwell/release keep both command-backed assignments");
            }

            let labels = buttons_frame(&mut app, &ctx, vec![]);
            let name = center(&labels, &expected.name);
            for pressed in [true, false] {
                buttons_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(name), pointer(name, pressed)]);
            }
            assert!(ctx.text_edit_focused());
            buttons_frame(
                &mut app,
                &ctx,
                vec![
                    egui::Event::Key { key: egui::Key::End, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE },
                    egui::Event::Key { key: egui::Key::End, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::NONE },
                    egui::Event::Text(" renamed".into()),
                ],
            );
            assert_eq!(field(&app, id), expected, "Name remains a draft until focus leaves");
            let outside = egui::pos2(620.0, 460.0);
            buttons_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(outside), pointer(outside, true)]);
            assert_eq!(field(&app, id), expected, "default click-based focus loss leaves Name a draft through outside press");
            buttons_frame(&mut app, &ctx, vec![pointer(outside, false)]);
            for _ in 0..2 {
                buttons_frame(&mut app, &ctx, vec![]);
            }
            assert!(!ctx.text_edit_focused());
            expected.name.push_str(" renamed");
            assert_eq!(field(&app, id), expected, "later Name commit preserves On/Required and all unrelated field attributes");
        }
    }
    #[test]
    fn checkbox_held_form_value_cannot_follow_queued_kind_change() {
        use designcraft_doc::{FieldKind, FormField, ItemId};
        use designcraft_engine::{DocState, HistoryEntry};
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };
        use std::sync::Arc;

        fn same(h: &Harness<'_, DesignApp>, e: &DocState, prefs: &Value, ui: &Value) {
            let app = h.state();
            assert_eq!(app.session.documents().len(), 1);
            assert_eq!(app.session.active_index(), Some(0));
            let a = app.session.active().unwrap();
            macro_rules! fields { ($($f:ident),*) => {$(assert_eq!(a.$f, e.$f, stringify!($f));)*} }
            fields!(doc, selection, uid, revision, saved_revision, saved_doc, active_layer, path, editing_parents, preview_stash, preview_record);
            assert_eq!(a.is_dirty(), e.is_dirty());
            assert!(a.interaction.is_none() && e.interaction.is_none());
            assert!(a.fonts.is_none() && e.fonts.is_none());
            assert_eq!((a.history.undo.len(), a.history.redo.len(), a.history.limit), (e.history.undo.len(), e.history.redo.len(), e.history.limit));
            for (x, y) in a.history.undo.iter().chain(&a.history.redo).zip(e.history.undo.iter().chain(&e.history.redo)) {
                assert_eq!((&x.label, x.doc.as_ref(), &x.selection), (&y.label, y.doc.as_ref(), &y.selection));
            }
            assert_eq!(&serde_json::to_value(&app.session.prefs).unwrap(), prefs);
            assert_eq!(&serde_json::to_value(&app.ui).unwrap(), ui);
        }
        fn literal_edit(before: &DocState, id: ItemId, f: FormField) -> DocState {
            let mut e = before.clone();
            Arc::make_mut(&mut e.doc).item_mut(id).unwrap().form_field = Some(f);
            e.revision += 1;
            e.history.undo.push(HistoryEntry { label: "Convert to Form Field".into(), doc: before.doc.clone(), selection: before.selection.clone() });
            e.history.redo.clear();
            e
        }
        fn step(h: &mut Harness<'_, DesignApp>, events: Vec<egui::Event>) {
            assert!(h.input().events.is_empty());
            let frame = h.ctx.cumulative_frame_nr();
            h.input_mut().events.extend(events);
            h.run_steps(1);
            assert_eq!(h.ctx.cumulative_frame_nr(), frame + 1);
        }
        fn occupied(h: &Harness<'_, DesignApp>) -> Vec<egui::Rect> {
            use egui::accesskit::Role;
            h.root()
                .children_recursive()
                .filter(|n| matches!(n.accesskit_node().role(), Role::CheckBox | Role::Button | Role::TextInput | Role::ScrollBar | Role::Label))
                .filter(|n| n.accesskit_node().bounding_box().is_some())
                .map(|n| n.rect())
                .collect()
        }
        fn blank_guard(h: &Harness<'_, DesignApp>, p: egui::Pos2, layer: egui::LayerId) {
            assert!(h.ctx.content_rect().contains(p));
            assert_eq!(h.ctx.layer_id_at(p), Some(layer));
            assert!(h.ctx.memory(|m| m.allows_interaction(layer)));
            assert!(occupied(h).iter().all(|r| !r.expand(8.0).contains(p)), "PREREQUISITE: actual blank outside all controls");
        }
        fn held(h: &Harness<'_, DesignApp>, blank: egui::Pos2) {
            assert!(h.ctx.input(|i| i.pointer.primary_down()
                && !i.pointer.primary_pressed()
                && !i.pointer.primary_released()
                && i.pointer.is_decidedly_dragging()
                && i.pointer.interact_pos() == Some(blank)
                && i.events.is_empty()));
            assert!(!h.ctx.text_edit_focused());
        }

        for switch in [false, true] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.ui.language = "en".into();
            app.run("file.new", json!({"facingPages":false,"primaryTextFrame":false})).unwrap();
            let id = ItemId(app.run("frame.create", json!({"rect":[20,20,100,40]})).unwrap()["id"].as_u64().unwrap());
            app.run("selection.set", json!({"ids":[id.0]})).unwrap();
            app.run(
                "form.set",
                json!({"kind":"checkBox","name":"Flag","value":"",
                "required":false,"multiline":false,"options":["Keep A","Keep B"],"fontSize":11}),
            )
            .unwrap();
            let initial = FormField {
                kind: FieldKind::CheckBox,
                name: "Flag".into(),
                value: "".into(),
                options: vec!["Keep A".into(), "Keep B".into()],
                multiline: false,
                required: false,
                font_size: 11.0,
            };
            assert_eq!(field(&app, id), initial);
            let restored = app.session.active().unwrap().doc.clone();
            app.run("layout.documentSetup", json!({"facingPages":true})).unwrap();
            assert_ne!(app.session.active().unwrap().doc, restored);
            app.run("edit.undo", json!({})).unwrap();
            assert_eq!(app.session.active().unwrap().doc, restored);
            app.ui.docking = None;
            app.ui.floating = vec![("buttons".into(), [60.0, 60.0])];
            let (tx, rx) = std::sync::mpsc::channel();
            app.control_rx = Some(rx);
            let mut ready = false;
            let mut h = Harness::builder().with_size(egui::vec2(1000.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
                move |ui, app: &mut DesignApp| {
                    if !ready {
                        ready = true;
                        return;
                    }
                    let ctx = ui.ctx().clone();
                    app.logic(&ctx);
                    if !app.fonts_ready {
                        return;
                    }
                    egui::CentralPanel::default().show(ui, |_| {});
                    crate::dock::floating(app, &ctx);
                },
                app,
            );
            h.input_mut().max_texture_side = Some(8192);
            h.run_steps(8);
            let before = h.state().session.active().unwrap().clone();
            let prefs = serde_json::to_value(&h.state().session.prefs).unwrap();
            let ui_prefs = serde_json::to_value(&h.state().ui).unwrap();
            assert_eq!(before.selection.items, vec![id]);
            assert!(!before.history.undo.is_empty() && before.history.redo.len() == 1);
            assert!(before.history.undo.len() + 2 <= before.history.limit.max(1));
            assert_eq!(field(h.state(), id), initial);
            assert!(h.state().ui.dialog.is_none() && h.state().ui.palette.is_none());
            assert!(!h.ctx.text_edit_focused());
            let on = FormField { value: "On".into(), ..initial.clone() };
            let signature = FormField { kind: FieldKind::Signature, ..on.clone() };
            let first = literal_edit(&before, id, on.clone());
            let converted = literal_edit(&first, id, signature.clone());
            let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new(("floating_panel", "buttons")));
            let n = h.get_by_label("Checked by default");
            assert_eq!(n.accesskit_node().role(), egui::accesskit::Role::CheckBox);
            let local = n.accesskit_node().locate().0;
            let composite = n.accesskit_node().id();
            let rect = n.rect();
            let required_before = h.get_by_label("Required").rect();
            assert!(rect.bottom() <= required_before.top());
            for r in [rect, required_before] {
                assert!(h.ctx.content_rect().contains_rect(r));
                assert_eq!(h.ctx.layer_id_at(r.center()), Some(layer));
            }
            let p = rect.center();
            step(&mut h, vec![egui::Event::PointerMoved(p)]);
            assert_eq!(h.get_by_label("Checked by default").accesskit_node().id(), composite);
            assert_eq!(h.get_by_label("Checked by default").rect(), rect);
            let origin = h
                .ctx
                .interaction_snapshot(|s| s.hovered.iter().copied().find(|id| id.accesskit_id() == local))
                .expect("PREREQUISITE: measured native checkbox hover");
            let response = h.ctx.read_response(origin).unwrap();
            assert!(response.enabled() && response.interact_rect.contains(p));
            assert_eq!(response.layer_id, layer);
            let area = h.ctx.memory(|m| m.area_rect(layer.id)).unwrap();
            let boxes = occupied(&h);
            let blank = (1..area.width() as usize)
                .map(|dx| egui::pos2(area.left() + dx as f32, p.y))
                .find(|q| p.distance(*q) > 32.0 && h.ctx.layer_id_at(*q) == Some(layer) && boxes.iter().all(|r| !r.expand(8.0).contains(*q)))
                .expect("PREREQUISITE: blank on origin row beyond every control");
            blank_guard(&h, blank, layer);
            assert!(!response.interact_rect.contains(blank));
            same(&h, &before, &prefs, &ui_prefs);
            step(&mut h, vec![egui::Event::PointerMoved(p), pointer(p, true)]);
            assert!(h.ctx.input(|i| i.pointer.primary_pressed()
                && i.pointer.primary_down()
                && !i.pointer.primary_released()
                && i.pointer.interact_pos() == Some(p)));
            assert_eq!(field(h.state(), id), on);
            same(&h, &first, &prefs, &ui_prefs);
            step(&mut h, vec![egui::Event::PointerMoved(blank)]);
            assert!(h.ctx.input(|i| i.pointer.primary_down()
                && i.pointer.is_decidedly_dragging()
                && !i.pointer.primary_pressed()
                && !i.pointer.primary_released()));
            assert_eq!(h.get_by_label("Checked by default").accesskit_node().locate().0, origin.accesskit_id());
            assert_eq!(h.ctx.dragged_id(), Some(origin));
            assert!(h.ctx.read_response(origin).unwrap().is_pointer_button_down_on());
            blank_guard(&h, blank, layer);
            same(&h, &first, &prefs, &ui_prefs);
            let params =
                if switch { json!({"command":"form.set","params":{"kind":"signature"}}) } else { json!({"command":"form.list","params":{}}) };
            let (request, reply) = crate::ControlRequest::new("engine.execute", params);
            tx.send(request).unwrap();
            step(&mut h, vec![]); // Actual app.logic drains queue before panel allocation.
            let r = reply.try_recv().unwrap();
            assert_eq!(r["ok"], json!(true), "{r}");
            if switch {
                assert_eq!(r["result"], json!({"fields":1}));
            } else {
                assert_eq!(
                    r["result"],
                    json!([{"id":id.0,"kind":"checkBox","name":"Flag",
                "value":"On","options":["Keep A","Keep B"]}])
                );
            }
            held(&h, blank);
            if switch {
                assert!(h.query_by_label("Checked by default").is_none());
            }
            let current = h.get_by_label(if switch { "Required" } else { "Checked by default" });
            assert_eq!(current.accesskit_node().role(), egui::accesskit::Role::CheckBox);
            assert!(!current.accesskit_node().is_disabled());
            let current_local = current.accesskit_node().locate().0;
            let current_rect = current.rect();
            assert_eq!(current_rect.min, rect.min, "PREREQUISITE: replacement occupies origin row");
            assert_eq!(current_rect.height(), rect.height());
            assert!(h.ctx.content_rect().contains_rect(current_rect) && !current_rect.contains(blank));
            assert_eq!(h.ctx.layer_id_at(current_rect.center()), Some(layer));
            assert!(!h.ctx.interaction_snapshot(|s| s.contains_pointer.iter().any(|id| id.accesskit_id() == current_local)));
            blank_guard(&h, blank, layer);
            let alias = current_local == origin.accesskit_id();
            eprintln!("switch={switch} alias={alias} origin={origin:?} current={current_local:?}");
            if alias {
                assert_eq!(h.ctx.dragged_id(), Some(origin));
                let response = h.ctx.read_response(origin).unwrap();
                assert!(response.enabled() && response.is_pointer_button_down_on());
                assert_eq!(response.layer_id, layer);
                assert!(!response.interact_rect.contains(blank));
            } else {
                assert_ne!(h.ctx.dragged_id().map(|id| id.accesskit_id()), Some(current_local));
            }
            let expected = if switch { &converted } else { &first };
            let expected_field = if switch { &signature } else { &on };
            assert_eq!(h.state().session.active().unwrap().selection, before.selection);
            // First product oracle after all fixture guards; expected Required is literally false.
            assert_eq!(&field(h.state(), id), expected_field);
            same(&h, expected, &prefs, &ui_prefs);
            for events in [vec![], vec![pointer(blank, false)], vec![], vec![]] {
                step(&mut h, events);
                assert_eq!(&field(h.state(), id), expected_field);
                same(&h, expected, &prefs, &ui_prefs);
            }
        }
    }

    #[test]
    fn checkbox_held_liquid_guide_append_only_create_preserves_sweep() {
        held_liquid_guide_context_case(false, false, false);
    }

    #[test]
    fn checkbox_held_liquid_guide_cannot_follow_queued_page_change() {
        held_liquid_guide_context_case(true, false, false);
        held_liquid_guide_context_case(true, true, false);
    }

    #[test]
    fn checkbox_held_liquid_guide_release_cannot_follow_same_frame_queued_page_change() {
        held_liquid_guide_context_case(true, true, true);
    }

    fn held_liquid_guide_context_case(page_switch: bool, same_position: bool, release_same_frame: bool) {
        use designcraft_doc::{Guide, LiquidRule, Orientation, Selection};
        use designcraft_engine::{DocState, HistoryEntry};
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };
        use std::sync::{Arc, mpsc::Sender};

        fn same(a: &DocState, e: &DocState) {
            macro_rules! fields {
            ($($f:ident),*) => {$(assert_eq!(a.$f, e.$f, stringify!($f));)*}
        }
            fields!(doc, selection, uid, revision, saved_revision, saved_doc, active_layer, path, editing_parents, preview_stash, preview_record);
            assert_eq!(a.is_dirty(), e.is_dirty());
            assert!(a.interaction.is_none() && e.interaction.is_none());
            assert!(a.fonts.is_none() && e.fonts.is_none());
            assert_eq!((a.history.undo.len(), a.history.redo.len(), a.history.limit), (e.history.undo.len(), e.history.redo.len(), e.history.limit));
            for (x, y) in a.history.undo.iter().chain(&a.history.redo).zip(e.history.undo.iter().chain(&e.history.redo)) {
                assert_eq!((&x.label, x.doc.as_ref(), &x.selection), (&y.label, y.doc.as_ref(), &y.selection));
            }
        }
        fn state(h: &Harness<'_, DesignApp>, expected: &DocState, page: usize, prefs: &Value, ui_prefs: &Value) {
            let app = h.state();
            assert_eq!(app.session.documents().len(), 1);
            assert_eq!(app.session.active_index(), Some(0));
            assert_eq!(crate::canvas::current_page(app), Some(page));
            assert_eq!(app.session.active().unwrap().selection, Selection::default());
            same(app.session.active().unwrap(), expected);
            assert_eq!(&serde_json::to_value(&app.session.prefs).unwrap(), prefs);
            assert_eq!(&serde_json::to_value(&app.ui).unwrap(), ui_prefs);
        }
        // No production command/helper computes an expected document or history entry.
        fn literal_edit(before: &DocState, loc: (usize, usize), guides: Vec<Guide>, label: &str) -> DocState {
            let mut e = before.clone();
            let d = Arc::make_mut(&mut e.doc);
            Arc::make_mut(&mut d.spreads[loc.0]).pages[loc.1].guides = guides;
            e.revision += 1;
            e.history.undo.push(HistoryEntry { label: label.into(), doc: before.doc.clone(), selection: before.selection.clone() });
            e.history.redo.clear();
            e
        }
        fn step(h: &mut Harness<'_, DesignApp>, events: Vec<egui::Event>) {
            let frame = h.ctx.cumulative_frame_nr();
            assert!(h.input().events.is_empty());
            h.input_mut().events.extend(events);
            h.run_steps(1);
            assert_eq!(h.ctx.cumulative_frame_nr(), frame + 1);
        }
        fn measure(h: &mut Harness<'_, DesignApp>, label: &str) -> egui::Response {
            let n = h.get_by_label(label);
            assert_eq!(n.accesskit_node().role(), egui::accesskit::Role::CheckBox);
            let local = n.accesskit_node().locate().0;
            let composite = n.accesskit_node().id();
            let rect = n.rect();
            assert!(h.ctx.content_rect().contains_rect(rect));
            let layer = h.ctx.layer_id_at(rect.center()).expect("uncovered actual panel row");
            assert!(!h.ctx.input(|i| i.pointer.any_down()));
            step(h, vec![egui::Event::PointerMoved(rect.center())]);
            assert_eq!(h.get_by_label(label).accesskit_node().id(), composite);
            assert_eq!(h.get_by_label(label).rect(), rect);
            let id = h
                .ctx
                .interaction_snapshot(|s| s.hovered.iter().copied().find(|id| id.accesskit_id() == local))
                .expect("PREREQUISITE: actual checkbox receives hover");
            let r = h.ctx.read_response(id).unwrap();
            assert!(r.enabled() && r.interact_rect.contains(rect.center()));
            assert_eq!(r.layer_id, layer);
            r
        }
        fn blank_guard(h: &Harness<'_, DesignApp>, p: egui::Pos2, layer: egui::LayerId) {
            use egui::accesskit::Role;
            assert!(h.ctx.content_rect().contains(p));
            assert_eq!(h.ctx.layer_id_at(p), Some(layer));
            assert!(
                h.root()
                    .children_recursive()
                    .filter(|n| matches!(n.accesskit_node().role(), Role::CheckBox | Role::Button | Role::TextInput | Role::ScrollBar | Role::Label))
                    .filter(|n| n.accesskit_node().bounding_box().is_some())
                    .all(|n| !n.rect().expand(8.0).contains(p)),
                "PREREQUISITE: actual blank outside controls"
            );
        }
        fn quiet_held(h: &Harness<'_, DesignApp>, blank: egui::Pos2) {
            assert!(h.ctx.input(|i| i.pointer.primary_down()
                && !i.pointer.primary_pressed()
                && !i.pointer.primary_released()
                && i.pointer.is_decidedly_dragging()
                && i.pointer.interact_pos() == Some(blank)
                && !i.events.iter().any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::PointerButton { .. } | egui::Event::Text(_)))));
            assert!(!h.ctx.text_edit_focused());
        }

        fn queued(h: &mut Harness<'_, DesignApp>, tx: &Sender<crate::ControlRequest>, method: &str, params: Value) -> Value {
            let (req, reply) = crate::ControlRequest::new(method, params);
            tx.send(req).unwrap();
            step(h, vec![]);
            let r = reply.try_recv().expect("actual queue completes before Liquid layout");
            assert_eq!(r["ok"], json!(true), "queued {method}: {r:?}");
            r["result"].clone()
        }
        fn diagnostic(h: &Harness<'_, DesignApp>, a: (usize, usize), b: (usize, usize), stage: &str) {
            let st = h.state().session.active().unwrap();
            eprintln!(
                "{stage}: page={:?} A-page={:?} B-page={:?} revision={} undo={} redo={}",
                crate::canvas::current_page(h.state()),
                st.doc.spreads[a.0].pages[a.1].guides.iter().map(|g| g.liquid).collect::<Vec<_>>(),
                st.doc.spreads[b.0].pages[b.1].guides.iter().map(|g| g.liquid).collect::<Vec<_>>(),
                st.revision,
                st.history.undo.len(),
                st.history.redo.len()
            );
        }

        for intervene in [false, true] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.ui.language = "en".into();
            app.run("file.new", json!({"pages":if page_switch {2} else {1},"facingPages":false,"primaryTextFrame":false})).unwrap();
            app.run("style.character.create", json!({"name":"Guide context sentinel","chars":{"fontStyle":"Italic"}})).unwrap();
            app.run("liquid.pageRule", json!({"rule":"guideBased"})).unwrap();
            let a_loc = app.session.active().unwrap().doc.page_loc(0).unwrap();
            let b_loc = app.session.active().unwrap().doc.page_loc(if page_switch { 1 } else { 0 }).unwrap();
            let b_position = if same_position { 100.0 } else { 200.0 };
            for (index, (loc, position)) in [(a_loc, 100.0), (b_loc, b_position)].into_iter().enumerate() {
                assert_eq!(
                    app.run(
                        "guide.add",
                        json!({"spread":loc.0,"page":loc.1,
                    "orientation":"vertical","position":position,"spreadGuide":false})
                    )
                    .unwrap(),
                    json!({"page":loc.1,"index":if page_switch {0} else {index}})
                );
            }
            let layer = app.session.active().unwrap().active_layer;
            let a = Guide { orientation: Orientation::Vertical, position: 100.0, spread: false, locked: false, layer: Some(layer), liquid: false };
            let b = Guide { position: b_position, ..a.clone() };
            let restored = app.session.active().unwrap().clone();
            assert_eq!(restored.doc.page_count(), if page_switch { 2 } else { 1 });
            assert_eq!(restored.selection, Selection::default());
            assert!(a.editable_in(&restored.doc) && b.editable_in(&restored.doc));
            let a_page = &restored.doc.spreads[a_loc.0].pages[a_loc.1];
            let b_page = &restored.doc.spreads[b_loc.0].pages[b_loc.1];
            assert_eq!((a_page.liquid, b_page.liquid), (LiquidRule::GuideBased, LiquidRule::GuideBased));
            if page_switch {
                assert_ne!(a_loc, b_loc);
                assert_ne!(a_page.id, b_page.id, "actual distinct page identity");
                assert_eq!(a_page.guides, vec![a.clone()]);
                assert_eq!(b_page.guides, vec![b.clone()]);
            } else {
                assert_eq!(a_page.guides, vec![a.clone(), b.clone()]);
            }
            app.run("layout.documentSetup", json!({"facingPages":true})).unwrap();
            assert_ne!(app.session.active().unwrap().doc, restored.doc);
            app.run("edit.undo", json!({})).unwrap();
            assert_eq!(app.session.active().unwrap().doc, restored.doc);
            assert_eq!(app.session.active().unwrap().selection, Selection::default());
            let before = app.session.active().unwrap().clone();
            assert!(!before.history.undo.is_empty());
            assert_eq!(before.history.redo.len(), 1);
            let prefs = serde_json::to_value(&app.session.prefs).unwrap();
            let ui_prefs = serde_json::to_value(&app.ui).unwrap();
            app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)));
            crate::canvas::go_to_page(&mut app, 0);
            assert_eq!(crate::canvas::current_page(&app), Some(0));
            let (tx, rx) = std::sync::mpsc::channel();
            app.control_rx = Some(rx);
            let mut ready = false;
            let mut h = Harness::builder().with_size(egui::vec2(800.0, 600.0)).with_step_dt(1.0 / 60.0).build_ui_state(
                move |ui, app: &mut DesignApp| {
                    if !ready {
                        ready = true;
                        return;
                    }
                    let ctx = ui.ctx().clone();
                    app.logic(&ctx);
                    if !app.fonts_ready {
                        return;
                    }
                    egui::CentralPanel::default().show(ui, |ui| {
                        ui.set_max_width(360.0);
                        liquid(app, ui);
                    });
                },
                app,
            );
            h.input_mut().max_texture_side = Some(8192);
            h.run_steps(8);
            state(&h, &before, 0, &prefs, &ui_prefs);
            // Warm B through actual page control, not direct selection/view mutation.
            if page_switch {
                assert_eq!(queued(&mut h, &tx, "ui.set", json!({"page":2})), Value::Null);
                state(&h, &before, 1, &prefs, &ui_prefs);
            }
            let b_label = if same_position { "Vertical at 100.0" } else { "Vertical at 200.0" };
            let destination = measure(&mut h, b_label);
            blank_guard(&h, egui::pos2(h.ctx.content_rect().right() - 24.0, destination.interact_rect.center().y), destination.layer_id);
            state(&h, &before, if page_switch { 1 } else { 0 }, &prefs, &ui_prefs);
            if page_switch {
                assert_eq!(queued(&mut h, &tx, "ui.set", json!({"page":1})), Value::Null);
                state(&h, &before, 0, &prefs, &ui_prefs);
            }
            let origin = measure(&mut h, "Vertical at 100.0");
            assert_eq!(origin.layer_id, destination.layer_id);
            if page_switch {
                assert_eq!(
                    (origin.rect.left(), origin.rect.top()),
                    (destination.rect.left(), destination.rect.top()),
                    "both pages expose their own first guide at the same row"
                );
            } else {
                assert_ne!(origin.id, destination.id);
                assert!(origin.rect.bottom() <= destination.rect.top());
            }
            state(&h, &before, 0, &prefs, &ui_prefs);
            let p = origin.interact_rect.center();
            let blank = egui::pos2(h.ctx.content_rect().right() - 24.0, p.y);
            assert!(p.distance(blank) > 32.0);
            for r in [&origin, &destination] {
                assert!(!r.interact_rect.expand(8.0).contains(blank));
            }
            blank_guard(&h, blank, origin.layer_id);
            let mut on_a = a.clone();
            on_a.liquid = true;
            let first_guides = if page_switch { vec![on_a.clone()] } else { vec![on_a.clone(), b.clone()] };
            let first = literal_edit(&before, a_loc, first_guides.clone(), "Convert to Liquid Guide");
            assert_eq!((first.revision, first.history.undo.len(), first.history.redo.len()), (before.revision + 1, before.history.undo.len() + 1, 0));
            step(&mut h, vec![egui::Event::PointerMoved(p), pointer(p, true)]);
            assert!(h.ctx.input(|i| i.pointer.primary_pressed()
                && i.pointer.primary_down()
                && !i.pointer.primary_released()
                && i.pointer.interact_pos() == Some(p)));
            state(&h, &first, 0, &prefs, &ui_prefs);
            if release_same_frame {
                assert!(page_switch && same_position, "probe requires equal-position guides on two pages");
                assert_ne!(origin.id, destination.id, "distinct measured page-scoped checkbox IDs");
                assert_eq!(origin.rect, destination.rect, "equal labels occupy the same actual rectangle");
                assert_eq!(origin.interact_rect, destination.interact_rect);
                assert_eq!(p, destination.interact_rect.center(), "release is at both measured centers");
                assert_eq!(origin.layer_id, destination.layer_id);
                for r in [&origin, &destination] {
                    assert!(h.ctx.content_rect().contains_rect(r.rect));
                    assert!(r.enabled() && r.interact_rect.contains(p));
                }
                assert_eq!(h.ctx.layer_id_at(p), Some(origin.layer_id));
                assert_eq!(liquid_guide_checkbox_count(&h), 1);
                assert_eq!(h.get_by_label("Vertical at 100.0").accesskit_node().locate().0, origin.id.accesskit_id());
                let held = h.ctx.read_response(origin.id).expect("actual origin response after press");
                assert_eq!(held.rect, origin.rect);
                assert_eq!(held.interact_rect, origin.interact_rect);
                assert_eq!(held.layer_id, origin.layer_id);
                assert!(held.enabled() && held.is_pointer_button_down_on(), "PREREQUISITE: real native press capture");

                // first was constructed from before + a literal single A edit,
                // before dispatching even the press. Never copy the disputed result.
                let expected = first.clone();
                assert_eq!(expected.doc.spreads[b_loc.0].pages[b_loc.1].guides, vec![b.clone()]);
                assert_eq!(expected.revision, before.revision + 1);
                assert_eq!(expected.history.undo.len(), before.history.undo.len() + 1);
                assert!(expected.history.redo.is_empty());
                let page = if intervene { 1 } else { 0 };
                let reply = if intervene {
                    let (request, reply) = crate::ControlRequest::new("ui.set", json!({"page":2}));
                    tx.send(request).unwrap();
                    Some(reply)
                } else {
                    None
                };
                // Exactly the next frame after press. Unlike queued(), this
                // dispatches the pending page request WITH primary release.
                step(&mut h, vec![pointer(p, false)]);
                if let Some(reply) = reply {
                    let result = reply.try_recv().expect("page switch completed in the release frame");
                    assert_eq!(result["ok"], json!(true), "{result:?}");
                    assert_eq!(result["result"], Value::Null, "{result:?}");
                }
                assert!(h.ctx.input(|i| i.pointer.primary_released()
                    && !i.pointer.primary_down()
                    && !i.pointer.primary_pressed()
                    && i.pointer.interact_pos() == Some(p)));
                assert_eq!(crate::canvas::current_page(h.state()), Some(page));
                assert_eq!(h.state().session.active().unwrap().selection, Selection::default());
                assert_eq!(liquid_guide_checkbox_count(&h), 1);
                let warmed = if intervene { &destination } else { &origin };
                let current_node = h.get_by_label("Vertical at 100.0");
                assert_eq!(current_node.accesskit_node().role(), egui::accesskit::Role::CheckBox);
                assert_eq!(current_node.accesskit_node().locate().0, warmed.id.accesskit_id());
                assert_eq!(current_node.rect(), warmed.rect);
                let current = h.ctx.read_response(warmed.id).expect("actual release-frame checkbox response");
                assert_eq!(current.rect, warmed.rect);
                assert_eq!(current.interact_rect, warmed.interact_rect);
                assert_eq!(current.layer_id, warmed.layer_id);
                assert!(current.enabled() && current.interact_rect.contains(p));
                assert!(h.ctx.content_rect().contains_rect(current.rect));
                assert_eq!(h.ctx.layer_id_at(p), Some(current.layer_id));
                // B need not inherit capture or report clicked/contains_pointer:
                // the disputed production branch authorizes the release by geometry.
                eprintln!("same-frame page release intervene={intervene} origin={:?} current={:?}", origin.id, current.id);
                diagnostic(&h, a_loc, b_loc, "same-frame page switch and release");
                state(&h, &expected, page, &prefs, &ui_prefs);

                for events in [vec![], vec![egui::Event::PointerMoved(blank)], vec![]] {
                    step(&mut h, events);
                    assert!(!h.ctx.input(|i| i.pointer.any_down()));
                    state(&h, &expected, page, &prefs, &ui_prefs);
                }
                eprintln!("same-frame page release branch completed intervene={intervene}");
                continue;
            }
            step(&mut h, vec![egui::Event::PointerMoved(blank)]);
            quiet_held(&h, blank);
            assert_eq!(h.get_by_label("Vertical at 100.0").accesskit_node().locate().0, origin.id.accesskit_id());
            assert_eq!(h.ctx.dragged_id(), Some(origin.id));
            assert!(h.ctx.read_response(origin.id).unwrap().is_pointer_button_down_on());
            blank_guard(&h, blank, origin.layer_id);
            state(&h, &first, 0, &prefs, &ui_prefs);

            // Expected before dispatch; no actual-command-derived model oracle.
            let after_command = if intervene && !page_switch { literal_edit(&first, a_loc, first_guides, "Create Guides…") } else { first.clone() };
            let page = if intervene && page_switch { 1 } else { 0 };
            if intervene {
                if page_switch {
                    assert_eq!(queued(&mut h, &tx, "ui.set", json!({"page":2})), Value::Null);
                } else {
                    assert_eq!(
                        queued(
                            &mut h,
                            &tx,
                            "engine.execute",
                            json!({"command":"layout.createGuides",
                        "params":{"rows":0,"columns":0,"removeExisting":false,"spread":a_loc.0,"page":a_loc.1}})
                        ),
                        json!({"guides":0})
                    );
                }
            } else {
                step(&mut h, vec![]);
            }
            quiet_held(&h, blank);
            blank_guard(&h, blank, origin.layer_id);
            assert_eq!(crate::canvas::current_page(h.state()), Some(page), "actual route reached intended page");
            assert_eq!(h.state().session.active().unwrap().selection, Selection::default());
            assert_eq!(liquid_guide_checkbox_count(&h), if page_switch { 1 } else { 2 });
            let (label, warmed) = if page_switch && intervene { (b_label, &destination) } else { ("Vertical at 100.0", &origin) };
            let n = h.get_by_label(label);
            assert_eq!(n.accesskit_node().role(), egui::accesskit::Role::CheckBox);
            assert_eq!(n.accesskit_node().locate().0, warmed.id.accesskit_id());
            assert_eq!(n.rect(), warmed.rect);
            let current = h.ctx.read_response(warmed.id).unwrap();
            assert!(current.enabled() && h.ctx.content_rect().contains_rect(current.rect));
            assert_eq!(h.ctx.layer_id_at(current.rect.center()), Some(origin.layer_id));
            assert_eq!(current.layer_id, origin.layer_id);
            assert!(!current.interact_rect.expand(8.0).contains(blank));
            assert!(!h.ctx.interaction_snapshot(|s| s.contains_pointer.contains(&current.id)));
            let alias = current.id == origin.id;
            if alias {
                assert_eq!(h.ctx.dragged_id(), Some(origin.id));
                assert!(current.is_pointer_button_down_on());
            } else {
                assert!(!current.is_pointer_button_down_on());
            }
            eprintln!("page_switch={page_switch} intervene={intervene} alias={alias} origin={:?} current={:?}", origin.id, current.id);
            diagnostic(&h, a_loc, b_loc, "after queued context operation");
            state(&h, &after_command, page, &prefs, &ui_prefs); // First disputed page-isolation assertion.

            let expected = if page_switch {
                after_command
            } else {
                // Append-only must preserve live sweep: actual held move, no fresh press.
                let mut on_b = b.clone();
                on_b.liquid = true;
                let both = literal_edit(&after_command, a_loc, vec![on_a, on_b], "Convert to Liquid Guide");
                let q = destination.interact_rect.center();
                step(&mut h, vec![egui::Event::PointerMoved(q)]);
                quiet_held(&h, q);
                assert_eq!(h.get_by_label("Vertical at 200.0").accesskit_node().locate().0, destination.id.accesskit_id());
                assert_eq!(h.get_by_label("Vertical at 200.0").rect(), destination.rect);
                let current = h.ctx.read_response(destination.id).unwrap();
                assert!(current.enabled() && current.interact_rect.contains(q) && current.contains_pointer());
                assert_eq!(current.layer_id, origin.layer_id);
                assert_eq!(h.ctx.layer_id_at(q), Some(origin.layer_id));
                assert_eq!(h.ctx.dragged_id(), Some(origin.id));
                assert!(!current.is_pointer_button_down_on());
                assert!(h.ctx.interaction_snapshot(|s| s.contains_pointer.contains(&destination.id)));
                assert_eq!(
                    (both.revision, both.history.undo.len(), both.history.redo.len()),
                    (before.revision + if intervene { 3 } else { 2 }, before.history.undo.len() + if intervene { 3 } else { 2 }, 0)
                );
                diagnostic(&h, a_loc, b_loc, "held sweep onto B");
                state(&h, &both, 0, &prefs, &ui_prefs); // BOTH flags and exact history required.
                step(&mut h, vec![egui::Event::PointerMoved(blank)]);
                both
            };
            quiet_held(&h, blank);
            blank_guard(&h, blank, origin.layer_id);
            state(&h, &expected, page, &prefs, &ui_prefs);
            step(&mut h, vec![]);
            quiet_held(&h, blank);
            state(&h, &expected, page, &prefs, &ui_prefs);
            step(&mut h, vec![pointer(blank, false)]);
            assert!(h.ctx.input(|i| i.pointer.primary_released() && !i.pointer.primary_down()));
            state(&h, &expected, page, &prefs, &ui_prefs);
            for _ in 0..2 {
                step(&mut h, vec![]);
                assert!(!h.ctx.input(|i| i.pointer.any_down()));
                state(&h, &expected, page, &prefs, &ui_prefs);
            }
            eprintln!("context branch completed page_switch={page_switch} intervene={intervene}");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn checkbox_held_liquid_guide_cannot_follow_queued_file_revert() {
        use designcraft_doc::Selection;
        use designcraft_engine::DocState;
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };
        use std::sync::Arc;

        struct TempNative(std::path::PathBuf);
        impl Drop for TempNative {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        fn step(h: &mut Harness<'_, DesignApp>, events: Vec<egui::Event>) {
            let frame = h.ctx.cumulative_frame_nr();
            assert!(h.input().events.is_empty());
            h.input_mut().events.extend(events);
            h.run_steps(1);
            assert_eq!(h.ctx.cumulative_frame_nr(), frame + 1);
        }
        fn quiet_held(h: &Harness<'_, DesignApp>, blank: egui::Pos2) {
            assert!(h.ctx.input(|i| i.pointer.primary_down()
                && !i.pointer.primary_pressed()
                && !i.pointer.primary_released()
                && i.pointer.interact_pos() == Some(blank)
                && !i.events.iter().any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::PointerButton { .. } | egui::Event::Text(_)))));
            assert!(!h.ctx.text_edit_focused());
        }
        fn restored(h: &Harness<'_, DesignApp>, saved: &DocState) {
            assert_eq!(h.state().session.documents().len(), 1);
            assert_eq!(h.state().session.active_index(), Some(0));
            let st = h.state().session.active().unwrap();
            assert_eq!(st.uid, saved.uid);
            assert_eq!(st.selection, Selection::default());
            assert_eq!(st.path, saved.path);
            assert_eq!(st.saved_doc, saved.doc);
            eprintln!(
                "Revert aftermath: liquid={} undo={} redo={} dirty={}",
                st.doc.spreads[0].pages[0].guides[0].liquid,
                st.history.undo.len(),
                st.history.redo.len(),
                st.is_dirty()
            );
            assert_eq!(st.doc, saved.doc, "held origin must not reapply its edit after successful Revert");
            assert!(st.history.undo.is_empty() && st.history.redo.is_empty());
            assert!(!st.is_dirty());
            assert!(st.interaction.is_none());
        }

        let dir = std::env::temp_dir().join(format!(
            "designcraft-cbc-revert-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let temp = TempNative(dir);
        let path = temp.0.join("saved.designcraft");
        let path_text = path.to_str().unwrap();
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".into();
        app.run("file.new", json!({"pages":1,"facingPages":false,"primaryTextFrame":false})).unwrap();
        app.run("liquid.pageRule", json!({"rule":"guideBased","pages":[1]})).unwrap();
        assert_eq!(
            app.run("guide.add", json!({"spread":0,"page":0,"orientation":"vertical","position":100.0,"spreadGuide":false})).unwrap(),
            json!({"page":0,"index":0})
        );
        app.run("file.save", json!({"path":path_text})).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        let saved = app.session.active().unwrap().clone();
        assert_eq!(saved.selection, Selection::default());
        assert!(!saved.is_dirty());
        assert_eq!(saved.doc.spreads[0].pages[0].guides.len(), 1);
        assert!(!saved.doc.spreads[0].pages[0].guides[0].liquid);
        // Literal press oracle: only the saved guide's liquid flag changes.
        let mut held = saved.doc.clone();
        let d = Arc::make_mut(&mut held);
        Arc::make_mut(&mut d.spreads[0]).pages[0].guides[0].liquid = true;
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)));
        crate::canvas::go_to_page(&mut app, 0);
        assert_eq!(crate::canvas::current_page(&app), Some(0));
        let (tx, rx) = std::sync::mpsc::channel();
        app.control_rx = Some(rx);
        let mut ready = false;
        let mut h = Harness::builder().with_size(egui::vec2(800.0, 600.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    ready = true;
                    return;
                }
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                if !app.fonts_ready {
                    return;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.set_max_width(360.0);
                    liquid(app, ui);
                });
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        h.run_steps(8);
        let node = h.get_by_label("Vertical at 100.0");
        assert_eq!(node.accesskit_node().role(), egui::accesskit::Role::CheckBox);
        let local = node.accesskit_node().locate().0;
        let rect = node.rect();
        assert!(h.ctx.content_rect().contains_rect(rect));
        assert!(!h.ctx.input(|i| i.pointer.any_down()));
        step(&mut h, vec![egui::Event::PointerMoved(rect.center())]);
        let id = h
            .ctx
            .interaction_snapshot(|s| s.hovered.iter().copied().find(|id| id.accesskit_id() == local))
            .expect("PREREQUISITE: actual Liquid checkbox receives hover");
        let origin = h.ctx.read_response(id).unwrap();
        let p = origin.interact_rect.center();
        assert!(origin.enabled() && origin.interact_rect.contains(rect.center()));
        let blank = egui::pos2(h.ctx.content_rect().right() - 24.0, p.y);
        assert!(h.ctx.content_rect().contains(blank) && p.distance(blank) > 32.0);
        assert_eq!(h.ctx.layer_id_at(p), Some(origin.layer_id));
        assert_eq!(h.ctx.layer_id_at(blank), Some(origin.layer_id));
        assert!(!origin.interact_rect.expand(8.0).contains(blank));
        assert!(
            h.root()
                .children_recursive()
                .filter(|n| matches!(
                    n.accesskit_node().role(),
                    egui::accesskit::Role::CheckBox
                        | egui::accesskit::Role::Button
                        | egui::accesskit::Role::TextInput
                        | egui::accesskit::Role::ScrollBar
                        | egui::accesskit::Role::Label
                ))
                .filter(|n| n.accesskit_node().bounding_box().is_some())
                .all(|n| !n.rect().expand(8.0).contains(blank)),
            "PREREQUISITE: actual blank outside controls"
        );
        assert_eq!(h.state().session.active().unwrap().doc, saved.doc);
        step(&mut h, vec![egui::Event::PointerMoved(p), pointer(p, true)]);
        assert!(h.ctx.input(|i| i.pointer.primary_pressed() && i.pointer.primary_down() && !i.pointer.primary_released()));
        assert_eq!(h.state().session.active().unwrap().doc, held);
        assert!(h.state().session.active().unwrap().is_dirty());
        step(&mut h, vec![egui::Event::PointerMoved(blank)]);
        quiet_held(&h, blank);
        assert_eq!(h.ctx.dragged_id(), Some(origin.id));
        assert!(h.ctx.read_response(origin.id).unwrap().is_pointer_button_down_on());
        assert_eq!(h.state().session.active().unwrap().doc, held);
        assert_eq!(h.state().session.active().unwrap().history.undo.len(), saved.history.undo.len() + 1);
        assert_eq!(h.state().session.active().unwrap().uid, saved.uid);
        assert_eq!(h.state().session.active().unwrap().selection, Selection::default());

        let (request, reply) = crate::ControlRequest::new("engine.execute", json!({"command":"file.revert","params":{}}));
        tx.send(request).unwrap();
        step(&mut h, vec![]);
        let result = reply.try_recv().expect("actual queued Revert completes before Liquid layout");
        assert_eq!(result["ok"], json!(true), "queued Revert must succeed: {result:?}");
        assert_eq!(result["result"], json!({"path":path_text}));
        quiet_held(&h, blank);
        restored(&h, &saved);
        step(&mut h, vec![]);
        quiet_held(&h, blank);
        restored(&h, &saved);
        step(&mut h, vec![pointer(blank, false)]);
        assert!(h.ctx.input(|i| i.pointer.primary_released() && !i.pointer.primary_down()));
        restored(&h, &saved);
        step(&mut h, vec![]);
        assert!(!h.ctx.input(|i| i.pointer.any_down()));
        restored(&h, &saved);
    }
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
                let key = egui::Id::new("test_fonts");
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
