//! Hyperlinks and Bookmarks panels (Window › Interactive).

use serde_json::{Value, json};

use crate::DesignApp;
use crate::app::file_dialogs::Bound;
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

/// The browser's and an uninstalled runtime's action buttons: real egui controls rather than inert framework
/// components. The desktop's are in [`framework`].
struct InteractiveButton {
    response: egui::Response,
    clicked: bool,
}

fn interactive_button(app: &DesignApp, ui: &mut egui::Ui, target: &str, action: &str, label: &str, enabled: bool) -> InteractiveButton {
    let uid = app.session.active().map(|st| st.uid);
    let id = ui.make_persistent_id(("interactive_action", uid, action, target));
    let label = crate::i18n::tr(&app.ui.language, label);
    let response = ui.push_id(id, |ui| ui.add_enabled(enabled, egui::Button::new(crate::rtl::widget(ui, label)))).inner;
    let clicked = response.clicked();
    InteractiveButton { response, clicked }
}

/// A small row action, its id bound to the document state and the row's target.
fn interactive_compact_button(
    app: &DesignApp,
    ui: &mut egui::Ui,
    target: impl std::hash::Hash + std::fmt::Debug,
    action: &str,
    label: &str,
    enabled: bool,
) -> InteractiveButton {
    // Revert may preserve uid while replacing the document; a held press must not follow it.
    let document = app.session.active().map(|st| (st.uid, st.identity_epoch));
    let id = ui.make_persistent_id(("interactive_compact_action", document, action, target));
    let label = crate::i18n::tr(&app.ui.language, label);
    let response = ui.push_id(id, |ui| ui.add_enabled(enabled, egui::Button::new(crate::rtl::widget(ui, label)).small())).inner;
    let clicked = response.clicked();
    InteractiveButton { response, clicked }
}

fn hyperlink_url(app: &DesignApp, ui: &mut egui::Ui, url: &mut String) {
    let name = crate::i18n::tr(&app.ui.language, "URL");
    let response = ui.add(egui::TextEdit::singleline(url).desired_width(170.0));
    // Name the real fallback editor, not a test-only label or a replacement control. Updating
    // only its semantic name preserves the editor's existing value, role and selection data.
    response.ctx.accesskit_node_builder(response.id, |node| node.set_label(name.to_owned()));
}

fn article_export(app: &DesignApp, ui: &mut egui::Ui, export: &mut bool) -> bool {
    ui.checkbox(export, "")
        .on_hover_ui(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Include when exporting"));
        })
        .changed()
}

/// Window › Interactive › Hyperlinks: a URL and New over the document's hyperlinks; a name goes to its source.
pub fn hyperlinks(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::hyperlinks(app, ui);
        return;
    }
    hyperlinks_fallback(app, ui);
}

/// Window › Interactive › Bookmarks: the PDF outline; a name goes to its page.
pub fn bookmarks(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::bookmarks(app, ui);
        return;
    }
    bookmarks_fallback(app, ui);
}

/// Articles panel: reading-order lists for EPUB / HTML export.
pub fn articles(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::articles(app, ui);
        return;
    }
    articles_fallback(app, ui);
}

/// Tags panel and structure: tags (click to tag the selection), the structure in reading order,
/// XML export/import.
pub fn tags(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::tags(app, ui);
        return;
    }
    tags_fallback(app, ui);
}

fn hyperlinks_fallback(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let id = egui::Id::unique("new_hyperlink_url");
    let mut url: String = ui.data(|d| d.get_temp(id)).unwrap_or_else(|| "https://".into());
    let has_target = app.session.active().is_some_and(|d| d.selection.text.is_some_and(|t| !t.is_caret()) || !d.selection.items.is_empty());
    ui.horizontal(|ui| {
        hyperlink_url(app, ui, &mut url);
        let new = interactive_button(app, ui, "new", "hyperlink.create", "New", has_target);
        new.response.on_hover_ui(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Hyperlink from the selected text or frames"));
        });
        if new.clicked {
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
                if interactive_compact_button(app, ui, ("hyperlink", hid.to_string()), "hyperlink.delete", "Delete", true).clicked {
                    let _ = app.run("hyperlink.delete", json!({"id": hid}));
                }
            });
        });
    }
}

fn bookmarks_fallback(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    if interactive_button(app, ui, "new", "bookmark.add", "New Bookmark for This Page", true).clicked {
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
                // Bookmarks are still deleted by their current displayed index. Name/page bind
                // a held row action to the target available in that existing policy.
                if interactive_compact_button(app, ui, ("bookmark", i, b["name"].as_str(), page), "bookmark.delete", "Delete", true).clicked {
                    let _ = app.run("bookmark.delete", json!({"index": i}));
                }
            });
        });
    }
}

fn articles_fallback(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let has_sel = app.session.active().is_some_and(|d| !d.selection.items.is_empty());
    if interactive_button(app, ui, "new", "article.new", "New Article from Selection", has_sel).clicked {
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
            if article_export(app, ui, &mut export) {
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
                if interactive_compact_button(app, ui, ("article", &name), "article.delete", "Delete", true).clicked {
                    let _ = app.run("article.delete", json!({"name": name}));
                }
                let add = interactive_compact_button(app, ui, ("article", &name), "article.add", "Add", has_sel);
                add.response.on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Add the selection"));
                });
                if add.clicked {
                    let _ = app.run("article.add", json!({"name": name}));
                }
            });
        });
    }
}

/// Independent Tags/XML controls use actual parent/input-measurement readiness.
fn tags_editable(ui: &egui::Ui) -> bool {
    if ui.is_sizing_pass() {
        return false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if craft_ui_widgets::runtime::measure::active(ui.ctx()) {
        return false;
    }
    ui.is_enabled()
}

#[derive(Clone, Default)]
struct TagsWorkflowDraft {
    document: Option<(u64, u64)>,
    name: String,
    problems: Option<Vec<String>>,
}

fn tags_identity(app: &DesignApp) -> Option<(u64, u64, u64)> {
    app.session.active().map(|st| (st.uid, st.identity_epoch, st.revision))
}

fn tags_targets(app: &DesignApp) -> String {
    app.session
        .active()
        .map(|st| {
            json!({
                "items": st.selection.items.iter().map(|id| id.0).collect::<Vec<_>>(),
                "text": st.selection.text.map(|t| json!({"story": t.story.0, "anchor": t.anchor, "focus": t.focus}))
            })
            .to_string()
        })
        .unwrap_or_default()
}

fn tags_action(ui: &mut egui::Ui, id: egui::Id, name: &str, painted: &str, enabled: bool) -> bool {
    let enabled = enabled && tags_editable(ui);
    let response = ui.push_id(id, |ui| ui.add_enabled(enabled, egui::Button::new(crate::rtl::widget(ui, painted)).small())).inner;
    response.ctx.accesskit_node_builder(response.id, |node| node.set_label(name.to_owned()));
    response.on_hover_text(name).clicked() && enabled
}

/// A wrapping navigation action: no single-line elision.
fn tags_structure_action(ui: &mut egui::Ui, id: egui::Id, name: &str, painted: &str, enabled: bool) -> bool {
    let enabled = enabled && tags_editable(ui);
    let response =
        ui.push_id(id, |ui| ui.add_enabled(enabled, egui::Button::selectable(false, egui::RichText::new(painted).size(11.0)).wrap())).inner;
    response.ctx.accesskit_node_builder(response.id, |node| node.set_label(name.to_owned()));
    response.clicked() && enabled
}

fn tags_name_field(app: &DesignApp, ui: &mut egui::Ui, id: egui::Id, draft: &mut String) {
    let enabled = tags_editable(ui);
    let name = crate::i18n::tr(&app.ui.language, "New tag");
    let response = ui.add_enabled(enabled, egui::TextEdit::singleline(draft).id(id).hint_text(crate::rtl::widget(ui, name)).desired_width(140.0));
    response.ctx.accesskit_node_builder(response.id, |node| node.set_label(name.to_owned()));
}

fn tags_markers(app: &DesignApp, ui: &mut egui::Ui, id: egui::Id, on: &mut bool, enabled: bool) -> bool {
    let enabled = enabled && tags_editable(ui);
    let name = crate::i18n::tr(&app.ui.language, "Show Tag Markers");
    ui.push_id(id, |ui| ui.add_enabled(enabled, egui::Checkbox::new(on, crate::rtl::widget(ui, name)))).inner.changed() && enabled
}

fn tags_fallback(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let memory = ui.make_persistent_id("tags-workflow-current-draft");
    let snapshot = tags_identity(app);
    let document = snapshot.map(|(uid, epoch, _)| (uid, epoch));
    let Some(document_key) = document else {
        if tags_editable(ui) {
            ui.data_mut(|d| d.remove::<TagsWorkflowDraft>(memory));
        }
        return;
    };
    let Ok(info) = app.session.execute("xml.tags", &json!({})) else { return };
    let lang = app.ui.language.clone();
    let editable = tags_editable(ui);
    let root = ui.make_persistent_id(("tags-workflow", document_key));
    let targets = tags_targets(app);
    let actions = root.with((snapshot, targets.clone(), info.to_string()));
    let has_sel = app.session.active().is_some_and(|d| !d.selection.items.is_empty());
    // Selected text is tagged inline instead of its frame; a caret alone is not a text range.
    let text_sel = app.session.active().and_then(|d| d.selection.text).is_some_and(|t| !t.range().is_empty());
    let mut draft = ui
        .data(|d| d.get_temp::<TagsWorkflowDraft>(memory))
        .filter(|d| d.document == document)
        .unwrap_or_else(|| TagsWorkflowDraft { document, ..Default::default() });
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(140.0, ui.spacing().interact_size.y), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            tags_name_field(app, ui, root.with("new-name"), &mut draft.name);
        });
        if tags_action(ui, actions.with(("new", draft.name.clone())), crate::i18n::tr(&lang, "New"), crate::i18n::tr(&lang, "New"), editable)
            && !draft.name.trim().is_empty()
            && tags_identity(app) == snapshot
        {
            match app.run("xml.newTag", json!({"name": draft.name.trim()})) {
                Ok(_) => draft.name.clear(),
                Err(e) => app.status_error("Tags", e),
            }
        }
    });
    for tag in info["tags"].as_array().cloned().unwrap_or_default() {
        let n = tag["name"].as_str().unwrap_or("").to_owned();
        let c = tag["color"].as_array().map(|a| a.iter().map(|v| v.as_u64().unwrap_or(0) as u8).collect::<Vec<_>>()).unwrap_or_default();
        let row = actions.with(("tag", tag.to_string()));
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            if c.len() == 3 {
                ui.painter().rect_filled(r, 2.0, egui::Color32::from_rgb(c[0], c[1], c[2]));
            }
            let name = format!("{} <{}>", crate::i18n::tr(&lang, "Tag the selection"), n);
            if ui
                .allocate_ui_with_layout(
                    egui::vec2((ui.available_width() - 80.0 - ui.spacing().item_spacing.x).max(0.0), ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| tags_action(ui, row.with("apply"), &name, &n, editable && (has_sel || text_sel) && tags_identity(app) == snapshot),
                )
                .inner
                && tags_identity(app) == snapshot
                && tags_targets(app) == targets
            {
                let _ = if text_sel { app.run("xml.tagText", json!({"tag": n})) } else { app.run("xml.tag", json!({"tag": n})) };
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let name = format!("{} <{}>", crate::i18n::tr(&lang, "Delete"), n);
                if tags_action(ui, row.with("delete"), &name, crate::i18n::tr(&lang, "Delete"), editable && tags_identity(app) == snapshot)
                    && tags_identity(app) == snapshot
                {
                    let _ = app.run("xml.deleteTag", json!({"name": n}));
                }
            });
        });
    }
    if has_sel || text_sel {
        let label = crate::i18n::tr(&lang, "Untag Selection");
        if tags_action(ui, actions.with("untag"), label, label, editable && tags_identity(app) == snapshot)
            && tags_identity(app) == snapshot
            && tags_targets(app) == targets
        {
            let _ = if text_sel { app.run("xml.tagText", json!({"tag": null})) } else { app.run("xml.tag", json!({"tag": null})) };
        }
    }
    let mut markers = app.ui.tag_markers;
    if tags_markers(app, ui, root.with("markers"), &mut markers, editable && tags_identity(app) == snapshot) && tags_identity(app) == snapshot {
        let _ = app.run("view.tagMarkers", json!({"on": markers}));
    }
    ui.separator();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Structure")).strong());
    let structure = app.session.execute("xml.structure", &json!({})).unwrap_or_default();
    crate::rtl::label(ui, egui::RichText::new(structure["root"].as_str().unwrap_or("Root")).size(11.0).color(t.text_dim));
    let structure_id = actions.with(("structure", structure.to_string()));
    for el in structure["elements"].as_array().cloned().unwrap_or_default() {
        let text: String = el["text"].as_str().unwrap_or("").chars().take(40).collect();
        let painted = format!("  <{}> {}", el["tag"].as_str().unwrap_or(""), text);
        let name = format!("{} {} (Object {})", crate::i18n::tr(&lang, "Structure"), painted.trim_start(), el["id"]);
        if tags_structure_action(ui, structure_id.with(el.to_string()), &name, &painted, editable && tags_identity(app) == snapshot)
            && tags_identity(app) == snapshot
        {
            let _ = app.run("selection.set", json!({"ids": [el["id"]]}));
        }
    }
    // Real validation results persist until validation or the original explicit Load/Delete actions.
    ui.horizontal(|ui| {
        let label = crate::i18n::tr(&lang, "Load DTD…");
        if tags_action(ui, actions.with("load-dtd"), label, label, editable && tags_identity(app) == snapshot) && tags_identity(app) == snapshot {
            draft.problems = None;
            ask_then_run(app, "dtd", "Load DTD", |path| ("xml.loadDtd", json!({"path": path})));
        }
        let has_dtd = info["dtd"].as_bool().unwrap_or(false);
        let label = crate::i18n::tr(&lang, "Validate");
        if tags_action(ui, actions.with("validate"), label, label, editable && has_dtd && tags_identity(app) == snapshot)
            && tags_identity(app) == snapshot
            && let Ok(v) = app.session.execute("xml.validate", &json!({}))
        {
            draft.problems = Some(
                v["problems"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|p| format!("{}: {}", p["path"].as_str().unwrap_or(""), p["message"].as_str().unwrap_or("")))
                    .collect(),
            );
        }
        if has_dtd {
            let label = crate::i18n::tr(&lang, "Delete DTD");
            if tags_action(ui, actions.with("delete-dtd"), label, label, editable && tags_identity(app) == snapshot) && tags_identity(app) == snapshot
            {
                let _ = app.run("xml.deleteDtd", json!({}));
                draft.problems = None;
            }
        }
    });
    match &draft.problems {
        Some(list) if list.is_empty() => {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "The structure is valid.")).size(11.0).color(t.text_dim));
        }
        Some(list) => {
            for p in list {
                crate::rtl::label(ui, egui::RichText::new(format!("⚠ {p}")).size(11.0));
            }
        }
        None => {}
    }
    ui.separator();
    ui.horizontal(|ui| {
        for (key, label) in [("export-xml", "Export XML…"), ("import-xml", "Import XML…")] {
            let label = crate::i18n::tr(&lang, label);
            if tags_action(ui, actions.with(key), label, label, editable && tags_identity(app) == snapshot) && tags_identity(app) == snapshot {
                if key == "export-xml" {
                    let _ = app.run("app.exportXml", json!({}));
                } else {
                    ask_then_run(app, "xml", "Import XML", |path| ("file.importXml", json!({"path": path})));
                }
            }
        }
    });
    if editable && tags_identity(app).map(|(uid, epoch, _)| (uid, epoch)) == document {
        ui.data_mut(|d| d.insert_temp(memory, draft));
    }
}

/// Hyperlinks, Bookmarks, Articles and Tags on the craft framework: the same commands, guards and drafts as the
/// fallbacks above, drawn with framework components only. Ids are scoped by the document state, so a press held
/// over one document's control never lands on another's.
#[cfg(not(target_arch = "wasm32"))]
mod framework {
    use super::{TagsWorkflowDraft, ask_then_run, dest_text, tags_editable, tags_identity, tags_targets};
    use crate::DesignApp;
    use craft_theme::ControlSize;
    use craft_ui_widgets::buttons::{Button, Link};
    use craft_ui_widgets::choice::Checkbox;
    use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _, Sizing};
    use craft_ui_widgets::display::{Divider, Picture, Shade, Text};
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::list::ThumbAspect;
    use craft_ui_widgets::primitives::{HAlign, Wrap};
    use craft_ui_widgets::style::{TypeRole, Variant};
    use craft_ui_widgets::text::TextField;
    use serde_json::{Value, json};

    /// Commands a pass collected, run once it is drawn.
    type Commands = Vec<(&'static str, Value)>;

    fn tr(lang: &str, s: &str) -> String {
        crate::i18n::tr(lang, s).to_owned()
    }

    /// The document state the panel shows: its uid and identity epoch (Revert keeps the uid and renews the epoch).
    fn document(app: &DesignApp) -> Option<(u64, u64)> {
        app.session.active().map(|st| (st.uid, st.identity_epoch))
    }

    /// A panel action; `compact` is the small size of a row's actions.
    fn action(ui: &mut egui::Ui, id: egui::Id, label: String, enabled: bool, compact: bool, tip: Option<String>) -> bool {
        let button = Button::new(label).id(id).enabled(enabled);
        let button = if compact { button.size(ControlSize::Sm) } else { button };
        let button = match tip {
            Some(tip) => button.tooltip(tip),
            None => button,
        };
        button.show(ui).clicked() && enabled
    }

    /// The panel's primary action, bound to the document and its command.
    fn primary(app: &DesignApp, ui: &mut egui::Ui, command: &str, label: &str, enabled: bool, tip: Option<&str>) -> bool {
        let uid = app.session.active().map(|st| st.uid);
        let id = ui.make_persistent_id(("interactive_action", uid, command, "new"));
        let lang = &app.ui.language;
        action(ui, id, tr(lang, label), enabled, false, tip.map(|tip| tr(lang, tip)))
    }

    /// A row's action, bound to the document state, its command and the row's target.
    fn row_action(
        app: &DesignApp,
        ui: &mut egui::Ui,
        target: impl std::hash::Hash + std::fmt::Debug,
        command: &str,
        label: &str,
        enabled: bool,
        tip: Option<&str>,
    ) -> bool {
        let id = ui.make_persistent_id(("interactive_compact_action", document(app), command, target));
        let lang = &app.ui.language;
        action(ui, id, tr(lang, label), enabled, true, tip.map(|tip| tr(lang, tip)))
    }

    /// A row's name: a button at the reading start that takes the row's free width.
    fn name_button(ui: &mut egui::Ui, salt: impl std::hash::Hash + std::fmt::Debug, name: String, tip: String) -> bool {
        Button::new(name).id_salt(salt).align(HAlign::Start).sizing(Sizing::FillWidth).tooltip(tip).show(ui).clicked()
    }

    /// The hint an empty list shows.
    fn empty(ui: &mut egui::Ui, text: String) {
        Text::new(text).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
    }

    fn list(app: &mut DesignApp, command: &str) -> Vec<Value> {
        app.session.execute(command, &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default()
    }

    fn run_all(app: &mut DesignApp, commands: Commands) {
        for (id, params) in commands {
            let _ = app.run(id, params);
        }
    }

    pub(super) fn hyperlinks(app: &mut DesignApp, ui: &mut egui::Ui) {
        let lang = app.ui.language.clone();
        let draft = egui::Id::unique("new_hyperlink_url");
        let mut url: String = ui.data(|d| d.get_temp(draft)).unwrap_or_else(|| "https://".into());
        let has_target = app.session.active().is_some_and(|d| d.selection.text.is_some_and(|t| !t.is_caret()) || !d.selection.items.is_empty());
        let uid = app.session.active().map(|st| st.uid);
        let links = list(app, "hyperlink.list");
        let mut create = false;
        let mut commands = Commands::new();
        Stack::new().id_salt(("hyperlinks", document(app))).fill_width().show(ui, |ui, _| {
            Row::new().id_salt("hyperlink_new").fill_width().show(ui, |ui, _| {
                let id = ui.make_persistent_id(("hyperlink_url", uid));
                let name = tr(&lang, "URL");
                let out = TextField::new(name, Bind::given(url.clone())).id(id).limit(None).sizing(Sizing::FillWidth).show(ui);
                // An immediate UI draft, not a document edit: a typed proposal stays when Escape leaves the field, and
                // only New creates a hyperlink.
                if let Some(value) = out.value() {
                    url = value.clone();
                }
                create = primary(app, ui, "hyperlink.create", "New", has_target, Some("Hyperlink from the selected text or frames"));
            });
            Divider::horizontal().show(ui);
            if links.is_empty() {
                empty(ui, tr(&lang, "No hyperlinks. Select text or a frame, type a URL and click New."));
            }
            for link in &links {
                let target = link["id"].clone();
                Row::new().id_salt(("hyperlink", target.to_string())).fill_width().show(ui, |ui, _| {
                    let name = link["name"].as_str().unwrap_or("").to_owned();
                    if name_button(ui, "hyperlink_source", name, dest_text(&lang, &link["dest"])) {
                        commands.push(("hyperlink.goToSource", json!({"id": target})));
                    }
                    if row_action(app, ui, ("hyperlink", target.to_string()), "hyperlink.delete", "Delete", true, None) {
                        commands.push(("hyperlink.delete", json!({"id": target})));
                    }
                });
            }
        });
        if create {
            let p = if let Some(m) = url.strip_prefix("mailto:") { json!({"email": m}) } else { json!({"url": url}) };
            if let Err(e) = app.run("hyperlink.create", p) {
                app.status_error("Hyperlinks", e);
            }
        }
        run_all(app, commands);
        ui.data_mut(|d| d.insert_temp(draft, url));
    }

    pub(super) fn bookmarks(app: &mut DesignApp, ui: &mut egui::Ui) {
        let lang = app.ui.language.clone();
        let marks = list(app, "bookmark.list");
        let (mut add, mut go, mut delete) = (false, None, None);
        Stack::new().id_salt(("bookmarks", document(app))).fill_width().show(ui, |ui, _| {
            add = primary(app, ui, "bookmark.add", "New Bookmark for This Page", true, None);
            Divider::horizontal().show(ui);
            if marks.is_empty() {
                empty(ui, tr(&lang, "No bookmarks. They become the PDF outline."));
            }
            for (i, mark) in marks.iter().enumerate() {
                let page = mark["page"].as_u64().unwrap_or(0) as usize;
                let name = mark["name"].as_str();
                Row::new().id_salt(("bookmark", i, name, page)).fill_width().show(ui, |ui, _| {
                    let tip = format!("{} {}", tr(&lang, "Page"), app.session.page_label(page));
                    if name_button(ui, "bookmark_name", name.unwrap_or("").to_owned(), tip) {
                        go = Some(page);
                    }
                    // Bookmarks are deleted by their displayed index; the name and page bind a held press to the
                    // bookmark it was pressed on.
                    if row_action(app, ui, ("bookmark", i, name, page), "bookmark.delete", "Delete", true, None) {
                        delete = Some(i);
                    }
                });
            }
        });
        if add {
            let page = crate::canvas::current_page(app).unwrap_or(0) + 1;
            let _ = app.run("bookmark.add", json!({"page": page}));
        }
        if let Some(page) = go {
            crate::canvas::go_to_page(app, page);
        }
        if let Some(index) = delete {
            let _ = app.run("bookmark.delete", json!({"index": index}));
        }
    }

    pub(super) fn articles(app: &mut DesignApp, ui: &mut egui::Ui) {
        let lang = app.ui.language.clone();
        let has_sel = app.session.active().is_some_and(|d| !d.selection.items.is_empty());
        let uid = app.session.active().map(|st| st.uid);
        let all = list(app, "article.list");
        let mut new = false;
        let mut commands = Commands::new();
        Stack::new().id_salt(("articles", document(app))).fill_width().show(ui, |ui, _| {
            new = primary(app, ui, "article.new", "New Article from Selection", has_sel, None);
            Divider::horizontal().show(ui);
            if all.is_empty() {
                empty(ui, tr(&lang, "No articles: exports follow page order."));
            }
            for article in &all {
                let name = article["name"].as_str().unwrap_or("").to_string();
                let export = article["export"].as_bool().unwrap_or(true);
                Row::new().id_salt(("article", &name)).fill_width().show(ui, |ui, _| {
                    let include = tr(&lang, "Include when exporting");
                    let id = ui.make_persistent_id(("article_export", uid, &name));
                    let out = Checkbox::new(Bind::given(export)).id(id).description(include.clone()).tooltip(include.clone()).show(ui);
                    // The box alone, named for its article: the article's name follows it once, as its caption.
                    out.response.ctx.accesskit_node_builder(out.response.id, |node| node.set_label(format!("{include}: {name}")));
                    if let Some(&on) = out.events.committed().filter(|on| **on != export) {
                        commands.push(("article.options", json!({"name": name, "export": on})));
                    }
                    Text::new(name.clone()).role(TypeRole::BodyStrong).show(ui);
                    let count = crate::i18n::count_label(&lang, "objects", article["items"].as_array().map_or(0, Vec::len));
                    Text::new(count).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
                    if row_action(app, ui, ("article", &name), "article.add", "Add", has_sel, Some("Add the selection")) {
                        commands.push(("article.add", json!({"name": name})));
                    }
                    if row_action(app, ui, ("article", &name), "article.delete", "Delete", true, None) {
                        commands.push(("article.delete", json!({"name": name})));
                    }
                });
            }
        });
        if new {
            let _ = app.run("article.new", json!({}));
        }
        run_all(app, commands);
    }

    /// A Tags action: small and quiet, named in full (`name`) while it shows `painted`; `fill` takes the row's free
    /// width. Only an editable panel (enabled, not measuring) takes it.
    fn tag_action(ui: &mut egui::Ui, id: egui::Id, name: &str, painted: &str, enabled: bool, fill: bool) -> bool {
        let enabled = enabled && tags_editable(ui);
        let button = Button::new(name.to_owned())
            .painted(painted.to_owned())
            .id(id)
            .enabled(enabled)
            .size(ControlSize::Sm)
            .variant(Variant::GHOST)
            .tooltip(name.to_owned());
        let button = if fill { button.align(HAlign::Start).sizing(Sizing::FillWidth) } else { button };
        button.show(ui).clicked() && enabled
    }

    pub(super) fn tags(app: &mut DesignApp, ui: &mut egui::Ui) {
        let memory = ui.make_persistent_id("tags-workflow-current-draft");
        let snapshot = tags_identity(app);
        let document = snapshot.map(|(uid, epoch, _)| (uid, epoch));
        let Some(document_key) = document else {
            if tags_editable(ui) {
                ui.data_mut(|d| d.remove::<TagsWorkflowDraft>(memory));
            }
            return;
        };
        let Ok(info) = app.session.execute("xml.tags", &json!({})) else { return };
        let lang = app.ui.language.clone();
        let editable = tags_editable(ui);
        let root = ui.make_persistent_id(("tags-workflow", document_key));
        let targets = tags_targets(app);
        let actions = root.with((snapshot, targets.clone(), info.to_string()));
        let has_sel = app.session.active().is_some_and(|d| !d.selection.items.is_empty());
        // Selected text is tagged inline instead of its frame; a caret alone is not a text range.
        let text_sel = app.session.active().and_then(|d| d.selection.text).is_some_and(|t| !t.range().is_empty());
        let mut draft = ui
            .data(|d| d.get_temp::<TagsWorkflowDraft>(memory))
            .filter(|d| d.document == document)
            .unwrap_or_else(|| TagsWorkflowDraft { document, ..Default::default() });
        Stack::new().id_salt(("tags", document_key)).fill_width().show(ui, |ui, _| {
            Row::new().id_salt("tags_new").fill_width().show(ui, |ui, _| {
                let enabled = tags_editable(ui);
                let name = tr(&lang, "New tag");
                let out = TextField::new(name.clone(), Bind::given(draft.name.clone()))
                    .id(root.with("new-name"))
                    .placeholder(name)
                    .limit(None)
                    .keeps_on_escape()
                    .enabled(enabled)
                    .sizing(Sizing::FillWidth)
                    .show(ui);
                if enabled && let Some(value) = out.value() {
                    draft.name = value.clone();
                }
                let new = tr(&lang, "New");
                if tag_action(ui, actions.with(("new", draft.name.clone())), &new, &new, editable, false)
                    && !draft.name.trim().is_empty()
                    && tags_identity(app) == snapshot
                {
                    match app.run("xml.newTag", json!({"name": draft.name.trim()})) {
                        Ok(_) => draft.name.clear(),
                        Err(e) => app.status_error("Tags", e),
                    }
                }
            });
            for tag in info["tags"].as_array().cloned().unwrap_or_default() {
                let n = tag["name"].as_str().unwrap_or("").to_owned();
                let color = tag["color"].as_array().map(|a| a.iter().map(|v| v.as_u64().unwrap_or(0) as u8).collect::<Vec<_>>()).unwrap_or_default();
                let row = actions.with(("tag", tag.to_string()));
                Row::new().id_salt(("tag", tag.to_string())).fill_width().show(ui, |ui, _| {
                    let chip = (color.len() == 3).then(|| egui::Color32::from_rgb(color[0], color[1], color[2]));
                    Picture::painter("", ThumbAspect::SQUARE, move |cx, rect| {
                        if let Some(chip) = chip {
                            cx.painter.rect_filled(egui::Rect::from_center_size(rect.center(), egui::vec2(12.0, 12.0)), 2.0, chip);
                        }
                    })
                    .id_salt("tag_color")
                    .show(ui);
                    let name = format!("{} <{}>", tr(&lang, "Tag the selection"), n);
                    let ready = editable && (has_sel || text_sel) && tags_identity(app) == snapshot;
                    if tag_action(ui, row.with("apply"), &name, &n, ready, true) && tags_identity(app) == snapshot && tags_targets(app) == targets {
                        let _ = if text_sel { app.run("xml.tagText", json!({"tag": n})) } else { app.run("xml.tag", json!({"tag": n})) };
                    }
                    let name = format!("{} <{}>", tr(&lang, "Delete"), n);
                    if tag_action(ui, row.with("delete"), &name, &tr(&lang, "Delete"), editable && tags_identity(app) == snapshot, false)
                        && tags_identity(app) == snapshot
                    {
                        let _ = app.run("xml.deleteTag", json!({"name": n}));
                    }
                });
            }
            if has_sel || text_sel {
                let label = tr(&lang, "Untag Selection");
                if tag_action(ui, actions.with("untag"), &label, &label, editable && tags_identity(app) == snapshot, false)
                    && tags_identity(app) == snapshot
                    && tags_targets(app) == targets
                {
                    let _ = if text_sel { app.run("xml.tagText", json!({"tag": null})) } else { app.run("xml.tag", json!({"tag": null})) };
                }
            }
            let markers = app.ui.tag_markers;
            let enabled = editable && tags_identity(app) == snapshot && tags_editable(ui);
            let out = Checkbox::new(Bind::given(markers)).id(root.with("markers")).label(tr(&lang, "Show Tag Markers")).enabled(enabled).show(ui);
            if let Some(&on) = out.events.committed().filter(|on| **on != markers)
                && enabled
                && tags_identity(app) == snapshot
            {
                let _ = app.run("view.tagMarkers", json!({"on": on}));
            }
            Divider::horizontal().show(ui);
            Text::new(tr(&lang, "Structure")).role(TypeRole::BodyStrong).show(ui);
            let structure = app.session.execute("xml.structure", &json!({})).unwrap_or_default();
            Text::new(structure["root"].as_str().unwrap_or("Root").to_owned()).role(TypeRole::Caption).shade(Shade::Muted).show(ui);
            let structure_id = actions.with(("structure", structure.to_string()));
            for el in structure["elements"].as_array().cloned().unwrap_or_default() {
                let text: String = el["text"].as_str().unwrap_or("").chars().take(40).collect();
                let painted = format!("<{}> {}", el["tag"].as_str().unwrap_or(""), text);
                let name = format!("{} {} (Object {})", tr(&lang, "Structure"), painted, el["id"]);
                let enabled = editable && tags_identity(app) == snapshot && tags_editable(ui);
                // The whole name wraps rather than elides: two elements whose previews start alike stay apart.
                let clicked = Link::new(name)
                    .id(structure_id.with(el.to_string()))
                    .wrap(Wrap::Wrap)
                    .sizing(Sizing::FillWidth)
                    .enabled(enabled)
                    .show(ui)
                    .clicked();
                if clicked && enabled && tags_identity(app) == snapshot {
                    let _ = app.run("selection.set", json!({"ids": [el["id"]]}));
                }
            }
            // Validation results stay until the next validation or an explicit Load or Delete DTD.
            Row::new().id_salt("tags_dtd").fill_width().wrap(true).show(ui, |ui, _| {
                let label = tr(&lang, "Load DTD…");
                if tag_action(ui, actions.with("load-dtd"), &label, &label, editable && tags_identity(app) == snapshot, false)
                    && tags_identity(app) == snapshot
                {
                    draft.problems = None;
                    ask_then_run(app, "dtd", "Load DTD", |path| ("xml.loadDtd", json!({"path": path})));
                }
                let has_dtd = info["dtd"].as_bool().unwrap_or(false);
                let label = tr(&lang, "Validate");
                if tag_action(ui, actions.with("validate"), &label, &label, editable && has_dtd && tags_identity(app) == snapshot, false)
                    && tags_identity(app) == snapshot
                    && let Ok(v) = app.session.execute("xml.validate", &json!({}))
                {
                    draft.problems = Some(
                        v["problems"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|p| format!("{}: {}", p["path"].as_str().unwrap_or(""), p["message"].as_str().unwrap_or("")))
                            .collect(),
                    );
                }
                if has_dtd {
                    let label = tr(&lang, "Delete DTD");
                    if tag_action(ui, actions.with("delete-dtd"), &label, &label, editable && tags_identity(app) == snapshot, false)
                        && tags_identity(app) == snapshot
                    {
                        let _ = app.run("xml.deleteDtd", json!({}));
                        draft.problems = None;
                    }
                }
            });
            match &draft.problems {
                Some(problems) if problems.is_empty() => {
                    Text::new(tr(&lang, "The structure is valid.")).role(TypeRole::Caption).shade(Shade::Muted).show(ui);
                }
                Some(problems) => {
                    for p in problems {
                        Text::new(format!("⚠ {p}")).role(TypeRole::Caption).sizing(Sizing::FillWidth).show(ui);
                    }
                }
                None => {}
            }
            Divider::horizontal().show(ui);
            Row::new().id_salt("tags_xml").fill_width().wrap(true).show(ui, |ui, _| {
                for (key, label) in [("export-xml", "Export XML…"), ("import-xml", "Import XML…")] {
                    let label = tr(&lang, label);
                    if tag_action(ui, actions.with(key), &label, &label, editable && tags_identity(app) == snapshot, false)
                        && tags_identity(app) == snapshot
                    {
                        if key == "export-xml" {
                            let _ = app.run("app.exportXml", json!({}));
                        } else {
                            ask_then_run(app, "xml", "Import XML", |path| ("file.importXml", json!({"path": path})));
                        }
                    }
                }
            });
        });
        if editable && tags_identity(app).map(|(uid, epoch, _)| (uid, epoch)) == document {
            ui.data_mut(|d| d.insert_temp(memory, draft));
        }
    }
}

/// Asks for a file for `purpose`, then runs the command `command(path)` names; a failure shows
/// in the status line as `feature`'s.
#[track_caller]
fn ask_then_run(app: &mut DesignApp, purpose: &str, feature: &'static str, command: impl FnOnce(String) -> (&'static str, Value) + 'static) {
    let asked = app.ask_open(purpose, Bound::Document, move |app, path| {
        let (id, params) = command(path);
        if let Err(e) = app.run(id, params) {
            app.status_error(feature, e);
        }
        Ok(Value::Null)
    });
    if let Err(e) = asked {
        app.status_error(feature, e);
    }
}

/// Local adapters for the remaining interactive workflow. Never share pending input across
/// documents, replacements, selected target sets or action/form kinds.
struct InteractiveFields {
    identity: egui::Id,
}

impl InteractiveFields {
    fn new(app: &DesignApp, ui: &egui::Ui, mode: impl std::hash::Hash + std::fmt::Debug) -> Self {
        let document = app.session.active().map(|st| (st.uid, st.identity_epoch));
        let targets = app.session.active().map(|st| st.selection.items.iter().map(|id| id.0).collect::<Vec<_>>());
        Self { identity: ui.make_persistent_id(("interactive-fields", document, targets, mode)) }
    }

    fn id(&self, key: &str) -> egui::Id {
        self.identity.with(key)
    }

    /// Whether a pass may edit: never while sizing or measuring, nor under a disabled parent.
    fn editable(ui: &egui::Ui) -> bool {
        if ui.is_sizing_pass() {
            return false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if craft_ui_widgets::runtime::measure::active(ui.ctx()) {
            return false;
        }
        ui.is_enabled()
    }

    fn button(&self, ui: &mut egui::Ui, key: &str, label: &str, enabled: bool, small: bool) -> bool {
        let enabled = enabled && Self::editable(ui);
        ui.push_id(self.id(key), |ui| {
            ui.add_enabled(
                enabled,
                if small { egui::Button::new(crate::rtl::widget(ui, label)).small() } else { egui::Button::new(crate::rtl::widget(ui, label)) },
            )
            .clicked()
        })
        .inner
            && enabled
    }

    fn choice(&self, ui: &mut egui::Ui, key: &str, name: &str, current: usize, options: &[String]) -> Option<usize> {
        let enabled = Self::editable(ui);
        let mut picked = None;
        let combo = craft_ui_legacy::ComboBox::from_id_salt(self.id(key))
            .selected_text(crate::rtl::widget(ui, options.get(current).map_or("", String::as_str)))
            .show_ui(ui, |ui| {
                for (i, label) in options.iter().enumerate() {
                    if ui.selectable_label(i == current, crate::rtl::widget(ui, label)).clicked() && enabled {
                        picked = Some(i);
                    }
                }
            });
        let _ = crate::widgets::name_combo(combo, name);
        picked
    }

    fn text(&self, ui: &mut egui::Ui, key: &str, name: &str, draft: &mut String) -> bool {
        let enabled = Self::editable(ui);
        let response = ui.add_enabled(enabled, egui::TextEdit::singleline(draft).id(self.id(key)).desired_width(ui.available_width()));
        response.ctx.accesskit_node_builder(response.id, |node| node.set_label(name.to_owned()));
        response.lost_focus() && enabled
    }

    fn check(&self, ui: &mut egui::Ui, key: &str, name: &str, value: &mut bool) -> bool {
        let enabled = Self::editable(ui);
        ui.push_id(self.id(key), |ui| ui.add_enabled(enabled, egui::Checkbox::new(value, crate::rtl::widget(ui, name)))).inner.changed() && enabled
    }
}

#[derive(Clone)]
struct InteractiveDraft<T> {
    identity: egui::Id,
    source: T,
    value: T,
}

impl<T: Clone + PartialEq + Send + Sync + 'static> InteractiveDraft<T> {
    fn read(ui: &egui::Ui, key: egui::Id, identity: egui::Id, source: T) -> Self {
        ui.data(|data| data.get_temp::<Self>(key)).filter(|draft| draft.identity == identity && draft.source == source).unwrap_or_else(|| Self {
            identity,
            source: source.clone(),
            value: source,
        })
    }
}

/// The framework forms of the panels below, for a runtime that is installed on a native build.
#[cfg(not(target_arch = "wasm32"))]
#[path = "interactive_b.rs"]
mod framework_b;

/// Object States panel: the selected multi-state object's states (click to show one).
pub fn states(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework_b::states(app, ui);
    }
    states_fallback(app, ui);
}

fn states_fallback(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let n = app.session.active().map_or(0, |d| d.selection.items.len());
    let info = app.session.execute("states.list", &json!({})).ok();
    let list = info.as_ref().and_then(|i| i["states"].as_array().cloned()).unwrap_or_default();
    let controls = InteractiveFields::new(app, ui, ("states", list.iter().map(|v| v.as_str().unwrap_or("")).collect::<Vec<_>>()));
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
        if controls.button(ui, "create", crate::i18n::tr(&app.ui.language, "Convert Selection to Multi-State Object"), n > 0, false)
            && let Err(e) = app.run("states.create", json!({}))
        {
            app.status_error("Object States", e);
        }
        return;
    }
    let active = info.as_ref().and_then(|i| i["active"].as_u64()).unwrap_or(0) as usize;
    let mut picked = None;
    for (i, st) in list.iter().enumerate() {
        let response = ui.push_id(controls.id("state").with(i), |ui| ui.selectable_label(i == active, st.as_str().unwrap_or(""))).inner;
        let name = format!("{}: {} ({})", crate::i18n::tr(&app.ui.language, "Object States"), st.as_str().unwrap_or(""), i + 1);
        response.ctx.accesskit_node_builder(response.id, |node| node.set_label(name));
        if response.clicked() {
            picked = Some(i);
        }
    }
    if let Some(i) = picked.filter(|i| *i < list.len() && InteractiveFields::editable(ui)) {
        let _ = app.run("states.show", json!({"index": i}));
    }
    ui.separator();
    if controls.button(ui, "release", crate::i18n::tr(&app.ui.language, "Release to Objects"), true, true) {
        let _ = app.run("states.release", json!({}));
    }
}

/// Buttons and Forms panel: the selected objects' On Release action.
pub fn buttons(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework_b::buttons(app, ui);
    }
    buttons_fallback(app, ui);
}

fn buttons_fallback(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let url_key = ui.make_persistent_id("interactive-button-url-draft");
    let form_key = ui.make_persistent_id("interactive-form-draft");
    let Some(id) = app.session.active().and_then(|d| d.selection.items.first().copied()) else {
        if InteractiveFields::editable(ui) {
            ui.data_mut(|data| {
                data.remove::<InteractiveDraft<String>>(url_key);
                data.remove::<InteractiveDraft<(String, String, String)>>(form_key);
            });
        }
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
    let controls = InteractiveFields::new(app, ui, ("button", kind));
    let labels = [
        ("none", "Not a button"),
        ("nextPage", "Go To Next Page"),
        ("previousPage", "Go To Previous Page"),
        ("firstPage", "Go To First Page"),
        ("lastPage", "Go To Last Page"),
        ("page", "Go To Page"),
        ("url", "Go To URL"),
    ];
    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "On Release"));
    let options: Vec<_> = labels.iter().map(|(_, label)| crate::i18n::tr(&app.ui.language, label).to_owned()).collect();
    let pick = controls.choice(
        ui,
        "action",
        crate::i18n::tr(&app.ui.language, "On Release Action"),
        labels.iter().position(|l| l.0 == kind).unwrap_or(0),
        &options,
    );
    let mut params =
        pick.and_then(|i| labels.get(i)).map(|(k, _)| json!({"action": k, "page": page, "url": if url.is_empty() { "https://" } else { &url }}));
    if kind == "page" {
        let total = app.session.active().map_or(1, |d| d.doc.page_count());
        let mut n = page + 1;
        ui.horizontal(|ui| {
            let label = crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Page"));
            // Retain the genuine lossless usize page domain and its live DragValue edits.
            let response = ui
                .push_id(controls.id("page"), |ui| ui.add_enabled(InteractiveFields::editable(ui), egui::DragValue::new(&mut n).range(1..=total)))
                .inner;
            response.clone().labelled_by(label.id);
            response
                .ctx
                .accesskit_node_builder(response.id, |node| node.set_label(crate::i18n::tr(&app.ui.language, "Button target page").to_owned()));
            if response.changed() && InteractiveFields::editable(ui) {
                params = Some(json!({"action": "page", "page": n - 1}));
            }
        });
    }
    if kind == "url" {
        let mut draft = InteractiveDraft::read(ui, url_key, controls.identity, url.clone());
        if controls.text(ui, "url", crate::i18n::tr(&app.ui.language, "Button URL"), &mut draft.value) && draft.value != url && pick.is_none() {
            params = Some(json!({"action": "url", "url": draft.value.clone()}));
        }
        if InteractiveFields::editable(ui) {
            ui.data_mut(|data| data.insert_temp(url_key, draft));
        }
    } else if InteractiveFields::editable(ui) {
        ui.data_mut(|data| data.remove::<InteractiveDraft<String>>(url_key));
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
    let controls = InteractiveFields::new(app, ui, ("form", cur.map(|kind| kind as u8)));
    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Form Field"));
    let options: Vec<_> = kinds.iter().map(|(_, label)| crate::i18n::tr(&app.ui.language, label).to_owned()).collect();
    if let Some(i) = controls.choice(
        ui,
        "kind",
        crate::i18n::tr(&app.ui.language, "Form Field Type"),
        kinds.iter().position(|k| k.0 == cur).unwrap_or(0),
        &options,
    ) && let Some((k, _)) = kinds.get(i)
    {
        let _ = match k {
            Some(k) => app.run("form.set", json!({"kind": k})),
            None => app.run("form.clear", json!({})),
        };
        // The old fields may lose focus while the kind is picked. Their pending
        // transaction belongs to the old kind and must not turn the new one back.
        ui.data_mut(|data| data.remove::<InteractiveDraft<(String, String, String)>>(form_key));
        return;
    }
    if let Some(f) = ff {
        let mut draft = InteractiveDraft::read(ui, form_key, controls.identity, (f.name.clone(), f.value.clone(), f.options.join(", ")));
        let (name, value, opts) = &mut draft.value;
        let mut commit = false;
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Name"));
        commit |= controls.text(ui, "name", crate::i18n::tr(&app.ui.language, "Form field name"), name);
        if f.kind == K::CheckBox {
            let mut on = !f.value.is_empty() && f.value != "Off";
            if controls.check(ui, "checked", crate::i18n::tr(&app.ui.language, "Checked by default"), &mut on) {
                let _ = app.run("form.set", json!({"kind": f.kind, "value": if on { "On" } else { "" }}));
            }
        } else if f.kind != K::Signature {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Default"));
            commit |= controls.text(ui, "default", crate::i18n::tr(&app.ui.language, "Form field default value"), value);
        }
        if matches!(f.kind, K::ComboBox | K::ListBox) {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Choices"));
            commit |= controls.text(ui, "choices", crate::i18n::tr(&app.ui.language, "Form field choices (separated by commas)"), opts);
        }
        let mut req = f.required;
        if controls.check(ui, "required", crate::i18n::tr(&app.ui.language, "Required"), &mut req) {
            let _ = app.run("form.set", json!({"kind": f.kind, "required": req}));
        }
        if f.kind == K::TextField {
            let mut ml = f.multiline;
            if controls.check(ui, "multiline", crate::i18n::tr(&app.ui.language, "Multiline"), &mut ml) {
                let _ = app.run("form.set", json!({"kind": f.kind, "multiline": ml}));
            }
        }
        if commit {
            let options: Vec<String> = opts.split(',').map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
            let _ = app.run("form.set", json!({"kind": f.kind, "name": name, "value": value, "options": options}));
            ui.data_mut(|data| data.remove::<InteractiveDraft<(String, String, String)>>(form_key));
        } else if InteractiveFields::editable(ui) {
            ui.data_mut(|data| data.insert_temp(form_key, draft));
        }
    } else if InteractiveFields::editable(ui) {
        ui.data_mut(|data| data.remove::<InteractiveDraft<(String, String, String)>>(form_key));
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

fn liquid_check(ui: &mut egui::Ui, id: egui::Id, name: &str, value: &mut bool, enabled: bool) -> bool {
    let enabled = enabled && InteractiveFields::editable(ui);
    ui.push_id(id, |ui| ui.add_enabled(enabled, egui::Checkbox::new(value, crate::rtl::widget(ui, name)))).inner.changed() && enabled
}

fn liquid_choice(ui: &mut egui::Ui, id: egui::Id, rule: designcraft_doc::LiquidRule, lang: &str) -> Option<designcraft_doc::LiquidRule> {
    let enabled = InteractiveFields::editable(ui);
    let mut pick = None;
    let combo = craft_ui_legacy::ComboBox::from_id_salt(id)
        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, liquid_rule_label(rule))))
        .width(190.0)
        .show_ui(ui, |ui| {
            for (r, label) in LIQUID_RULES {
                if ui.selectable_label(rule == r, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() && enabled && r != rule {
                    pick = Some(r);
                }
            }
        });
    let _ = crate::widgets::name_combo(combo, crate::i18n::tr(lang, "Liquid Page Rule"));
    pick
}

/// Liquid Layout panel, laid out as InDesign's: the page's Liquid Page Rule (with "Page: n"),
/// Content › Auto-Fit, and Object Constraints › Resize with Page (Height, Width) and Pin (Top,
/// Left, Bottom, Right) for the selected object on an object-based page. The page's liquid
/// guides follow (Create Alternate Layout is in the Layout menu).
pub fn liquid(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework_b::liquid(app, ui);
    }
    liquid_fallback(app, ui);
}

fn liquid_fallback(app: &mut DesignApp, ui: &mut egui::Ui) {
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
    let document = app.session.active().map(|st| (st.uid, st.identity_epoch));
    let targets = app.session.active().map(|st| st.selection.items.iter().map(|id| id.0).collect::<Vec<_>>());
    let inherited = d.parent_page_for(page).and_then(|(si, pi)| d.parents.get(si)?.pages.get(pi).map(|pg| (pg.id.0, pg.liquid as u8)));
    // Page IDs survive reordering; indices still bind the exact existing command payload.
    let identity = ui.make_persistent_id(("liquid-workflow", document, (page, si, pi, pg.id.0), targets, pg.liquid as u8, inherited));
    if let Some(rule) = liquid_choice(ui, identity.with("rule"), pg.liquid, &lang) {
        let _ = app.run("liquid.pageRule", json!({"rule": rule, "pages": [page + 1]}));
    }
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
        if liquid_check(ui, identity.with("auto-fit"), crate::i18n::tr(&lang, "Auto-Fit"), &mut auto, graphic.is_some()) {
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
                            if liquid_check(ui, identity.with(("constraint", key)), crate::i18n::tr(&lang, label), &mut v, on) {
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
        // Guides have no persistent ID. Bind to the complete structural row sequence,
        // raw coordinates and page identity, not a rounded caption or a shifted index.
        let rows: Vec<_> =
            pg.guides.iter().map(|g| (g.orientation as u8, g.position.to_bits(), g.spread, g.locked, g.layer.map(|id| id.0))).collect();
        let guide_identity = identity.with(("guides", rows));
        for (gi, g) in pg.guides.iter().enumerate() {
            let mut on = g.liquid;
            let label = format!(
                "{} {}: {:?} at {} ({} {})",
                crate::i18n::tr(&lang, "Liquid guide"),
                gi + 1,
                g.orientation,
                g.position,
                crate::i18n::tr(&lang, "Page"),
                d.page_name(page)
            );
            if liquid_check(ui, guide_identity.with(gi), &label, &mut on, true) {
                let _ = app.run("guide.liquid", json!({"spread": si, "page": pi, "index": gi, "on": on}));
            }
        }
    }
}

/// Media panel: the selected video or sound's playback options and poster.
pub fn media(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework_b::media(app, ui);
    }
    media_fallback(app, ui);
}

fn media_fallback(app: &mut DesignApp, ui: &mut egui::Ui) {
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
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Choose Image…"))).clicked() {
            ask_then_run(app, "place", "Media", |path| ("media.options", json!({"poster": {"path": path}})));
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
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework_b::transitions(app, ui);
    }
    transitions_fallback(app, ui);
}

fn transitions_fallback(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(page) = crate::canvas::current_page(app) else { return };
    let Some(d) = app.session.active().map(|d| d.doc.clone()) else { return };
    let Some((si, _)) = d.page_loc(page) else { return };
    let cur = d.spreads[si].pages.first().and_then(|p| p.transition);
    use designcraft_doc::TransitionKind as K;
    crate::rtl::label(ui, egui::RichText::new(format!("{} {}", crate::i18n::tr(&app.ui.language, "Spread"), si + 1)).strong());
    let mut kind = cur.map(|c| c.kind);
    let combo = craft_ui_legacy::ComboBox::from_id_salt("transition_kind")
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

fn track_identity(app: &DesignApp) -> Option<(u64, u64, u64)> {
    app.session.active().map(|st| (st.uid, st.identity_epoch, st.revision))
}

fn track_action(ui: &mut egui::Ui, id: egui::Id, name: &str, painted: &str, enabled: bool) -> bool {
    let enabled = enabled && InteractiveFields::editable(ui);
    let response = ui.push_id(id, |ui| ui.add_enabled(enabled, egui::Button::new(crate::rtl::widget(ui, painted)).small())).inner;
    response.ctx.accesskit_node_builder(response.id, |node| node.set_label(name.to_owned()));
    response.clicked() && enabled
}

fn track_row_name(action: &str, change: &Value) -> String {
    let preview: String = change["text"].as_str().unwrap_or("").chars().take(40).collect();
    format!(
        "{} {} change in story {} [{}..{}]: {}",
        action,
        change["kind"].as_str().unwrap_or(""),
        change["story"],
        change["start"],
        change["end"],
        preview.replace('\n', "¶")
    )
}

/// Track Changes panel: tracking on/off and each change with Accept / Reject.
pub fn track_changes(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        return framework_b::track_changes(app, ui);
    }
    track_changes_fallback(app, ui);
}

fn track_changes_fallback(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(on) = app.session.active().map(|d| d.doc.settings.track_changes) else { return };
    let lang = app.ui.language.clone();
    let identity = track_identity(app);
    let root = ui.make_persistent_id(("track-changes-workflow", identity));
    let mut track = on;
    let editable = InteractiveFields::editable(ui);
    let changed = ui
        .push_id(root.with("track"), |ui| {
            ui.add_enabled(editable, egui::Checkbox::new(&mut track, crate::rtl::widget(ui, crate::i18n::tr(&lang, "Track Changes in All Stories"))))
        })
        .inner
        .changed()
        && editable;
    if changed && track_identity(app) == identity {
        let _ = app.run("changes.track", json!({"on": track}));
    }
    let list = app.session.execute("changes.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let current = track_identity(app);
    let collection = Value::Array(list.clone()).to_string();
    // Full collection + document/replacement/revision, never a preview or row index.
    let actions = ui.make_persistent_id(("track-changes-actions", current, collection));
    ui.separator();
    if list.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "No changes.")).size(11.0).color(t.text_dim));
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt(ui.make_persistent_id(("track-scroll", current.map(|(uid, epoch, _)| (uid, epoch)))))
        .max_height(260.0)
        .show(ui, |ui| {
            for c in &list {
                let (story, start) = (c["story"].clone(), c["start"].clone());
                let kind = c["kind"].as_str().unwrap_or("");
                let text: String = c["text"].as_str().unwrap_or("").chars().take(40).collect();
                let row = actions.with(("change", c.to_string()));
                let label = egui::RichText::new(format!(
                    "{} “{}”",
                    crate::i18n::tr(&lang, if kind == "inserted" { "Added" } else { "Deleted" }),
                    text.replace('\n', "¶")
                ))
                .size(11.0);
                let label = if kind == "deleted" { label.strikethrough() } else { label };
                let ready = editable && track_identity(app) == current;
                let excerpt = ui.push_id(row.with("show"), |ui| ui.add_enabled(ready, egui::Button::selectable(false, label).wrap())).inner;
                excerpt.ctx.accesskit_node_builder(excerpt.id, |node| node.set_label(track_row_name(crate::i18n::tr(&lang, "Show in the text"), c)));
                let excerpt = excerpt.on_hover_text(crate::i18n::tr(&lang, "Show in the text"));
                if excerpt.clicked() && ready && track_identity(app) == current {
                    let _ = app.run("text.select", json!({"story": story, "anchor": start, "focus": c["end"].clone()}));
                }
                // Keep all three actions inside the narrow parent instead of overflowing
                // its right edge with a forty-scalar excerpt plus two button labels.
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for (command, label, key) in [("changes.reject", "Reject", "reject"), ("changes.accept", "Accept", "accept")] {
                            let enabled = editable && track_identity(app) == current;
                            if track_action(
                                ui,
                                row.with(key),
                                &track_row_name(crate::i18n::tr(&lang, label), c),
                                crate::i18n::tr(&lang, label),
                                enabled,
                            ) && track_identity(app) == current
                            {
                                let _ = app.run(command, json!({"story": story, "start": start}));
                            }
                        }
                    });
                });
            }
        });
    ui.separator();
    ui.horizontal(|ui| {
        for (command, label, key) in [("changes.acceptAll", "Accept All", "accept-all"), ("changes.rejectAll", "Reject All", "reject-all")] {
            let enabled = editable && track_identity(app) == current;
            if track_action(ui, actions.with(key), crate::i18n::tr(&lang, label), crate::i18n::tr(&lang, label), enabled)
                && track_identity(app) == current
            {
                let _ = app.run(command, json!({}));
            }
        }
    });
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "interactive_tests_b.rs"]
mod framework_tests_b;

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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui223_interactive_actions_tests {
    use super::*;
    use designcraft_doc::{HyperlinkDest, HyperlinkSource, ItemId, StoryId};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{Harness, kittest::Queryable};
    use std::sync::Arc;

    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"title": "Interactive", "pages": 3})).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::pos2(500.0, 0.0), egui::vec2(600.0, 700.0)));
        app
    }

    fn frame_item(app: &mut DesignApp, x: i32) -> ItemId {
        ItemId(app.run("frame.create", json!({"rect": [x, 72, x + 72, 144]})).unwrap()["id"].as_u64().unwrap())
    }

    fn region() -> egui::Id {
        egui::Id::NULL.with("ui223_interactive_actions_region")
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
        assert!(
            !face.is_placeholder() && face.covers('\u{0627}') && face.covers('\u{0625}'),
            "the Arabic face must contain real U+0627 and U+0625 glyphs"
        );
        for key in [
            "URL",
            "New",
            "Hyperlink from the selected text or frames",
            "No hyperlinks. Select text or a frame, type a URL and click New.",
            "Delete",
            "New Bookmark for This Page",
            "Page",
            "No bookmarks. They become the PDF outline.",
            "New Article from Selection",
            "No articles: exports follow page order.",
            "Include when exporting",
            "Add",
            "Add the selection",
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
            "Interactive Arabic fixture: real existing {} {}, face {}, {} font bytes, explicit Interactive alphabetic-glyph coverage checked",
            face.family,
            face.style,
            face.index(),
            face.data().len()
        );
    }

    fn harness(mut app: DesignApp, panel: fn(&mut DesignApp, &mut egui::Ui), sdk: bool) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        if sdk {
            app.framework_opacity = true;
            app.install_framework(&ctx);
            assert!(app.framework_installed);
        }
        let mut ready = false;
        let mut locale_fonts_ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(500.0, 700.0)).build_ui_state(
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
                let parent = ui.max_rect();
                ui.data_mut(|data| data.insert_temp(region().with("actual_parent"), parent));
                let _ = craft_ui_widgets::runtime::inspect::record(ui, region(), |ui| panel(app, ui));
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 3);
        h
    }

    fn input(h: &mut Harness<'_, DesignApp>, event: Option<Event>) {
        let mut raw = std::mem::take(h.input_mut());
        if let Some(event) = event {
            raw.events.push(event);
        }
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    fn idle(h: &mut Harness<'_, DesignApp>, n: usize) {
        for _ in 0..n {
            input(h, None);
        }
    }

    fn click_rect(h: &mut Harness<'_, DesignApp>, rect: egui::Rect) {
        let pos = rect.center();
        input(h, Some(Event::PointerMoved(pos)));
        for pressed in [true, false] {
            input(h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
        }
        idle(h, 3);
    }

    fn click(h: &mut Harness<'_, DesignApp>, label: &str) {
        let rect = h.get_by_label(label).rect();
        click_rect(h, rect);
    }

    fn key_mod(h: &mut Harness<'_, DesignApp>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            input(h, Some(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }));
        }
        input(h, Some(Event::ModifiersChanged(Modifiers::NONE)));
        idle(h, 2);
    }

    fn key(h: &mut Harness<'_, DesignApp>, key: Key) {
        key_mod(h, key, Modifiers::NONE);
    }

    fn type_url(h: &mut Harness<'_, DesignApp>, value: &str) {
        let name = crate::i18n::tr(&h.state().ui.language, "URL");
        let rect = h.get_by_role_and_label(egui::accesskit::Role::TextInput, name).rect();
        click_rect(h, rect);
        key_mod(h, Key::A, Modifiers::COMMAND);
        input(h, Some(Event::Text(value.into())));
        idle(h, 2);
    }

    fn sdk_button(h: &Harness<'_, DesignApp>, label: &str) -> craft_ui_widgets::runtime::inspect::Element {
        craft_ui_widgets::runtime::inspect::recorded(&h.ctx, region())
            .unwrap()
            .elements
            .iter()
            .find(|e| e.component == "Button" && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(label))
            .unwrap()
            .clone()
    }

    fn export_checkbox(h: &Harness<'_, DesignApp>, name: &str) -> craft_ui_widgets::runtime::inspect::Element {
        let y = h.get_by_label(name).rect().center().y;
        craft_ui_widgets::runtime::inspect::recorded(&h.ctx, region())
            .unwrap()
            .elements
            .iter()
            .find(|e| e.component == "Checkbox" && e.rect.y_range().contains(y))
            .unwrap()
            .clone()
    }

    fn undo(app: &DesignApp) -> usize {
        app.session.active().unwrap().history.undo.len()
    }
    fn count(app: &DesignApp, command: &str) -> usize {
        app.session.journal.iter().filter(|(id, _)| id == command).count()
    }
    fn last(app: &DesignApp, command: &str, payload: Value) {
        assert_eq!(app.session.journal.last(), Some(&(command.to_owned(), payload)));
    }
    fn no_idle_replay(h: &mut Harness<'_, DesignApp>) {
        let journal = h.state().session.journal.clone();
        let steps = undo(h.state());
        idle(h, 5);
        assert_eq!(h.state().session.journal, journal);
        assert_eq!(undo(h.state()), steps);
    }

    #[test]
    fn ui223_interactive_hyperlink_frame_url_and_correct_duplicate_delete_undo() {
        let mut app = app();
        let item = frame_item(&mut app, 72);
        app.run("selection.set", json!({"ids": [item.0]})).unwrap();
        let mut h = harness(app, hyperlinks, true);
        let steps = undo(h.state());
        type_url(&mut h, "https://example.com/layout?q=a%20b#part");
        assert_eq!(undo(h.state()), steps, "typing edits only the UI draft");
        let new = sdk_button(&h, "New");
        click_rect(&mut h, new.rect);
        last(h.state(), "hyperlink.create", json!({"url": "https://example.com/layout?q=a%20b#part"}));
        let first = h.state().session.active().unwrap().doc.hyperlinks[0].id;
        assert!(matches!(h.state().session.active().unwrap().doc.hyperlinks[0].source, HyperlinkSource::Item { id } if id == item));
        assert_eq!(undo(h.state()), steps + 1);
        no_idle_replay(&mut h);
        click(&mut h, "New");
        let second = h.state().session.active().unwrap().doc.hyperlinks[1].id;
        assert_ne!(first, second);
        // Existing selectable source navigation remains the app's, not an SDK list rewrite.
        frame_item(h.state_mut(), 300);
        idle(&mut h, 2);
        let source = h.get_all_by_label("https://example.com/layout?q=a%20b#part").next().unwrap().rect();
        click_rect(&mut h, source);
        last(h.state(), "hyperlink.goToSource", json!({"id": first}));
        assert_eq!(h.state().session.active().unwrap().selection.items, vec![item]);
        let rect = h.get_all_by_label("Delete").nth(1).unwrap().rect();
        let steps = undo(h.state());
        click_rect(&mut h, rect);
        last(h.state(), "hyperlink.delete", json!({"id": second}));
        let links = &h.state().session.active().unwrap().doc.hyperlinks;
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, first);
        assert_eq!(undo(h.state()), steps + 1);
        no_idle_replay(&mut h);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.active().unwrap().doc.hyperlinks[1].id, second);
    }

    #[test]
    fn ui223_interactive_hyperlink_text_mailto_natural_tab_enter_once() {
        let mut app = app();
        let made = app.run("frame.create", json!({"rect": [72, 72, 300, 140], "content": "text", "text": "Read this text"})).unwrap();
        let story = StoryId(made["story"].as_u64().unwrap());
        app.run("text.select", json!({"story": story.0, "anchor": 0, "focus": 4})).unwrap();
        let mut h = harness(app, hyperlinks, true);
        type_url(&mut h, "mailto:Case+layout@example.com?subject=Hello%20there");
        let new = sdk_button(&h, "New");
        for _ in 0..8 {
            key(&mut h, Key::Tab);
            if h.ctx.memory(|m| m.focused()) == Some(new.id) {
                break;
            }
        }
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(new.id), "natural Tab reaches the real New button");
        let steps = undo(h.state());
        let commands = count(h.state(), "hyperlink.create");
        key(&mut h, Key::Enter);
        last(h.state(), "hyperlink.create", json!({"email": "Case+layout@example.com?subject=Hello%20there"}));
        let doc = &h.state().session.active().unwrap().doc;
        assert_eq!(doc.hyperlinks.len(), 1);
        assert!(matches!(doc.hyperlinks[0].source, HyperlinkSource::Text { story: sid, start: 0, end: 4 } if sid == story));
        assert!(matches!(&doc.hyperlinks[0].dest, HyperlinkDest::Email(email) if email == "Case+layout@example.com?subject=Hello%20there"));
        assert_eq!(undo(h.state()), steps + 1);
        assert_eq!(count(h.state(), "hyperlink.create"), commands + 1);
        no_idle_replay(&mut h);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert!(h.state().session.active().unwrap().doc.hyperlinks.is_empty());
    }

    #[test]
    fn ui223_interactive_hyperlink_disabled_and_refused_url_do_not_edit() {
        let mut h = harness(app(), hyperlinks, true);
        let new = sdk_button(&h, "New");
        assert!(new.facts.as_ref().unwrap().disabled);
        let journal = h.state().session.journal.clone();
        click_rect(&mut h, new.rect);
        assert_eq!(h.state().session.journal, journal);
        let item = frame_item(h.state_mut(), 72);
        h.state_mut().run("selection.set", json!({"ids": [item.0]})).unwrap();
        idle(&mut h, 2);
        type_url(&mut h, "javascript:alert(1)");
        let before = Arc::clone(&h.state().session.active().unwrap().doc);
        let steps = undo(h.state());
        click(&mut h, "New");
        assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &before));
        assert_eq!(undo(h.state()), steps);
        assert!(h.state().ui.status.contains("`url` must start with"));
        assert!(h.state().session.active().unwrap().doc.hyperlinks.is_empty());
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_interactive_url_draft_escape_and_long_address_are_preserved() {
        let mut app = app();
        frame_item(&mut app, 72);
        let mut h = harness(app, hyperlinks, true);
        let value = format!("https://example.com/{}", "a".repeat(1200));
        type_url(&mut h, &value);
        let steps = undo(h.state());
        key(&mut h, Key::Escape);
        let draft: String = h.ctx.data(|d| d.get_temp(egui::Id::unique("new_hyperlink_url"))).unwrap();
        assert_eq!(draft, value, "Escape does not roll back the immediate UI draft or truncate it at 1024 characters");
        assert_eq!(undo(h.state()), steps);
        click(&mut h, "New");
        last(h.state(), "hyperlink.create", json!({"url": value}));
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_interactive_bookmark_current_page_navigation_and_index_delete() {
        let mut app = app();
        crate::canvas::go_to_page(&mut app, 1);
        assert_eq!(crate::canvas::current_page(&app), Some(1));
        let mut h = harness(app, bookmarks, true);
        let steps = undo(h.state());
        let new = sdk_button(&h, "New Bookmark for This Page");
        click_rect(&mut h, new.rect);
        last(h.state(), "bookmark.add", json!({"page": 2}));
        assert_eq!(h.state().session.active().unwrap().doc.bookmarks[0].page, 1);
        assert_eq!(undo(h.state()), steps + 1);
        crate::canvas::go_to_page(h.state_mut(), 2);
        click(&mut h, "New Bookmark for This Page");
        assert_eq!(h.state().session.active().unwrap().doc.bookmarks[1].page, 2);
        crate::canvas::go_to_page(h.state_mut(), 0);
        let steps = undo(h.state());
        click(&mut h, "Bookmark 1");
        assert_eq!(crate::canvas::current_page(h.state()), Some(1));
        assert_eq!(undo(h.state()), steps, "navigation is not an edit");
        let rect = h.get_all_by_label("Delete").nth(1).unwrap().rect();
        click_rect(&mut h, rect);
        last(h.state(), "bookmark.delete", json!({"index": 1}));
        assert_eq!(h.state().session.active().unwrap().doc.bookmarks.len(), 1);
        assert_eq!(h.state().session.active().unwrap().doc.bookmarks[0].name, "Bookmark 1");
        no_idle_replay(&mut h);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.active().unwrap().doc.bookmarks[1].page, 2);
    }

    #[test]
    fn ui223_interactive_article_create_add_independent_export_and_delete_undo() {
        let mut app = app();
        let a = frame_item(&mut app, 72);
        let b = frame_item(&mut app, 180);
        app.run("selection.set", json!({"ids": [a.0]})).unwrap();
        let mut h = harness(app, articles, true);
        let steps = undo(h.state());
        let new = sdk_button(&h, "New Article from Selection");
        click_rect(&mut h, new.rect);
        last(h.state(), "article.new", json!({}));
        assert_eq!(h.state().session.active().unwrap().doc.articles[0].items, vec![a]);
        assert_eq!(undo(h.state()), steps + 1);
        h.state_mut().run("selection.set", json!({"ids": [b.0]})).unwrap();
        idle(&mut h, 2);
        click(&mut h, "Add");
        last(h.state(), "article.add", json!({"name": "Article 1"}));
        assert_eq!(h.state().session.active().unwrap().doc.articles[0].items, vec![a, b]);
        no_idle_replay(&mut h);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.active().unwrap().doc.articles[0].items, vec![a]);
        click(&mut h, "New Article from Selection");
        let export = export_checkbox(&h, "Article 2");
        let steps = undo(h.state());
        click_rect(&mut h, export.rect);
        last(h.state(), "article.options", json!({"name": "Article 2", "export": false}));
        let doc = &h.state().session.active().unwrap().doc;
        assert!(doc.articles[0].export);
        assert!(!doc.articles[1].export);
        assert_eq!(doc.articles[1].items, vec![b]);
        assert_eq!(undo(h.state()), steps + 1);
        no_idle_replay(&mut h);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert!(h.state().session.active().unwrap().doc.articles[1].export);
        idle(&mut h, 2);
        let rect = h.get_all_by_label("Delete").nth(1).unwrap().rect();
        click_rect(&mut h, rect);
        last(h.state(), "article.delete", json!({"name": "Article 2"}));
        assert_eq!(h.state().session.active().unwrap().doc.articles.len(), 1);
        assert_eq!(h.state().session.active().unwrap().doc.articles[0].name, "Article 1");
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.active().unwrap().doc.articles[1].name, "Article 2");
    }

    #[test]
    fn ui223_interactive_article_natural_tab_space_toggles_once_and_selection_guards() {
        let mut h = harness(app(), articles, true);
        let new = sdk_button(&h, "New Article from Selection");
        assert!(new.facts.as_ref().unwrap().disabled);
        let steps = undo(h.state());
        let journal = h.state().session.journal.clone();
        click_rect(&mut h, new.rect);
        assert_eq!(undo(h.state()), steps);
        assert_eq!(h.state().session.journal, journal);
        frame_item(h.state_mut(), 72);
        idle(&mut h, 2);
        click(&mut h, "New Article from Selection");
        let export = export_checkbox(&h, "Article 1");
        for _ in 0..8 {
            key(&mut h, Key::Tab);
            if h.ctx.memory(|m| m.focused()) == Some(export.id) {
                break;
            }
        }
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(export.id), "Tab naturally reaches the installed Checkbox");
        let steps = undo(h.state());
        let commands = count(h.state(), "article.options");
        key(&mut h, Key::Space);
        assert!(!h.state().session.active().unwrap().doc.articles[0].export);
        assert_eq!(undo(h.state()), steps + 1);
        assert_eq!(count(h.state(), "article.options"), commands + 1);
        no_idle_replay(&mut h);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert!(h.state().session.active().unwrap().doc.articles[0].export);
        h.state_mut().run("selection.set", json!({"ids": []})).unwrap();
        idle(&mut h, 2);
        let steps = undo(h.state());
        let journal = h.state().session.journal.clone();
        click(&mut h, "Add");
        assert_eq!(undo(h.state()), steps);
        assert_eq!(h.state().session.journal, journal);
    }

    #[test]
    fn ui223_interactive_new_document_does_not_reuse_previous_article_toggle_state() {
        let mut app = app();
        frame_item(&mut app, 72);
        let mut h = harness(app, articles, true);
        click(&mut h, "New Article from Selection");
        let export = export_checkbox(&h, "Article 1");
        click_rect(&mut h, export.rect);
        let first_uid = h.state().session.active().unwrap().uid;
        h.state_mut().run("file.new", json!({"title": "Other document"})).unwrap();
        frame_item(h.state_mut(), 72);
        idle(&mut h, 2);
        click(&mut h, "New Article from Selection");
        let second_uid = h.state().session.active().unwrap().uid;
        assert_ne!(first_uid, second_uid);
        assert!(h.state().session.active().unwrap().doc.articles[0].export);
        assert!(!h.state().session.documents().iter().find(|d| d.uid == first_uid).unwrap().doc.articles[0].export);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_interactive_uninstalled_native_fallback_remains_operable() {
        let mut app = app();
        frame_item(&mut app, 72);
        let mut h = harness(app, hyperlinks, false);
        type_url(&mut h, "mailto:layout@example.com");
        click(&mut h, "New");
        last(h.state(), "hyperlink.create", json!({"email": "layout@example.com"}));
        assert_eq!(h.state().session.active().unwrap().doc.hyperlinks.len(), 1);
        assert!(!h.state().framework_installed);
        let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, region()).unwrap();
        assert!(record.elements.iter().all(|e| e.component != "Button" && e.component != "Checkbox"));
        no_idle_replay(&mut h);
    }
    #[test]
    fn ui223_interactive_localized_bookmark_action_keeps_command_and_page_base() {
        for language in ["fr", "ar"] {
            let mut app = app();
            app.ui.language = language.into();
            let label = crate::i18n::tr(language, "New Bookmark for This Page").to_owned();
            let mut h = harness(app, bookmarks, true);
            let page = crate::canvas::current_page(h.state()).unwrap_or(0);
            let new = sdk_button(&h, &label);
            click_rect(&mut h, new.rect);
            last(h.state(), "bookmark.add", json!({"page": page + 1}));
            assert_eq!(h.state().session.active().unwrap().doc.bookmarks[0].page, page);
            no_idle_replay(&mut h);
        }
    }

    fn compact_row_button(h: &Harness<'_, DesignApp>, row: &str, caption: &str, standard: &str) -> craft_ui_widgets::runtime::inspect::Element {
        let y = h.get_by_label(row).rect().center().y;
        let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, region()).unwrap();
        let button = record
            .elements
            .iter()
            .find(|element| {
                element.component == "Button"
                    && element.part.is_none()
                    && element.rect.y_range().contains(y)
                    && element.facts.as_ref().and_then(|facts| facts.name.as_deref()) == Some(caption)
            })
            .unwrap_or_else(|| panic!("the actual SDK {caption} for row {row} must be present"))
            .clone();
        let facts = button.facts.as_ref().unwrap();
        assert_eq!(facts.role, "Button");
        let standard = sdk_button(h, standard);
        assert!(button.rect.height() < standard.rect.height(), "the production compact action uses Sm, not the default Md role");
        let parent: egui::Rect = h.ctx.data(|data| data.get_temp(region().with("actual_parent"))).unwrap();
        assert!(parent.contains_rect(button.rect), "compact action must fit its actual panel parent: {:?} in {parent:?}", button.rect);
        assert!(button.rect.contains_rect(button.content));
        if !facts.disabled {
            assert!(record.hits.iter().any(|hit| hit.owner == button.id && hit.part.is_none()), "the enabled SDK component must own a real hit area");
        }
        button
    }

    fn compact_pointer(h: &mut Harness<'_, DesignApp>, rect: egui::Rect, pressed: bool) {
        let pos = rect.center();
        input(h, Some(Event::PointerMoved(pos)));
        input(h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
        idle(h, 2);
    }

    #[test]
    fn future_compact_hyperlink_delete_rejects_other_document_and_recreated_epoch() {
        for preserve_uid in [false, true] {
            let mut app = app();
            frame_item(&mut app, 72);
            let made = app.run("hyperlink.create", json!({"url": "https://example.com/target", "name": "Target"})).unwrap();
            let target = made["ids"][0].as_u64().unwrap();
            let mut h = harness(app, hyperlinks, true);
            let original = compact_row_button(&h, "Target", "Delete", "New");
            let before = h.state().session.active().unwrap();
            let original_document = (before.uid, before.identity_epoch);
            let document = (*before.doc).clone();
            compact_pointer(&mut h, original.rect, true);
            // In-memory host fixture: a new DocState preserves all item/hyperlink ids. With
            // preserve_uid it models Revert's documented uid retention and fresh epoch, without disk IO.
            let mut replacement = designcraft_engine::DocState::new(document, None);
            if preserve_uid {
                replacement.uid = original_document.0;
                *h.state_mut().session.active_mut().unwrap() = replacement;
            } else {
                h.state_mut().session.add_document(replacement);
            }
            h.state_mut().after_engine();
            idle(&mut h, 2);
            let current = h.state().session.active().unwrap();
            assert_eq!(current.uid == original_document.0, preserve_uid);
            assert_ne!(current.identity_epoch, original_document.1);
            assert_eq!(current.doc.hyperlinks[0].id, target, "the numeric target is deliberately reused");
            let replacement = compact_row_button(&h, "Target", "Delete", "New");
            assert_ne!(original.id, replacement.id, "UID and epoch bind the held SDK action");
            assert_eq!(original.rect, replacement.rect, "the stale release still lands over the new control, not empty space");
            let journal = h.state().session.journal.clone();
            let steps = undo(h.state());
            compact_pointer(&mut h, replacement.rect, false);
            assert_eq!(h.state().session.journal, journal, "a held action cannot migrate into another document state");
            assert_eq!(undo(h.state()), steps);
            assert_eq!(h.state().session.active().unwrap().doc.hyperlinks.len(), 1);
            // Positive control: a fresh real click invokes this target's real callback exactly once.
            click_rect(&mut h, replacement.rect);
            last(h.state(), "hyperlink.delete", json!({"id": target}));
            assert!(h.state().session.active().unwrap().doc.hyperlinks.is_empty());
            assert_eq!(undo(h.state()), steps + 1);
            no_idle_replay(&mut h);
            h.state_mut().run("edit.undo", json!({})).unwrap();
            assert_eq!(h.state().session.active().unwrap().doc.hyperlinks[0].id, target);
        }
    }

    #[test]
    fn future_compact_bookmark_delete_rejects_reindexed_target_and_keeps_current_index_policy() {
        let mut app = app();
        for (name, page) in [("First", 1), ("Held", 2), ("Replacement", 3)] {
            app.run("bookmark.add", json!({"name": name, "page": page})).unwrap();
        }
        let mut h = harness(app, bookmarks, true);
        let held = compact_row_button(&h, "Held", "Delete", "New Bookmark for This Page");
        let before = h.state().session.active().unwrap();
        let identity = (before.uid, before.identity_epoch);
        compact_pointer(&mut h, held.rect, true);
        // Another host action removes the earlier row while the SDK press is held.
        h.state_mut().run("bookmark.delete", json!({"index": 0})).unwrap();
        idle(&mut h, 2);
        let current = h.state().session.active().unwrap();
        assert_eq!((current.uid, current.identity_epoch), identity, "only row identity changed");
        let replacement = compact_row_button(&h, "Replacement", "Delete", "New Bookmark for This Page");
        assert_ne!(held.id, replacement.id, "the same displayed index now names a different bookmark");
        assert_eq!(held.rect, replacement.rect);
        let journal = h.state().session.journal.clone();
        let steps = undo(h.state());
        compact_pointer(&mut h, replacement.rect, false);
        assert_eq!(h.state().session.journal, journal);
        assert_eq!(h.state().session.active().unwrap().doc.bookmarks.len(), 2);
        click_rect(&mut h, replacement.rect);
        last(h.state(), "bookmark.delete", json!({"index": 1}));
        assert_eq!(h.state().session.active().unwrap().doc.bookmarks[0].name, "Held");
        assert_eq!(h.state().session.active().unwrap().doc.bookmarks.len(), 1);
        assert_eq!(undo(h.state()), steps + 1);
        no_idle_replay(&mut h);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.active().unwrap().doc.bookmarks[1].name, "Replacement");
    }

    #[test]
    fn future_compact_article_add_keyboard_disabled_guard_and_name_delete_use_sdk() {
        let mut app = app();
        let a = frame_item(&mut app, 72);
        let b = frame_item(&mut app, 180);
        let c = frame_item(&mut app, 288);
        app.run("article.new", json!({"name": "Main", "ids": [a.0]})).unwrap();
        app.run("article.new", json!({"name": "Sidebar", "ids": [b.0]})).unwrap();
        app.run("selection.set", json!({"ids": [c.0]})).unwrap();
        let mut h = harness(app, articles, true);
        // The real Article export checkboxes retain compact paint but must have useful,
        // distinguishable names in the actual AccessKit tree, not just descriptions.
        let _ = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Include when exporting: Main");
        let _ = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Include when exporting: Sidebar");
        let add = compact_row_button(&h, "Main", "Add", "New Article from Selection");
        assert!(!add.facts.as_ref().unwrap().disabled);
        for _ in 0..8 {
            key(&mut h, Key::Tab);
            if h.ctx.memory(|memory| memory.focused()) == Some(add.id) {
                break;
            }
        }
        assert_eq!(h.ctx.memory(|memory| memory.focused()), Some(add.id), "Tab naturally reaches the actual compact Add");
        let steps = undo(h.state());
        let commands = count(h.state(), "article.add");
        key(&mut h, Key::Enter);
        last(h.state(), "article.add", json!({"name": "Main"}));
        assert_eq!(h.state().session.active().unwrap().doc.articles[0].items, vec![a, c]);
        assert_eq!(h.state().session.active().unwrap().doc.articles[1].items, vec![b]);
        assert_eq!(undo(h.state()), steps + 1);
        assert_eq!(count(h.state(), "article.add"), commands + 1);
        no_idle_replay(&mut h);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.active().unwrap().doc.articles[0].items, vec![a]);
        h.state_mut().run("selection.set", json!({"ids": []})).unwrap();
        idle(&mut h, 2);
        let disabled = compact_row_button(&h, "Main", "Add", "New Article from Selection");
        assert!(disabled.facts.as_ref().unwrap().disabled);
        let steps = undo(h.state());
        let journal = h.state().session.journal.clone();
        click_rect(&mut h, disabled.rect);
        assert_eq!(undo(h.state()), steps);
        assert_eq!(h.state().session.journal, journal);
        let delete = compact_row_button(&h, "Sidebar", "Delete", "New Article from Selection");
        assert!(!delete.facts.as_ref().unwrap().disabled, "deletion is independent of frame selection");
        click_rect(&mut h, delete.rect);
        last(h.state(), "article.delete", json!({"name": "Sidebar"}));
        assert_eq!(h.state().session.active().unwrap().doc.articles.len(), 1);
        assert_eq!(h.state().session.active().unwrap().doc.articles[0].name, "Main");
        assert_eq!(undo(h.state()), steps + 1);
        no_idle_replay(&mut h);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.active().unwrap().doc.articles[1].name, "Sidebar");
    }

    #[test]
    fn future_compact_uninstalled_runtime_keeps_small_native_callback() {
        let mut app = app();
        frame_item(&mut app, 72);
        let target = app.run("hyperlink.create", json!({"url": "https://example.com", "name": "Native target"})).unwrap()["ids"][0].as_u64().unwrap();
        let mut h = harness(app, hyperlinks, false);
        let delete = h.get_by_label("Delete").rect();
        assert!(delete.height() < h.get_by_label("New").rect().height(), "the fallback keeps native small-button sizing");
        let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, region()).unwrap();
        assert!(record.elements.iter().all(|element| element.component != "Button"), "a same-label native button is not SDK evidence");
        let steps = undo(h.state());
        click_rect(&mut h, delete);
        last(h.state(), "hyperlink.delete", json!({"id": target}));
        assert!(h.state().session.active().unwrap().doc.hyperlinks.is_empty());
        assert_eq!(undo(h.state()), steps + 1);
        no_idle_replay(&mut h);
    }

    #[test]
    fn public_control_size_reaches_existing_component_builders() {
        use craft_ui_widgets::buttons::Button;
        use craft_ui_widgets::choice::Checkbox;
        use craft_ui_widgets::contract::{Bind, Props as _};
        use craft_theme::ControlSize;
        use craft_ui_widgets::style::{Variant};
        use craft_ui_widgets::text::TextField;

        // Both named roles must be the exact enum already accepted by Props::size, not a copy.
        let _small = Button::new("Small toolbar action").painted("+").variant(Variant::GHOST).size(ControlSize::Sm);
        let _standard = Button::new("Standard action").size(ControlSize::Md);
        let _check = Checkbox::new(Bind::given(false)).size(ControlSize::Sm);
        let mut name = String::new();
        let _field = TextField::new("Name", Bind::bound(&mut name)).size(ControlSize::Md);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui223_object_states_forms_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use designcraft_doc::{ButtonAction, FieldKind, FormField, ItemId};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{Harness, kittest::Queryable};
    use std::collections::HashSet;
    use std::sync::Arc;

    struct Window {
        app: DesignApp,
        panel: fn(&mut DesignApp, &mut egui::Ui),
        controls: Option<Arc<Recorded>>,
        parent: egui::Rect,
        enabled: bool,
        measuring: bool,
    }
    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"title": "Interactive workflow", "pages": 3})).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::pos2(500.0, 0.0), egui::vec2(500.0, 700.0)));
        app
    }
    fn item(app: &mut DesignApp, x: i32) -> ItemId {
        ItemId(app.run("frame.create", json!({"rect": [x, 72, x + 60, 132]})).unwrap()["id"].as_u64().unwrap())
    }
    fn select(app: &mut DesignApp, ids: &[ItemId]) {
        app.run("selection.set", json!({"ids": ids.iter().map(|id| id.0).collect::<Vec<_>>()})).unwrap();
    }
    fn fixture(mut app: DesignApp, panel: fn(&mut DesignApp, &mut egui::Ui), sdk: bool) -> Harness<'static, Window> {
        app.framework_transform = false;
        app.framework_opacity = sdk;
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        assert_eq!(app.framework_installed, sdk);
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(320.0, 720.0)).build_ui_state(
            move |ui, state: &mut Window| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), "");
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                state.parent = ui.max_rect();
                ui.add_enabled_ui(state.enabled, |ui| {
                    let ctx = ui.ctx().clone();
                    let mut render = || {
                        state.controls = record(ui, egui::Id::NULL.with("object-states-forms"), |ui| (state.panel)(&mut state.app, ui)).1;
                    };
                    if state.measuring {
                        craft_ui_widgets::runtime::measure::run(&ctx, render);
                    } else {
                        render();
                    }
                });
            },
            Window { app, panel, controls: None, parent: egui::Rect::NOTHING, enabled: true, measuring: false },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }
    fn step(h: &mut Harness<'_, Window>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn event(h: &mut Harness<'_, Window>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn idle(h: &mut Harness<'_, Window>, n: usize) {
        for _ in 0..n {
            step(h);
        }
    }
    fn key_mod(h: &mut Harness<'_, Window>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        event(h, Event::ModifiersChanged(Modifiers::NONE));
        idle(h, 2);
    }
    fn key(h: &mut Harness<'_, Window>, key: Key) {
        key_mod(h, key, Modifiers::NONE);
    }
    fn pointer(h: &mut Harness<'_, Window>, rect: egui::Rect, pressed: bool) {
        let pos = rect.center();
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        idle(h, 2);
    }
    fn click_rect(h: &mut Harness<'_, Window>, rect: egui::Rect) {
        pointer(h, rect, true);
        pointer(h, rect, false);
    }
    fn node(h: &Harness<'_, Window>, component: &str, name: &str) -> Element {
        let matches: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.part.is_none() && e.component == component && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(name))
            .cloned()
            .collect();
        assert_eq!(matches.len(), 1, "exactly one actual {component} named {name:?}");
        matches[0].clone()
    }
    fn click(h: &mut Harness<'_, Window>, name: &str) {
        // Full-name lookup is intentionally unique; never select a same-label sibling by index.
        let rect = h.get_by_label(name).rect();
        click_rect(h, rect);
    }
    fn choose(h: &mut Harness<'_, Window>, name: &str, value: &str) {
        click(h, name);
        click(h, value);
    }
    fn type_text(h: &mut Harness<'_, Window>, name: &str, value: &str) {
        let rect = h.get_by_role_and_label(egui::accesskit::Role::TextInput, name).rect();
        click_rect(h, rect);
        key_mod(h, Key::A, Modifiers::COMMAND);
        event(h, Event::Text(value.to_owned()));
        idle(h, 2);
    }
    fn commit(h: &mut Harness<'_, Window>, name: &str, value: &str) {
        type_text(h, name, value);
        key(h, Key::Tab);
    }
    fn tab_to(h: &mut Harness<'_, Window>, id: egui::Id) {
        let mut seen = HashSet::new();
        loop {
            let focused = h.ctx.memory(|memory| memory.focused());
            if focused == Some(id) {
                return;
            }
            assert!(seen.insert(focused), "natural Tab must reach {id:?}");
            key(h, Key::Tab);
        }
    }
    fn undo(h: &Harness<'_, Window>) -> usize {
        h.state().app.session.active().unwrap().history.undo.len()
    }
    fn journal(h: &Harness<'_, Window>) -> Vec<(String, Value)> {
        h.state().app.session.journal.clone()
    }
    fn last(h: &Harness<'_, Window>, command: &str, params: Value) {
        assert_eq!(h.state().app.session.journal.last(), Some(&(command.to_owned(), params)));
    }
    fn field(h: &Harness<'_, Window>, id: ItemId) -> FormField {
        h.state().app.session.active().unwrap().doc.item(id).unwrap().form_field.clone().unwrap()
    }
    fn action(h: &Harness<'_, Window>, id: ItemId) -> Option<ButtonAction> {
        h.state().app.session.active().unwrap().doc.item(id).unwrap().button.clone()
    }
    fn no_replay(h: &mut Harness<'_, Window>) {
        let before = (journal(h), undo(h));
        idle(h, 4);
        assert_eq!((journal(h), undo(h)), before);
    }
    fn prepared_form(kind: &str) -> (DesignApp, ItemId) {
        let mut app = app();
        let id = item(&mut app, 72);
        select(&mut app, &[id]);
        app.run("form.set", json!({"kind": kind, "name": "Original", "value": "Initial", "options": ["A", "B"]})).unwrap();
        (app, id)
    }

    #[test]
    fn ui223_states_selected_objects_create_real_parent_show_release_and_undo() {
        let mut app = app();
        let a = item(&mut app, 72);
        let b = item(&mut app, 172);
        select(&mut app, &[a, b]);
        let mut h = fixture(app, states, true);
        let before = undo(&h);
        let before_journal = journal(&h).len();
        let before_doc = h.state().app.session.active().unwrap().doc.clone();
        let before_selection = h.state().app.session.active().unwrap().selection.clone();
        let create = node(&h, "Button", "Convert Selection to Multi-State Object");
        assert!(h.state().parent.expand(0.5).contains_rect(create.rect));
        click(&mut h, "Convert Selection to Multi-State Object");
        last(&h, "states.create", json!({}));
        // execute_unguarded makes the nested object.group part of states.create's
        // outer undoable command; record_undo_step explicitly excludes nested steps.
        assert_eq!(undo(&h), before + 1, "one public command is one atomic Undo step");
        assert_eq!(
            &journal(&h)[before_journal..],
            &[("object.group".to_owned(), json!({"ids": [a.0, b.0]})), ("states.create".to_owned(), json!({})),],
            "atomic history must not suppress either actual domain command"
        );
        let gid = h.state().app.session.active().unwrap().selection.items[0];
        let group = h.state().app.session.active().unwrap().doc.item(gid).unwrap();
        assert_eq!(group.children().iter().map(|c| c.id).collect::<Vec<_>>(), vec![a, b]);
        assert_eq!(group.states, vec!["State 1", "State 2"]);
        assert_eq!(group.active_state, 0);
        let created_doc = h.state().app.session.active().unwrap().doc.clone();
        let created_selection = h.state().app.session.active().unwrap().selection.clone();
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 3);
        assert_eq!(undo(&h), before);
        assert_eq!(
            serde_json::to_value(h.state().app.session.active().unwrap().doc.as_ref()).unwrap(),
            serde_json::to_value(before_doc.as_ref()).unwrap(),
            "one Undo restores both original top-level objects, not an intermediate plain group"
        );
        assert_eq!(h.state().app.session.active().unwrap().selection, before_selection);
        assert!(h.state().app.session.active().unwrap().doc.item(gid).is_none());
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        idle(&mut h, 3);
        assert_eq!(undo(&h), before + 1);
        assert_eq!(
            serde_json::to_value(h.state().app.session.active().unwrap().doc.as_ref()).unwrap(),
            serde_json::to_value(created_doc.as_ref()).unwrap(),
            "one Redo restores the actual group and all state metadata together"
        );
        assert_eq!(h.state().app.session.active().unwrap().selection, created_selection);
        let parent = node(&h, "RadioGroup", "Object States");
        let second = node(&h, "Radio", "Object States: State 2 (2)");
        assert!(second.depth > parent.depth);
        assert_eq!(second.facts.as_ref().unwrap().role, "RadioButton");
        let _ = h.get_by_role_and_label(egui::accesskit::Role::RadioGroup, "Object States");
        let _ = h.get_by_role_and_label(egui::accesskit::Role::RadioButton, "Object States: State 2 (2)");
        click(&mut h, "Object States: State 2 (2)");
        last(&h, "states.show", json!({"index": 1}));
        assert_eq!(h.state().app.session.active().unwrap().doc.item(gid).unwrap().active_state, 1);
        let before_release = undo(&h);
        click(&mut h, "Release to Objects");
        last(&h, "states.release", json!({}));
        assert_eq!(undo(&h), before_release + 1);
        assert!(h.state().app.session.active().unwrap().doc.item(gid).unwrap().states.is_empty());
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 3);
        let restored = h.state().app.session.active().unwrap().doc.item(gid).unwrap();
        assert_eq!(restored.states.len(), 2);
        assert_eq!(restored.active_state, 1);
        assert_eq!(restored.children().len(), 2);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_states_existing_group_natural_tab_arrow_space_and_single_edit() {
        fn radio_focused(h: &Harness<'_, Window>, target: &Element) -> bool {
            use egui_kittest::kittest::NodeT as _;

            assert_eq!(target.component, "Radio");
            assert!(target.part.is_none());
            let facts = target.facts.as_ref().expect("actual radio has access facts");
            let name = facts.name.as_deref().expect("actual radio has its full name");
            assert!(!facts.disabled, "natural keyboard target must be enabled");
            assert_eq!(node(h, "Radio", name).id, target.id, "the actual target radio remains current");
            let radio = h.get_by_role_and_label(egui::accesskit::Role::RadioButton, name);
            let access = radio.accesskit_node();
            assert!(!access.is_disabled(), "the actual RadioButton must be enabled");
            access.is_focused()
        }
        fn tab_to_radio(h: &mut Harness<'_, Window>, target: &Element) {
            use egui_kittest::kittest::NodeT as _;

            // Native focus belongs to the arena root; AX reports its actual focused member.
            // Read that output, never request focus or replace the natural Tab input.
            let mut seen = HashSet::new();
            loop {
                if radio_focused(h, target) {
                    return;
                }
                let root = h.queryable_node();
                let focused: Vec<_> = std::iter::once(root.clone())
                    .chain(root.children_recursive())
                    .filter_map(|node| {
                        let access = node.accesskit_node();
                        access.is_focused().then(|| format!("{:?}", access.id()))
                    })
                    .collect();
                assert!(
                    seen.insert(focused.clone()),
                    "natural Tab must reach the actual target RadioButton {:?}; repeated AX focus {focused:?}",
                    target.id
                );
                key(h, Key::Tab);
            }
        }
        let mut app = app();
        let a = item(&mut app, 72);
        let b = item(&mut app, 172);
        select(&mut app, &[a, b]);
        let gid = ItemId(app.run("object.group", json!({})).unwrap()["id"].as_u64().unwrap());
        let mut h = fixture(app, states, true);
        let before = undo(&h);
        click(&mut h, "Convert Selection to Multi-State Object");
        assert_eq!(undo(&h), before + 1);
        let first = node(&h, "Radio", "Object States: State 1 (1)");
        tab_to_radio(&mut h, &first);
        let before = (journal(&h), undo(&h));
        key(&mut h, Key::ArrowDown);
        let second = node(&h, "Radio", "Object States: State 2 (2)");
        assert!(radio_focused(&h, &second), "natural ArrowDown must focus the actual second RadioButton");
        assert_eq!((journal(&h), undo(&h)), before, "roving focus is not an engine edit");
        key(&mut h, Key::Space);
        last(&h, "states.show", json!({"index": 1}));
        assert_eq!(h.state().app.session.active().unwrap().doc.item(gid).unwrap().active_state, 1);
        assert_eq!(undo(&h), before.1 + 1);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_states_empty_selection_disabled_and_single_object_refusal_are_retained() {
        let mut app = app();
        select(&mut app, &[]);
        let mut h = fixture(app, states, true);
        let create = node(&h, "Button", "Convert Selection to Multi-State Object");
        assert!(create.facts.unwrap().disabled);
        let before = (journal(&h), undo(&h));
        click_rect(&mut h, create.rect);
        assert_eq!((journal(&h), undo(&h)), before);
        let id = item(&mut h.state_mut().app, 72);
        select(&mut h.state_mut().app, &[id]);
        idle(&mut h, 3);
        let create = node(&h, "Button", "Convert Selection to Multi-State Object");
        assert!(!create.facts.unwrap().disabled, "n > 0 eligibility is unchanged");
        let before = (journal(&h), undo(&h));
        click(&mut h, "Convert Selection to Multi-State Object");
        assert_eq!((journal(&h), undo(&h)), before);
        assert!(h.state().app.ui.status.contains("select two or more"));
        assert!(h.state().app.session.active().unwrap().doc.item(id).unwrap().states.is_empty());
        no_replay(&mut h);
    }

    #[test]
    fn ui223_buttons_all_actions_default_url_page_domain_and_full_selection_payloads() {
        fn observe(h: &Harness<'_, Window>, phase: &str, events: &[Event]) {
            use egui_kittest::kittest::{NodeT as _, Queryable as _};
            let native_focus = h.ctx.memory(|memory| memory.focused());
            let observed: Vec<_> = {
                let root = h.queryable_node();
                std::iter::once(root.clone())
                    .chain(root.children_recursive())
                    .filter_map(|node| {
                        let access = node.accesskit_node();
                        let label = access.label();
                        (access.is_focused() || label.as_deref() == Some("Button target page")).then(|| {
                            (access.id(), access.role(), label, access.value(), access.numeric_value(), access.is_focused(), access.is_disabled())
                        })
                    })
                    .collect()
            };
            let app = &h.state().app;
            let context = app.session.active().map(|doc| {
                (
                    doc.uid,
                    doc.identity_epoch,
                    doc.history.undo.len(),
                    doc.selection.items.clone(),
                    doc.selection.items.iter().map(|id| doc.doc.item(*id).map(|item| item.button.clone())).collect::<Vec<_>>(),
                )
            });
            eprintln!(
                "STATES-PAGE-READONLY phase={phase} events={events:?} native_focus={native_focus:?} ax={observed:?} context={context:?} journal_len={} last={:?}",
                app.session.journal.len(),
                app.session.journal.last()
            );
        }
        fn step(h: &mut Harness<'_, Window>) {
            let mut raw = std::mem::take(h.input_mut());
            observe(h, "before-hook", &raw.events);
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            observe(h, "after-hook", &raw.events);
            *h.input_mut() = raw;
            h.step();
            observe(h, "after-step", &[]);
        }
        fn event(h: &mut Harness<'_, Window>, event: Event) {
            h.input_mut().events.push(event);
            step(h);
        }
        fn idle(h: &mut Harness<'_, Window>, n: usize) {
            for _ in 0..n {
                step(h);
            }
        }
        fn key_mod(h: &mut Harness<'_, Window>, key: Key, modifiers: Modifiers) {
            for pressed in [true, false] {
                event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
            }
            event(h, Event::ModifiersChanged(Modifiers::NONE));
            idle(h, 2);
        }
        fn key(h: &mut Harness<'_, Window>, key: Key) {
            key_mod(h, key, Modifiers::NONE);
        }
        fn click_rect(h: &mut Harness<'_, Window>, rect: egui::Rect) {
            let pos = rect.center();
            event(h, Event::PointerMoved(pos));
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            let pressed_at = h.ctx.input(|i| i.time);
            event(h, Event::PointerMoved(pos));
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            let held_for = h.ctx.input(|i| i.time) - pressed_at;
            let native_limit = h.ctx.options(|o| o.input_options.max_click_duration);
            assert!(held_for > 0.0 && held_for <= native_limit, "the real page gesture must remain a click: {held_for}s > {native_limit}s");
            assert!(h.ctx.input(|i| i.pointer.any_click()), "the native page editor must receive a real click before keyboard input");
            idle(h, 2);
        }
        let mut app = app();
        let a = item(&mut app, 72);
        let b = item(&mut app, 172);
        select(&mut app, &[a, b]);
        let mut h = fixture(app, buttons, true);
        for (caption, action_name) in [
            ("Go To Next Page", "nextPage"),
            ("Go To Previous Page", "previousPage"),
            ("Go To First Page", "firstPage"),
            ("Go To Last Page", "lastPage"),
            ("Go To Page", "page"),
        ] {
            let before = undo(&h);
            choose(&mut h, "On Release Action", caption);
            last(&h, "button.set", json!({"action": action_name, "page": 0, "url": "https://"}));
            assert_eq!(undo(&h), before + 1);
            assert_eq!(action(&h, a), action(&h, b));
            no_replay(&mut h);
        }
        let page = h.get_by_role_and_label(egui::accesskit::Role::SpinButton, "Button target page").rect();
        click_rect(&mut h, page);
        key_mod(&mut h, Key::A, Modifiers::COMMAND);
        event(&mut h, Event::Text("3".to_owned()));
        key(&mut h, Key::Enter);
        last(&h, "button.set", json!({"action": "page", "page": 2}));
        assert_eq!(action(&h, a), Some(ButtonAction::GoToPage { page: 2 }));
        choose(&mut h, "On Release Action", "Go To URL");
        last(&h, "button.set", json!({"action": "url", "page": 2, "url": "https://"}));
        assert_eq!(action(&h, b), Some(ButtonAction::GoToUrl { url: "https://".to_owned() }));
        choose(&mut h, "On Release Action", "Not a button");
        last(&h, "button.set", json!({"action": "none", "page": 0, "url": "https://"}));
        assert_eq!(action(&h, a), None);
        assert_eq!(action(&h, b), None);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_buttons_real_select_natural_keyboard_escape_and_commit_do_not_preview_engine_edits() {
        let mut app = app();
        let id = item(&mut app, 72);
        select(&mut app, &[id]);
        let mut h = fixture(app, buttons, true);
        let action = node(&h, "Select", "On Release Action");
        assert_eq!(action.facts.as_ref().unwrap().role, "ComboBox");
        tab_to(&mut h, action.id);
        let before = (journal(&h), undo(&h));
        key(&mut h, Key::Space);
        key(&mut h, Key::ArrowDown);
        assert_eq!((journal(&h), undo(&h)), before);
        key(&mut h, Key::Escape);
        assert_eq!((journal(&h), undo(&h)), before);
        key(&mut h, Key::Space);
        key(&mut h, Key::Home);
        key(&mut h, Key::ArrowDown);
        key(&mut h, Key::Enter);
        last(&h, "button.set", json!({"action": "nextPage", "page": 0, "url": "https://"}));
        assert_eq!(undo(&h), before.1 + 1);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_buttons_url_lost_focus_refusal_correction_escape_and_undo_preserve_draft_semantics() {
        let mut app = app();
        let id = item(&mut app, 72);
        select(&mut app, &[id]);
        app.run("button.set", json!({"action": "url", "url": "https://example.com/original"})).unwrap();
        let mut h = fixture(app, buttons, true);
        let before = (journal(&h), undo(&h));
        type_text(&mut h, "Button URL", "javascript:alert(1)");
        assert_eq!((journal(&h), undo(&h)), before);
        key(&mut h, Key::Tab);
        assert_eq!((journal(&h), undo(&h)), before);
        assert_eq!(action(&h, id), Some(ButtonAction::GoToUrl { url: "https://example.com/original".into() }));
        assert!(h.state().app.ui.status.contains("http://"));
        no_replay(&mut h);
        type_text(&mut h, "Button URL", "https://example.com/corrected?q=a%20b#part");
        key(&mut h, Key::Escape);
        last(&h, "button.set", json!({"action": "url", "url": "https://example.com/corrected?q=a%20b#part"}));
        assert_eq!(undo(&h), before.1 + 1, "Escape ends the old TextEdit draft; it is not a document rollback");
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 3);
        assert_eq!(action(&h, id), Some(ButtonAction::GoToUrl { url: "https://example.com/original".into() }));
        assert_eq!(node(&h, "TextField", "Button URL").facts.unwrap().value.as_deref(), Some("https://example.com/original"));
        no_replay(&mut h);
    }

    #[test]
    fn ui223_forms_every_kind_set_clear_and_unique_full_names_keep_all_selected_targets() {
        let mut app = app();
        let a = item(&mut app, 72);
        let b = item(&mut app, 172);
        select(&mut app, &[a, b]);
        let mut h = fixture(app, buttons, true);
        for (caption, kind) in [
            ("Text Field", FieldKind::TextField),
            ("Check Box", FieldKind::CheckBox),
            ("Combo Box", FieldKind::ComboBox),
            ("List Box", FieldKind::ListBox),
            ("Signature Field", FieldKind::Signature),
        ] {
            let before = undo(&h);
            choose(&mut h, "Form Field Type", caption);
            last(&h, "form.set", json!({"kind": kind}));
            assert_eq!(field(&h, a).kind, kind);
            assert_eq!(field(&h, b).kind, kind);
            assert_eq!(undo(&h), before + 1);
            assert_ne!(field(&h, a).name, field(&h, b).name, "engine still owns unique default names");
            let name = node(&h, "TextField", "Form field name");
            assert_eq!(name.facts.unwrap().role, "TextInput");
            let _ = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Required");
            if kind == FieldKind::Signature {
                assert_eq!(h.query_all_by_label("Form field default value").count(), 0);
                assert_eq!(h.query_all_by_label("Form field choices (separated by commas)").count(), 0);
            }
            no_replay(&mut h);
        }
        let before = undo(&h);
        choose(&mut h, "Form Field Type", "Not a form field");
        last(&h, "form.clear", json!({}));
        assert_eq!(undo(&h), before + 1);
        assert!(h.state().app.session.active().unwrap().doc.item(a).unwrap().form_field.is_none());
        assert!(h.state().app.session.active().unwrap().doc.item(b).unwrap().form_field.is_none());
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(field(&h, a).kind, FieldKind::Signature);
    }

    #[test]
    fn ui223_forms_name_default_choices_blur_payload_trim_filter_and_immediate_flags() {
        let (app, id) = prepared_form("comboBox");
        let mut h = fixture(app, buttons, true);
        let before = undo(&h);
        type_text(&mut h, "Form field name", "Order choice");
        assert_eq!(field(&h, id).name, "Original");
        assert_eq!(undo(&h), before);
        key(&mut h, Key::Tab);
        last(&h, "form.set", json!({"kind": "comboBox", "name": "Order choice", "value": "Initial", "options": ["A", "B"]}));
        assert_eq!(undo(&h), before + 1);
        commit(&mut h, "Form field default value", "Blue");
        assert_eq!(field(&h, id).value, "Blue");
        commit(&mut h, "Form field choices (separated by commas)", " Red, , Blue ,Green, ");
        last(&h, "form.set", json!({"kind": "comboBox", "name": "Order choice", "value": "Blue", "options": ["Red", "Blue", "Green"]}));
        assert_eq!(field(&h, id).options, vec!["Red", "Blue", "Green"]);
        click(&mut h, "Required");
        last(&h, "form.set", json!({"kind": "comboBox", "required": true}));
        assert!(field(&h, id).required);
        choose(&mut h, "Form Field Type", "Text Field");
        click(&mut h, "Multiline");
        last(&h, "form.set", json!({"kind": "textField", "multiline": true}));
        assert!(field(&h, id).multiline);
        choose(&mut h, "Form Field Type", "Check Box");
        click(&mut h, "Checked by default");
        last(&h, "form.set", json!({"kind": "checkBox", "value": ""}));
        assert_eq!(field(&h, id).value, "");
        click(&mut h, "Checked by default");
        last(&h, "form.set", json!({"kind": "checkBox", "value": "On"}));
        assert_eq!(field(&h, id).value, "On");
        no_replay(&mut h);
    }

    #[test]
    fn ui223_forms_target_change_absence_external_source_and_fresh_recovery_cancel_stale_drafts() {
        let (mut app, a) = prepared_form("textField");
        let b = item(&mut app, 172);
        app.run("form.set", json!({"ids": [b.0], "kind": "textField", "name": "Second", "value": "Other"})).unwrap();
        select(&mut app, &[a]);
        let mut h = fixture(app, buttons, true);
        let original = node(&h, "TextField", "Form field name");
        type_text(&mut h, "Form field name", "Stale");
        select(&mut h.state_mut().app, &[b]);
        idle(&mut h, 3);
        assert_ne!(node(&h, "TextField", "Form field name").id, original.id);
        let before = (journal(&h), undo(&h));
        key(&mut h, Key::Tab);
        assert_eq!((journal(&h), undo(&h)), before);
        assert_eq!(field(&h, a).name, "Original");
        assert_eq!(field(&h, b).name, "Second");
        select(&mut h.state_mut().app, &[]);
        idle(&mut h, 3);
        assert_eq!(h.query_all_by_label("Form field name").count(), 0);
        select(&mut h.state_mut().app, &[a]);
        idle(&mut h, 3);
        assert_eq!(node(&h, "TextField", "Form field name").facts.unwrap().value.as_deref(), Some("Original"));
        type_text(&mut h, "Form field name", "Also stale");
        h.state_mut().app.run("form.set", json!({"kind": "textField", "name": "External"})).unwrap();
        idle(&mut h, 3);
        assert_eq!(node(&h, "TextField", "Form field name").facts.unwrap().value.as_deref(), Some("External"));
        commit(&mut h, "Form field name", "Recovered");
        assert_eq!(field(&h, a).name, "Recovered");
        assert_eq!(field(&h, b).name, "Second");
        no_replay(&mut h);
    }

    #[test]
    fn ui223_forms_replaced_document_and_same_uid_revert_do_not_inherit_pending_edits() {
        for preserve_uid in [false, true] {
            let (app, id) = prepared_form("textField");
            let mut h = fixture(app, buttons, true);
            let original = node(&h, "TextField", "Form field name");
            type_text(&mut h, "Form field name", "Stale replacement draft");
            let state = h.state().app.session.active().unwrap();
            let identity = (state.uid, state.identity_epoch);
            let document = (*state.doc).clone();
            let mut replacement = designcraft_engine::DocState::new(document, None);
            replacement.selection.items = vec![id];
            if preserve_uid {
                replacement.uid = identity.0;
                *h.state_mut().app.session.active_mut().unwrap() = replacement;
            } else {
                h.state_mut().app.session.add_document(replacement);
            }
            h.state_mut().app.after_engine();
            idle(&mut h, 3);
            let state = h.state().app.session.active().unwrap();
            assert_ne!(state.identity_epoch, identity.1);
            assert_eq!(state.uid == identity.0, preserve_uid);
            assert_ne!(node(&h, "TextField", "Form field name").id, original.id);
            let before = (journal(&h), undo(&h));
            key(&mut h, Key::Tab);
            assert_eq!((journal(&h), undo(&h)), before);
            assert_eq!(field(&h, id).name, "Original");
            commit(&mut h, "Form field name", "Fresh state");
            assert_eq!(field(&h, id).name, "Fresh state");
            no_replay(&mut h);
        }
    }

    #[test]
    fn ui223_states_held_release_and_form_checks_cancel_on_full_target_or_epoch_change() {
        let mut app = app();
        let a = item(&mut app, 72);
        let b = item(&mut app, 172);
        select(&mut app, &[a, b]);
        let gid = ItemId(app.run("states.create", json!({})).unwrap()["id"].as_u64().unwrap());
        let mut h = fixture(app, states, true);
        let held = node(&h, "Button", "Release to Objects");
        pointer(&mut h, held.rect, true);
        let state = h.state().app.session.active().unwrap();
        let mut replacement = designcraft_engine::DocState::new((*state.doc).clone(), None);
        replacement.uid = state.uid;
        replacement.selection.items = vec![gid];
        *h.state_mut().app.session.active_mut().unwrap() = replacement;
        h.state_mut().app.after_engine();
        idle(&mut h, 2);
        let fresh = node(&h, "Button", "Release to Objects");
        assert_ne!(held.id, fresh.id);
        let before = (journal(&h), undo(&h));
        pointer(&mut h, fresh.rect, false);
        assert_eq!((journal(&h), undo(&h)), before);
        assert_eq!(h.state().app.session.active().unwrap().doc.item(gid).unwrap().states.len(), 2);
        click(&mut h, "Release to Objects");
        last(&h, "states.release", json!({}));
        no_replay(&mut h);
        let (mut app, a) = prepared_form("textField");
        let b = item(&mut app, 172);
        select(&mut app, &[a]);
        let mut h = fixture(app, buttons, true);
        let held = node(&h, "Checkbox", "Required");
        pointer(&mut h, held.rect, true);
        select(&mut h.state_mut().app, &[a, b]);
        idle(&mut h, 2);
        let fresh = node(&h, "Checkbox", "Required");
        assert_ne!(held.id, fresh.id, "full target set matters even with unchanged first item");
        let before = (journal(&h), undo(&h));
        pointer(&mut h, fresh.rect, false);
        assert_eq!((journal(&h), undo(&h)), before);
        assert!(!field(&h, a).required);
        click(&mut h, "Required");
        assert!(field(&h, a).required && field(&h, b).required);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_forms_disabled_parent_measurement_and_narrow_real_parent_do_not_mutate_or_escape_bounds() {
        let (app, id) = prepared_form("comboBox");
        let mut h = fixture(app, buttons, true);
        for name in ["On Release Action", "Form Field Type"] {
            let element = node(&h, "Select", name);
            assert!(h.state().parent.expand(0.5).contains_rect(element.rect));
        }
        for name in ["Form field name", "Form field default value", "Form field choices (separated by commas)"] {
            let element = node(&h, "TextField", name);
            assert!(h.state().parent.expand(0.5).contains_rect(element.rect));
        }
        h.state_mut().enabled = false;
        idle(&mut h, 2);
        let required = node(&h, "Checkbox", "Required");
        assert!(required.facts.unwrap().disabled);
        let before = (journal(&h), undo(&h), field(&h, id));
        click_rect(&mut h, required.rect);
        key(&mut h, Key::Space);
        assert_eq!((journal(&h), undo(&h), field(&h, id)), before);
        h.state_mut().enabled = true;
        h.state_mut().measuring = true;
        idle(&mut h, 2);
        let before = (journal(&h), undo(&h), field(&h, id));
        pointer(&mut h, required.rect, true);
        pointer(&mut h, required.rect, false);
        assert_eq!((journal(&h), undo(&h), field(&h, id)), before);
        h.state_mut().measuring = false;
        idle(&mut h, 3);
        click(&mut h, "Required");
        assert!(field(&h, id).required);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_states_and_forms_real_native_fallback_retains_complete_command_workflow() {
        fn click_rect(h: &mut Harness<'_, Window>, rect: egui::Rect) {
            let pos = rect.center();
            event(h, Event::PointerMoved(pos));
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            let pressed_at = h.ctx.input(|i| i.time);
            event(h, Event::PointerMoved(pos));
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            let held_for = h.ctx.input(|i| i.time) - pressed_at;
            let native_limit = h.ctx.options(|o| o.input_options.max_click_duration);
            assert!(held_for > 0.0 && held_for <= native_limit, "native fallback click: {held_for}s > {native_limit}s");
            assert!(h.ctx.input(|i| i.pointer.any_click()), "the native fallback must report a real click before looking for effects");
            idle(h, 2);
        }
        fn click(h: &mut Harness<'_, Window>, name: &str) {
            // Full-name lookup is intentionally unique; never select a same-label sibling by index.
            let rect = h.get_by_label(name).rect();
            click_rect(h, rect);
        }
        fn choose(h: &mut Harness<'_, Window>, name: &str, value: &str) {
            click(h, name);
            click(h, value);
        }
        fn type_text(h: &mut Harness<'_, Window>, name: &str, value: &str) {
            let rect = h.get_by_role_and_label(egui::accesskit::Role::TextInput, name).rect();
            click_rect(h, rect);
            key_mod(h, Key::A, Modifiers::COMMAND);
            event(h, Event::Text(value.to_owned()));
            idle(h, 2);
        }
        fn commit(h: &mut Harness<'_, Window>, name: &str, value: &str) {
            type_text(h, name, value);
            key(h, Key::Tab);
        }
        let mut app = app();
        let a = item(&mut app, 72);
        let b = item(&mut app, 172);
        select(&mut app, &[a, b]);
        let mut h = fixture(app, states, false);
        click(&mut h, "Convert Selection to Multi-State Object");
        let gid = h.state().app.session.active().unwrap().selection.items[0];
        click(&mut h, "Object States: State 2 (2)");
        assert_eq!(h.state().app.session.active().unwrap().doc.item(gid).unwrap().active_state, 1);
        click(&mut h, "Release to Objects");
        assert!(h.state().app.session.active().unwrap().doc.item(gid).unwrap().states.is_empty());
        no_replay(&mut h);
        let (app, id) = prepared_form("textField");
        let mut h = fixture(app, buttons, false);
        choose(&mut h, "On Release Action", "Go To URL");
        commit(&mut h, "Button URL", "https://example.com/native");
        assert_eq!(action(&h, id), Some(ButtonAction::GoToUrl { url: "https://example.com/native".into() }));
        commit(&mut h, "Form field name", "Native field");
        click(&mut h, "Required");
        click(&mut h, "Multiline");
        assert_eq!(field(&h, id).name, "Native field");
        assert!(field(&h, id).required && field(&h, id).multiline);
        choose(&mut h, "Form Field Type", "Combo Box");
        commit(&mut h, "Form field choices (separated by commas)", " One, ,Two ");
        assert_eq!(field(&h, id).options, vec!["One", "Two"]);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_forms_kind_change_and_button_action_change_cannot_recommit_the_old_editor() {
        let (mut app, id) = prepared_form("textField");
        app.run("button.set", json!({"action": "url", "url": "https://example.com/original"})).unwrap();
        let mut h = fixture(app, buttons, true);
        type_text(&mut h, "Button URL", "https://example.com/pending");
        choose(&mut h, "On Release Action", "Go To Next Page");
        assert_eq!(action(&h, id), Some(ButtonAction::GoToNextPage));
        assert_eq!(h.query_all_by_label("Button URL").count(), 0);
        no_replay(&mut h);
        type_text(&mut h, "Form field name", "Pending old kind");
        choose(&mut h, "Form Field Type", "Signature Field");
        assert_eq!(field(&h, id).kind, FieldKind::Signature);
        assert_eq!(h.query_all_by_label("Form field default value").count(), 0);
        no_replay(&mut h);
        commit(&mut h, "Form field name", "Fresh signature");
        assert_eq!(field(&h, id).kind, FieldKind::Signature);
        assert_eq!(field(&h, id).name, "Fresh signature");
        no_replay(&mut h);
    }

    #[test]
    fn ui223_forms_unlimited_unicode_draft_enter_then_undo_restores_authoritative_source() {
        let (app, id) = prepared_form("textField");
        let mut h = fixture(app, buttons, true);
        let text = format!("{} café", "Design field ".repeat(180));
        let before = undo(&h);
        type_text(&mut h, "Form field default value", &text);
        assert_eq!(field(&h, id).value, "Initial");
        assert_eq!(undo(&h), before);
        key(&mut h, Key::Enter);
        assert_eq!(field(&h, id).value, text);
        assert_eq!(undo(&h), before + 1);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 3);
        assert_eq!(field(&h, id).value, "Initial");
        assert_eq!(node(&h, "TextField", "Form field default value").facts.unwrap().value.as_deref(), Some("Initial"));
        no_replay(&mut h);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui223_liquid_workflow_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use designcraft_doc::{Fitting, ItemId, LiquidRule, ObjectLiquid};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{Harness, kittest::Queryable};
    use std::collections::HashSet;
    use std::sync::Arc;

    struct Window {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        parent: egui::Rect,
        enabled: bool,
        measuring: bool,
    }
    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"title": "Liquid workflow", "pages": 2, "facingPages": false})).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::pos2(500.0, 0.0), egui::vec2(500.0, 700.0)));
        app
    }
    fn item(app: &mut DesignApp, page: usize, x: i32) -> ItemId {
        let (si, _) = app.session.active().unwrap().doc.page_loc(page).unwrap();
        ItemId(app.run("frame.create", json!({"spread": si, "rect": [x, 72, x+60, 132]})).unwrap()["id"].as_u64().unwrap())
    }
    fn select(app: &mut DesignApp, ids: &[ItemId], page: usize) {
        app.run("selection.set", json!({"ids": ids.iter().map(|id| id.0).collect::<Vec<_>>()})).unwrap();
        // The real Page tool supplies this UI selection field; no control focus/value is forced.
        app.session.active_mut().unwrap().selection.page = Some(page);
        app.after_engine();
    }
    fn rule(app: &DesignApp, page: usize) -> LiquidRule {
        let doc = &app.session.active().unwrap().doc;
        let (si, pi) = doc.page_loc(page).unwrap();
        doc.spreads[si].pages[pi].liquid
    }
    fn constraints(app: &DesignApp, id: ItemId) -> Option<ObjectLiquid> {
        app.session.active().unwrap().doc.item(id).unwrap().liquid
    }
    fn graphic(app: &DesignApp, id: ItemId) -> designcraft_doc::Graphic {
        app.session.active().unwrap().doc.item(id).unwrap().graphic().unwrap().clone()
    }
    fn add_guide(app: &mut DesignApp, page: usize, orientation: &str, position: f64) {
        let (si, pi) = app.session.active().unwrap().doc.page_loc(page).unwrap();
        app.run("guide.add", json!({"spread": si, "page": pi, "orientation": orientation, "position": position})).unwrap();
        app.session.active_mut().unwrap().selection.page = Some(page);
    }
    fn guide(app: &DesignApp, page: usize, index: usize) -> designcraft_doc::Guide {
        let doc = &app.session.active().unwrap().doc;
        let (si, pi) = doc.page_loc(page).unwrap();
        doc.spreads[si].pages[pi].guides[index].clone()
    }
    fn guide_name(app: &DesignApp, page: usize, index: usize) -> String {
        let g = guide(app, page, index);
        format!("Liquid guide {}: {:?} at {} (Page {})", index + 1, g.orientation, g.position, app.session.active().unwrap().doc.page_name(page))
    }
    fn fixture(mut app: DesignApp, sdk: bool) -> Harness<'static, Window> {
        app.framework_transform = false;
        app.framework_opacity = sdk;
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        assert_eq!(app.framework_installed, sdk);
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(320.0, 720.0)).build_ui_state(
            move |ui, state: &mut Window| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), "");
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                state.parent = ui.max_rect();
                ui.add_enabled_ui(state.enabled, |ui| {
                    let ctx = ui.ctx().clone();
                    let mut render = || {
                        state.controls = record(ui, egui::Id::NULL.with("liquid-workflow"), |ui| liquid(&mut state.app, ui)).1;
                    };
                    if state.measuring {
                        craft_ui_widgets::runtime::measure::run(&ctx, render);
                    } else {
                        render();
                    }
                });
            },
            Window { app, controls: None, parent: egui::Rect::NOTHING, enabled: true, measuring: false },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }
    fn step(h: &mut Harness<'_, Window>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn event(h: &mut Harness<'_, Window>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn idle(h: &mut Harness<'_, Window>, n: usize) {
        for _ in 0..n {
            step(h);
        }
    }
    fn key(h: &mut Harness<'_, Window>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        idle(h, 2);
    }
    fn pointer(h: &mut Harness<'_, Window>, rect: egui::Rect, pressed: bool) {
        let pos = rect.center();
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        idle(h, 2);
    }
    fn click_rect(h: &mut Harness<'_, Window>, rect: egui::Rect) {
        pointer(h, rect, true);
        pointer(h, rect, false);
    }
    fn click(h: &mut Harness<'_, Window>, name: &str) {
        let rect = h.get_by_label(name).rect();
        click_rect(h, rect);
    }
    fn choose(h: &mut Harness<'_, Window>, value: &str) {
        click(h, "Liquid Page Rule");
        click(h, value);
    }
    fn node(h: &Harness<'_, Window>, component: &str, name: &str) -> Element {
        let nodes: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.part.is_none() && e.component == component && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(name))
            .cloned()
            .collect();
        assert_eq!(nodes.len(), 1, "exactly one actual {component} with full name {name:?}");
        nodes[0].clone()
    }
    fn tab_to(h: &mut Harness<'_, Window>, id: egui::Id) {
        let mut seen = HashSet::new();
        loop {
            let focus = h.ctx.memory(|m| m.focused());
            if focus == Some(id) {
                return;
            }
            assert!(seen.insert(focus), "ordinary Tab must reach {id:?}");
            key(h, Key::Tab);
        }
    }
    fn undo(h: &Harness<'_, Window>) -> usize {
        h.state().app.session.active().unwrap().history.undo.len()
    }
    fn journal(h: &Harness<'_, Window>) -> Vec<(String, Value)> {
        h.state().app.session.journal.clone()
    }
    fn last(h: &Harness<'_, Window>, command: &str, payload: Value) {
        assert_eq!(h.state().app.session.journal.last(), Some(&(command.to_owned(), payload)));
    }
    fn no_replay(h: &mut Harness<'_, Window>) {
        let before = (journal(h), undo(h));
        idle(h, 4);
        assert_eq!((journal(h), undo(h)), before);
    }
    fn object_fixture() -> (DesignApp, ItemId, ItemId) {
        let mut app = app();
        let a = item(&mut app, 0, 72);
        let b = item(&mut app, 0, 172);
        select(&mut app, &[a, b], 0);
        app.run("liquid.pageRule", json!({"rule": "objectBased", "pages": [1]})).unwrap();
        (app, a, b)
    }

    #[test]
    fn ui223_liquid_all_existing_page_rules_target_only_the_actual_page_and_undo() {
        let mut app = app();
        select(&mut app, &[], 1);
        let mut h = fixture(app, true);
        for (expected, caption) in [
            (LiquidRule::Scale, "Scale"),
            (LiquidRule::ReCenter, "Re-center"),
            (LiquidRule::ObjectBased, "Object-based"),
            (LiquidRule::GuideBased, "Guide-based"),
            (LiquidRule::Off, "Off"),
        ] {
            let before = undo(&h);
            choose(&mut h, caption);
            last(&h, "liquid.pageRule", json!({"rule": expected, "pages": [2]}));
            assert_eq!(rule(&h.state().app, 1), expected);
            assert_eq!(rule(&h.state().app, 0), LiquidRule::Off);
            assert_eq!(undo(&h), before + 1);
            no_replay(&mut h);
        }
        let before = (journal(&h), undo(&h));
        choose(&mut h, "Off");
        assert_eq!((journal(&h), undo(&h)), before, "current rule remains a no-op");
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 3);
        assert_eq!(rule(&h.state().app, 1), LiquidRule::GuideBased);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_liquid_real_select_natural_tab_preview_escape_and_enter_commit_once() {
        let mut app = app();
        select(&mut app, &[], 0);
        let mut h = fixture(app, true);
        let field = node(&h, "Select", "Liquid Page Rule");
        assert_eq!(field.facts.unwrap().role, "ComboBox");
        tab_to(&mut h, field.id);
        let before = (journal(&h), undo(&h));
        key(&mut h, Key::Space);
        key(&mut h, Key::ArrowDown);
        assert_eq!((journal(&h), undo(&h)), before);
        key(&mut h, Key::Escape);
        assert_eq!((journal(&h), undo(&h)), before);
        assert_eq!(rule(&h.state().app, 0), LiquidRule::Off);
        key(&mut h, Key::Space);
        key(&mut h, Key::Home);
        key(&mut h, Key::ArrowDown);
        key(&mut h, Key::Enter);
        last(&h, "liquid.pageRule", json!({"rule": "scale", "pages": [1]}));
        assert_eq!(undo(&h), before.1 + 1);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_liquid_all_six_constraint_defaults_payloads_full_selection_and_undo() {
        let (app, a, b) = object_fixture();
        let mut h = fixture(app, true);
        assert_eq!(constraints(&h.state().app, a), None);
        for (name, payload) in [
            ("Height", json!({"resizeHeight":false})),
            ("Width", json!({"resizeWidth":false})),
            ("Top", json!({"pinTop":true})),
            ("Left", json!({"pinLeft":true})),
            ("Bottom", json!({"pinBottom":true})),
            ("Right", json!({"pinRight":true})),
        ] {
            let control = node(&h, "Checkbox", name);
            assert!(!control.facts.as_ref().unwrap().disabled);
            let _ = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, name);
            let before = undo(&h);
            click(&mut h, name);
            last(&h, "liquid.object", payload);
            assert_eq!(undo(&h), before + 1);
            assert_eq!(constraints(&h.state().app, a), constraints(&h.state().app, b));
            no_replay(&mut h);
        }
        let actual = constraints(&h.state().app, a).unwrap();
        assert!(!actual.resize_height && !actual.resize_width && actual.pin_top && actual.pin_bottom && actual.pin_left && actual.pin_right);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 3);
        assert!(!constraints(&h.state().app, a).unwrap().pin_right);
    }

    #[test]
    fn ui223_liquid_parent_inheritance_wrong_page_and_absent_selection_preserve_eligibility() {
        let mut app = app();
        let id = item(&mut app, 0, 72);
        select(&mut app, &[id], 0);
        // Set up the existing document/parent domain in memory, without a file or fake widget callback.
        let (ps, pp) = app.session.active().unwrap().doc.parent_page_for(0).unwrap();
        {
            let doc = Arc::make_mut(&mut app.session.active_mut().unwrap().doc);
            let (si, pi) = doc.page_loc(0).unwrap();
            Arc::make_mut(&mut doc.spreads[si]).pages[pi].liquid = LiquidRule::ControlledByParent;
            Arc::make_mut(&mut doc.parents[ps]).pages[pp].liquid = LiquidRule::ObjectBased;
        }
        app.after_engine();
        let mut h = fixture(app, true);
        assert!(object_based(&h.state().app.session.active().unwrap().doc, 0));
        assert!(!node(&h, "Checkbox", "Right").facts.unwrap().disabled);
        click(&mut h, "Right");
        last(&h, "liquid.object", json!({"pinRight":true}));
        assert!(constraints(&h.state().app, id).unwrap().pin_right);
        select(&mut h.state_mut().app, &[id], 1);
        idle(&mut h, 3);
        assert!(node(&h, "Checkbox", "Right").facts.unwrap().disabled);
        let before = (journal(&h), undo(&h));
        click(&mut h, "Right");
        assert_eq!((journal(&h), undo(&h)), before);
        select(&mut h.state_mut().app, &[], 0);
        idle(&mut h, 3);
        assert!(node(&h, "Checkbox", "Right").facts.unwrap().disabled);
        select(&mut h.state_mut().app, &[id], 0);
        {
            let doc = Arc::make_mut(&mut h.state_mut().app.session.active_mut().unwrap().doc);
            Arc::make_mut(&mut doc.parents[ps]).pages[pp].liquid = LiquidRule::Off;
        }
        h.state_mut().app.after_engine();
        idle(&mut h, 3);
        assert!(node(&h, "Checkbox", "Right").facts.unwrap().disabled);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_liquid_real_graphic_autofit_defaults_full_selection_turnoff_and_undo() {
        let mut app = app();
        let a = item(&mut app, 0, 72);
        let b = item(&mut app, 0, 172);
        // Original, deterministic 2x1 RGBA PNG bytes, inline only; file.place consumes base64 in memory.
        const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8DwHwQBEPgD/U6VwW8AAAAASUVORK5CYII=";
        for id in [a, b] {
            app.run("file.place", json!({"base64":PNG,"name":"liquid-inline.png","frame":id.0})).unwrap();
        }
        select(&mut app, &[a, b], 0);
        assert_eq!(graphic(&app, a).auto_fit, Fitting::None, "real file.place-to-frame leaves Auto-Fit off, unlike grid placement");
        let mut h = fixture(app, true);
        assert!(!node(&h, "Checkbox", "Auto-Fit").facts.unwrap().disabled);
        let before = undo(&h);
        click(&mut h, "Auto-Fit");
        last(&h, "object.fittingOptions", json!({"autoFit":true}));
        assert_eq!(undo(&h), before + 1);
        assert_eq!(graphic(&h.state().app, a).auto_fit, Fitting::FillProportionally);
        assert_eq!(graphic(&h.state().app, b).auto_fit, Fitting::FillProportionally);
        let before_off = graphic(&h.state().app, a).xf;
        click(&mut h, "Auto-Fit");
        last(&h, "object.fittingOptions", json!({"autoFit":false}));
        assert_eq!(graphic(&h.state().app, a).auto_fit, Fitting::None);
        assert_eq!(graphic(&h.state().app, a).xf, before_off, "turning auto-fit off must not move the image");
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 3);
        assert_eq!(graphic(&h.state().app, a).auto_fit, Fitting::FillProportionally);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_liquid_guides_full_unique_names_raw_positions_exact_address_and_undo() {
        let mut app = app();
        add_guide(&mut app, 0, "vertical", 123.41);
        add_guide(&mut app, 0, "vertical", 123.44);
        add_guide(&mut app, 0, "vertical", 123.41);
        let names: Vec<_> = (0..3).map(|i| guide_name(&app, 0, i)).collect();
        assert_eq!(names.iter().collect::<HashSet<_>>().len(), 3);
        let mut h = fixture(app, true);
        let before = undo(&h);
        for name in &names {
            let _ = node(&h, "Checkbox", name);
            let _ = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, name);
        }
        click(&mut h, &names[1]);
        last(&h, "guide.liquid", json!({"spread":0,"page":0,"index":1,"on":true}));
        assert_eq!(undo(&h), before + 1);
        assert!(!guide(&h.state().app, 0, 0).liquid && guide(&h.state().app, 0, 1).liquid && !guide(&h.state().app, 0, 2).liquid);
        assert_eq!(guide(&h.state().app, 0, 1).position, 123.44);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 3);
        assert!(!guide(&h.state().app, 0, 1).liquid);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_liquid_held_guide_cancels_after_row_shift_and_fresh_target_recovers() {
        let mut app = app();
        add_guide(&mut app, 0, "vertical", 100.0);
        add_guide(&mut app, 0, "vertical", 200.0);
        add_guide(&mut app, 0, "vertical", 300.0);
        let held_name = guide_name(&app, 0, 1);
        let mut h = fixture(app, true);
        let held = node(&h, "Checkbox", &held_name);
        pointer(&mut h, held.rect, true);
        h.state_mut().app.run("guide.delete", json!({"spread":0,"page":0,"index":0})).unwrap();
        idle(&mut h, 3);
        let fresh_name = guide_name(&h.state().app, 0, 1);
        let fresh = node(&h, "Checkbox", &fresh_name);
        assert_ne!(held.id, fresh.id);
        assert_eq!(held.rect, fresh.rect, "old row coordinate now refers to a different guide");
        let before = (journal(&h), undo(&h));
        pointer(&mut h, fresh.rect, false);
        assert_eq!((journal(&h), undo(&h)), before);
        assert!(!guide(&h.state().app, 0, 1).liquid);
        click(&mut h, &fresh_name);
        last(&h, "guide.liquid", json!({"spread":0,"page":0,"index":1,"on":true}));
        assert!(guide(&h.state().app, 0, 1).liquid);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_liquid_held_constraint_cancels_on_full_selection_and_page_change() {
        let (app, a, b) = object_fixture();
        let mut h = fixture(app, true);
        select(&mut h.state_mut().app, &[a], 0);
        idle(&mut h, 3);
        let held = node(&h, "Checkbox", "Right");
        pointer(&mut h, held.rect, true);
        select(&mut h.state_mut().app, &[a, b], 0);
        idle(&mut h, 3);
        let fresh = node(&h, "Checkbox", "Right");
        assert_ne!(held.id, fresh.id);
        let before = (journal(&h), undo(&h));
        pointer(&mut h, fresh.rect, false);
        assert_eq!((journal(&h), undo(&h)), before);
        assert_eq!(constraints(&h.state().app, a), None);
        assert_eq!(constraints(&h.state().app, b), None);
        click(&mut h, "Right");
        assert!(constraints(&h.state().app, a).unwrap().pin_right && constraints(&h.state().app, b).unwrap().pin_right);
        let held = node(&h, "Checkbox", "Left");
        pointer(&mut h, held.rect, true);
        select(&mut h.state_mut().app, &[a, b], 1);
        idle(&mut h, 3);
        let changed = node(&h, "Checkbox", "Left");
        assert_ne!(held.id, changed.id);
        let before = (journal(&h), undo(&h));
        pointer(&mut h, changed.rect, false);
        assert_eq!((journal(&h), undo(&h)), before);
        select(&mut h.state_mut().app, &[a, b], 0);
        idle(&mut h, 3);
        click(&mut h, "Left");
        assert!(constraints(&h.state().app, a).unwrap().pin_left);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_liquid_same_uid_revert_and_new_document_cancel_held_input_with_fresh_recovery() {
        for preserve_uid in [false, true] {
            let (app, a, _) = object_fixture();
            let mut h = fixture(app, true);
            let held = node(&h, "Checkbox", "Width");
            pointer(&mut h, held.rect, true);
            let state = h.state().app.session.active().unwrap();
            let identity = (state.uid, state.identity_epoch);
            let mut replacement = designcraft_engine::DocState::new((*state.doc).clone(), None);
            replacement.selection = state.selection.clone();
            if preserve_uid {
                replacement.uid = identity.0;
                *h.state_mut().app.session.active_mut().unwrap() = replacement;
            } else {
                h.state_mut().app.session.add_document(replacement);
            }
            h.state_mut().app.after_engine();
            idle(&mut h, 3);
            let state = h.state().app.session.active().unwrap();
            assert_ne!(identity.1, state.identity_epoch);
            assert_eq!(state.uid == identity.0, preserve_uid);
            let fresh = node(&h, "Checkbox", "Width");
            assert_ne!(held.id, fresh.id);
            let before = (journal(&h), undo(&h));
            pointer(&mut h, fresh.rect, false);
            assert_eq!((journal(&h), undo(&h)), before);
            assert_eq!(constraints(&h.state().app, a), None);
            click(&mut h, "Width");
            last(&h, "liquid.object", json!({"resizeWidth":false}));
            assert!(!constraints(&h.state().app, a).unwrap().resize_width);
            no_replay(&mut h);
        }
    }

    #[test]
    fn ui223_liquid_disabled_measurement_bounds_absence_and_positive_recovery() {
        let (app, a, _) = object_fixture();
        let mut h = fixture(app, true);
        for (component, name) in [
            ("Select", "Liquid Page Rule"),
            ("Checkbox", "Auto-Fit"),
            ("Checkbox", "Height"),
            ("Checkbox", "Width"),
            ("Checkbox", "Top"),
            ("Checkbox", "Left"),
            ("Checkbox", "Bottom"),
            ("Checkbox", "Right"),
        ] {
            let control = node(&h, component, name);
            assert!(h.state().parent.expand(0.5).contains_rect(control.rect));
        }
        assert!(node(&h, "Checkbox", "Auto-Fit").facts.unwrap().disabled, "a plain frame is not a graphic");
        h.state_mut().enabled = false;
        idle(&mut h, 3);
        let right = node(&h, "Checkbox", "Right");
        assert!(right.facts.unwrap().disabled);
        let before = (journal(&h), undo(&h));
        click_rect(&mut h, right.rect);
        key(&mut h, Key::Space);
        assert_eq!((journal(&h), undo(&h)), before);
        h.state_mut().enabled = true;
        h.state_mut().measuring = true;
        idle(&mut h, 3);
        let before = (journal(&h), undo(&h));
        click_rect(&mut h, right.rect);
        assert_eq!((journal(&h), undo(&h)), before);
        h.state_mut().measuring = false;
        idle(&mut h, 3);
        click(&mut h, "Right");
        assert!(constraints(&h.state().app, a).unwrap().pin_right);
        no_replay(&mut h);
        h.state_mut().app.session.active_mut().unwrap().selection.page = Some(usize::MAX);
        idle(&mut h, 3);
        assert_eq!(h.query_all_by_label("Liquid Page Rule").count(), 0);
        select(&mut h.state_mut().app, &[a], 0);
        idle(&mut h, 3);
        click(&mut h, "Left");
        assert!(constraints(&h.state().app, a).unwrap().pin_left);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_liquid_real_native_fallback_retains_page_constraints_guides_and_undo() {
        // Harness defaults to 0.25 seconds per frame. The shared SDK pointer
        // helper holds for four frames (1.0 s), outside native's 0.8 s click
        // limit. Keep that helper and every passed SDK case unchanged; use a
        // genuine short native gesture here, with no input-policy/focus edits.
        fn native_click(h: &mut Harness<'_, Window>, name: &str) {
            let rect = h.get_by_label(name).rect();
            let pos = rect.center();
            event(h, Event::PointerMoved(pos));
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            let pressed_at = h.ctx.input(|i| i.time);
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            let held_for = h.ctx.input(|i| i.time) - pressed_at;
            let native_limit = h.ctx.options(|o| o.input_options.max_click_duration);
            assert!(held_for > 0.0 && held_for <= native_limit, "the actual native gesture must remain a click: {held_for}s > {native_limit}s");
            assert!(h.ctx.input(|i| i.pointer.any_click()), "the real egui input path must report a click before looking for effects");
            idle(h, 2);
        }
        let mut app = app();
        let id = item(&mut app, 0, 72);
        add_guide(&mut app, 0, "horizontal", 180.0);
        select(&mut app, &[id], 0);
        let name = guide_name(&app, 0, 0);
        let mut h = fixture(app, false);
        native_click(&mut h, "Liquid Page Rule");
        assert_eq!(
            h.query_all_by_role(egui::accesskit::Role::ListBox).count(),
            1,
            "the real native ComboBox popup must be present before choosing a real option"
        );
        native_click(&mut h, "Object-based");
        last(&h, "liquid.pageRule", json!({"rule":"objectBased","pages":[1]}));
        native_click(&mut h, "Right");
        native_click(&mut h, "Width");
        let actual = constraints(&h.state().app, id).unwrap();
        assert!(actual.pin_right && !actual.resize_width && actual.resize_height);
        native_click(&mut h, &name);
        last(&h, "guide.liquid", json!({"spread":0,"page":0,"index":0,"on":true}));
        assert!(guide(&h.state().app, 0, 0).liquid);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 3);
        assert!(!guide(&h.state().app, 0, 0).liquid);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_liquid_open_rule_popup_cancels_on_page_change_and_accepts_fresh_input() {
        fn click(h: &mut Harness<'_, Window>, name: &str) {
            // The trigger and its expanded list share a name; bind to the actual control role.
            let role = if name == "Liquid Page Rule" { egui::accesskit::Role::ComboBox } else { egui::accesskit::Role::ListBoxOption };
            let rect = h.get_by_role_and_label(role, name).rect();
            click_rect(h, rect);
        }
        fn choose(h: &mut Harness<'_, Window>, value: &str, expected: egui::Id) {
            use egui_kittest::kittest::NodeT as _;
            let tree = h.root().accesskit_node().locate().1;
            let current = node(h, "Select", "Liquid Page Rule");
            assert_eq!(current.id, expected, "fresh choice stays on the actual new page control");
            assert!(current.facts.as_ref().is_some_and(|facts| !facts.disabled));
            let expanded = {
                let trigger = h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Liquid Page Rule");
                let access = trigger.accesskit_node();
                assert_eq!(access.locate(), (current.id.accesskit_id(), tree));
                assert!(!access.is_disabled());
                access.data().is_expanded().expect("the actual Select publishes its expanded state")
            };
            // Enter and the face are toggles. Open a closed list; never toggle an already open list shut.
            if !expanded {
                click(h, "Liquid Page Rule");
            }
            assert_eq!(node(h, "Select", "Liquid Page Rule").id, expected);
            let rect = {
                let trigger = h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Liquid Page Rule");
                let access = trigger.accesskit_node();
                assert_eq!(access.locate(), (expected.accesskit_id(), tree));
                assert!(!access.is_disabled());
                assert_eq!(access.data().is_expanded(), Some(true));
                let popup = h.get_by_role_and_label(egui::accesskit::Role::ListBox, "Liquid Page Rule");
                let mut ancestor = Some(popup.clone());
                let mut owned = false;
                while let Some(node) = ancestor {
                    if access.controls().any(|root| root.id() == node.accesskit_node().id()) {
                        owned = true;
                        break;
                    }
                    ancestor = node.parent();
                }
                assert!(owned, "the actual current trigger owns this popup or one of its ancestors");
                let option = popup.get_by_role_and_label(egui::accesskit::Role::ListBoxOption, value);
                assert!(!option.accesskit_node().is_disabled());
                option.rect()
            };
            click_rect(h, rect);
        }
        let mut app = app();
        select(&mut app, &[], 0);
        let mut h = fixture(app, true);
        let first = node(&h, "Select", "Liquid Page Rule");
        click(&mut h, "Liquid Page Rule");
        key(&mut h, Key::ArrowDown);
        select(&mut h.state_mut().app, &[], 1);
        idle(&mut h, 3);
        let next = node(&h, "Select", "Liquid Page Rule");
        assert_ne!(first.id, next.id);
        let before = (journal(&h), undo(&h));
        key(&mut h, Key::Enter);
        assert_eq!((journal(&h), undo(&h)), before);
        assert_eq!(rule(&h.state().app, 0), LiquidRule::Off);
        assert_eq!(rule(&h.state().app, 1), LiquidRule::Off);
        choose(&mut h, "Scale", next.id);
        last(&h, "liquid.pageRule", json!({"rule": "scale", "pages": [2]}));
        assert_eq!(rule(&h.state().app, 1), LiquidRule::Scale);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_liquid_no_document_has_no_actions_and_new_document_recovers_normally() {
        let app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let mut h = fixture(app, true);
        assert_eq!(h.query_all_by_label("Liquid Page Rule").count(), 0);
        // The notice is framework text; no control or action stands beside it.
        assert!(h.state().controls.as_ref().unwrap().elements.iter().all(|e| e.component == "Text"));
        let before = journal(&h);
        key(&mut h, Key::Tab);
        key(&mut h, Key::Space);
        assert_eq!(journal(&h), before);
        h.state_mut().app.run("file.new", json!({"pages": 1})).unwrap();
        select(&mut h.state_mut().app, &[], 0);
        idle(&mut h, 3);
        choose(&mut h, "Object-based");
        last(&h, "liquid.pageRule", json!({"rule": "objectBased", "pages": [1]}));
        assert_eq!(rule(&h.state().app, 0), LiquidRule::ObjectBased);
        no_replay(&mut h);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui223_track_workflow_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use designcraft_doc::StoryId;
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{Harness, kittest::Queryable};
    use std::{collections::HashSet, sync::Arc};
    struct Window {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        parent: egui::Rect,
        enabled: bool,
        measuring: bool,
    }
    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages":1,"title":"Track workflow"})).unwrap();
        app
    }
    fn frame(app: &mut DesignApp, text: &str) -> StoryId {
        StoryId(
            app.run("frame.create", json!({"rect":[72,72,400,200],"content":"text","text":text,"caret":false})).unwrap()["story"].as_u64().unwrap(),
        )
    }
    fn tracked() -> (DesignApp, StoryId) {
        let mut app = app();
        let sid = frame(&mut app, "The cat sat");
        app.run("changes.track", json!({"on":true})).unwrap();
        app.run("text.select", json!({"story":sid.0,"anchor":4,"focus":7})).unwrap();
        app.run("text.insert", json!({"text":"dog"})).unwrap();
        (app, sid)
    }
    fn list(app: &mut DesignApp) -> Vec<Value> {
        app.session.execute("changes.list", &json!({})).unwrap().as_array().unwrap().clone()
    }
    fn text(app: &DesignApp, sid: StoryId) -> String {
        app.session.active().unwrap().doc.story(sid).unwrap().text.clone()
    }
    fn doc(app: &DesignApp) -> Value {
        serde_json::to_value(app.session.active().unwrap().doc.as_ref()).unwrap()
    }
    fn paint(shape: &egui::Shape, out: &mut Vec<(String, bool)>) {
        match shape {
            egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.galley.job.sections.iter().all(|s| s.format.strikethrough.width > 0.0))),
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    paint(s, out);
                }
            }
            _ => {}
        }
    }
    fn fixture(mut app: DesignApp, sdk: bool) -> Harness<'static, Window> {
        app.framework_transform = false;
        app.framework_opacity = sdk;
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        assert_eq!(app.framework_installed, sdk);
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(320.0, 720.0)).build_ui_state(
            move |ui, state: &mut Window| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), "");
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                state.parent = ui.max_rect();
                ui.add_enabled_ui(state.enabled, |ui| {
                    let ctx = ui.ctx().clone();
                    let mut render = || {
                        state.controls = record(ui, egui::Id::NULL.with("track-workflow"), |ui| track_changes(&mut state.app, ui)).1;
                    };
                    if state.measuring {
                        craft_ui_widgets::runtime::measure::run(&ctx, render);
                    } else {
                        render();
                    }
                });
            },
            Window { app, controls: None, parent: egui::Rect::NOTHING, enabled: true, measuring: false },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }
    fn step(h: &mut Harness<'_, Window>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn event(h: &mut Harness<'_, Window>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn idle(h: &mut Harness<'_, Window>, n: usize) {
        for _ in 0..n {
            step(h);
        }
    }
    fn key(h: &mut Harness<'_, Window>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        idle(h, 2);
    }
    fn pointer(h: &mut Harness<'_, Window>, rect: egui::Rect, pressed: bool) {
        let pos = rect.center();
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        idle(h, 2);
    }
    fn click_rect(h: &mut Harness<'_, Window>, rect: egui::Rect) {
        pointer(h, rect, true);
        pointer(h, rect, false);
    }
    fn click(h: &mut Harness<'_, Window>, name: &str) {
        let rect = h.get_by_label(name).rect();
        click_rect(h, rect);
    }

    fn node(h: &Harness<'_, Window>, component: &str, name: &str) -> Element {
        let nodes: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.part.is_none() && e.component == component && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(name))
            .cloned()
            .collect();
        assert_eq!(nodes.len(), 1, "exactly one actual {component} with full name {name:?}");
        nodes[0].clone()
    }
    fn tab_to(h: &mut Harness<'_, Window>, id: egui::Id) {
        let mut seen = HashSet::new();
        loop {
            let focus = h.ctx.memory(|m| m.focused());
            if focus == Some(id) {
                return;
            }
            assert!(seen.insert(focus), "ordinary Tab must reach {id:?}");
            key(h, Key::Tab);
        }
    }
    fn undo(h: &Harness<'_, Window>) -> usize {
        h.state().app.session.active().unwrap().history.undo.len()
    }
    fn journal(h: &Harness<'_, Window>) -> Vec<(String, Value)> {
        h.state().app.session.journal.clone()
    }
    fn last(h: &Harness<'_, Window>, command: &str, payload: Value) {
        assert_eq!(h.state().app.session.journal.last(), Some(&(command.to_owned(), payload)));
    }
    fn no_replay(h: &mut Harness<'_, Window>) {
        let before = (journal(h), undo(h));
        idle(h, 4);
        assert_eq!((journal(h), undo(h)), before);
    }

    fn track_native_click(h: &mut Harness<'_, Window>, name: &str) {
        let rect = h.get_by_label(name).rect();
        let pos = rect.center();
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        let pressed_at = h.ctx.input(|i| i.time);
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        let held_for = h.ctx.input(|i| i.time) - pressed_at;
        let native_limit = h.ctx.options(|o| o.input_options.max_click_duration);
        assert!(held_for > 0.0 && held_for <= native_limit, "the actual native gesture must remain a click: {held_for}s > {native_limit}s");
        assert!(h.ctx.input(|i| i.pointer.any_click()), "the real egui input path must report a click before looking for effects");
        idle(h, 2);
    }

    #[test]
    fn ui223_track_checkbox_default_pointer_payload_undo_and_no_idle_edit() {
        let app = app();
        assert!(!app.session.active().unwrap().doc.settings.track_changes);
        let mut h = fixture(app, true);
        let control = node(&h, "Checkbox", "Track Changes in All Stories");
        assert_eq!(control.facts.unwrap().role, "CheckBox");
        let before = undo(&h);
        click(&mut h, "Track Changes in All Stories");
        last(&h, "changes.track", json!({"on":true}));
        assert!(h.state().app.session.active().unwrap().doc.settings.track_changes);
        assert_eq!(undo(&h), before + 1);
        assert_eq!(h.query_all_by_label("Accept All").count(), 0);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 3);
        assert!(!h.state().app.session.active().unwrap().doc.settings.track_changes);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_track_real_excerpt_selects_exact_story_full_range_without_document_or_history_edit() {
        let (mut app, sid) = tracked();
        let changes = list(&mut app);
        let change = changes.iter().find(|c| c["kind"] == "deleted").unwrap().clone();
        let protected = designcraft_engine::DocState::new((*app.session.active().unwrap().doc).clone(), None);
        app.session.add_document(protected);
        app.session.set_active(0);
        app.after_engine();
        let other_doc = serde_json::to_value(app.session.documents()[1].doc.as_ref()).unwrap();
        let other_selection = app.session.documents()[1].selection.clone();
        let mut h = fixture(app, true);
        let before_doc = doc(&h.state().app);
        let before = undo(&h);
        track_native_click(&mut h, &track_row_name("Show in the text", &change));
        last(&h, "text.select", json!({"story":sid.0,"anchor":change["start"],"focus":change["end"]}));
        let selection = h.state().app.session.active().unwrap().selection.text.as_ref().unwrap();
        assert_eq!(selection.story, sid);
        assert_eq!(selection.anchor, 4);
        assert_eq!(selection.focus, 7);
        assert_eq!(doc(&h.state().app), before_doc);
        assert_eq!(undo(&h), before);
        assert_eq!(serde_json::to_value(h.state().app.session.documents()[1].doc.as_ref()).unwrap(), other_doc);
        assert_eq!(h.state().app.session.documents()[1].selection, other_selection);
        no_replay(&mut h);
    }

    #[test]
    fn ui223_track_each_accept_reject_kind_has_real_domain_effect_and_exact_undo() {
        for (kind, action, command, expected) in [
            ("deleted", "Accept", "changes.accept", "The dog sat"),
            ("deleted", "Reject", "changes.reject", "The catdog sat"),
            ("inserted", "Accept", "changes.accept", "The catdog sat"),
            ("inserted", "Reject", "changes.reject", "The cat sat"),
        ] {
            let (mut app, sid) = tracked();
            let change = list(&mut app).into_iter().find(|c| c["kind"] == kind).unwrap();
            let mut h = fixture(app, true);
            let before_doc = doc(&h.state().app);
            let before = undo(&h);
            let name = track_row_name(action, &change);
            let control = node(&h, "Button", &name);
            assert_eq!(control.facts.unwrap().role, "Button");
            let _ = h.get_by_role_and_label(egui::accesskit::Role::Button, &name);
            click(&mut h, &name);
            last(&h, command, json!({"story":change["story"],"start":change["start"]}));
            assert_eq!(text(&h.state().app, sid), expected);
            assert_eq!(undo(&h), before + 1);
            let remaining = list(&mut h.state_mut().app);
            assert_eq!(remaining.len(), 1);
            assert!(remaining.iter().all(|c| c["kind"] != kind));
            h.state_mut().app.run("edit.undo", json!({})).unwrap();
            idle(&mut h, 3);
            assert_eq!(doc(&h.state().app), before_doc);
            assert_eq!(undo(&h), before);
            no_replay(&mut h);
        }
    }

    #[test]
    fn ui223_track_bulk_accept_reject_all_stories_not_other_document_and_undo_once() {
        for (action, command, expected) in [("Accept All", "changes.acceptAll", "The dog sat"), ("Reject All", "changes.rejectAll", "The cat sat")] {
            let (mut app, sid) = tracked();
            let second = frame(&mut app, "Second");
            app.run("text.select", json!({"story":second.0,"anchor":6,"focus":6})).unwrap();
            app.run("text.insert", json!({"text":" added"})).unwrap();
            let protected = designcraft_engine::DocState::new((*app.session.active().unwrap().doc).clone(), None);
            app.session.add_document(protected);
            app.session.set_active(0);
            app.after_engine();
            let protected_doc = serde_json::to_value(app.session.documents()[1].doc.as_ref()).unwrap();
            let protected_history = app.session.documents()[1].history.undo.len();
            let mut h = fixture(app, true);
            let before_doc = doc(&h.state().app);
            let before = undo(&h);
            click(&mut h, action);
            last(&h, command, json!({}));
            assert_eq!(text(&h.state().app, sid), expected);
            assert_eq!(text(&h.state().app, second), if action == "Accept All" { "Second added" } else { "Second" });
            assert!(list(&mut h.state_mut().app).is_empty());
            assert_eq!(undo(&h), before + 1);
            assert_eq!(serde_json::to_value(h.state().app.session.documents()[1].doc.as_ref()).unwrap(), protected_doc);
            assert_eq!(h.state().app.session.documents()[1].history.undo.len(), protected_history);
            h.state_mut().app.run("edit.undo", json!({})).unwrap();
            idle(&mut h, 3);
            assert_eq!(doc(&h.state().app), before_doc);
            no_replay(&mut h);
        }
    }

    #[test]
    fn ui223_track_excerpt_preserves_forty_unicode_scalars_newline_marker_and_deleted_strike() {
        // The framework draws the excerpt as its own component (ChangeExcerpt), which paints the same struck caption
        // the native remainder does; only whether the excerpt is a framework element differs between the two.
        for sdk in [false, true] {
            let mut app = app();
            let original = format!("{}TAIL MUST NOT APPEAR", "é\n".repeat(25));
            let sid = frame(&mut app, &original);
            app.run("changes.track", json!({"on":true})).unwrap();
            app.run("text.select", json!({"story":sid.0,"anchor":0,"focus":original.len()})).unwrap();
            app.run("text.delete", json!({})).unwrap();
            let change = list(&mut app).into_iter().next().unwrap();
            assert_eq!(change["text"], original);
            let preview = original.chars().take(40).collect::<String>().replace('\n', "¶");
            let mut h = fixture(app, sdk);
            let name = track_row_name("Show in the text", &change);
            let _ = h.get_by_label(&name);
            assert!(!name.contains("TAIL MUST NOT APPEAR"));
            let reported =
                h.state().controls.as_ref().unwrap().elements.iter().any(|e| e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(name.as_str()));
            assert_eq!(
                reported, sdk,
                "the excerpt is a framework action exactly when the framework draws it; the native remainder is never reported as one"
            );
            let mut painted = Vec::new();
            for clipped in &h.output().shapes {
                paint(&clipped.shape, &mut painted);
            }
            assert!(
                painted.iter().any(|(text, struck)| text == &format!("Deleted “{preview}”") && *struck),
                "actual paint must preserve all caption/preview strike-through, not just an accessibility string: {painted:?}"
            );
            track_native_click(&mut h, &name);
            last(&h, "text.select", json!({"story":sid.0,"anchor":0,"focus":original.len()}));
            no_replay(&mut h);
        }
    }

    #[test]
    fn ui223_track_natural_tab_enter_space_ready_actions_commit_once() {
        let (mut app, _) = tracked();
        let change = list(&mut app).into_iter().find(|c| c["kind"] == "inserted").unwrap();
        let mut h = fixture(app, true);
        let name = track_row_name("Accept", &change);
        let control = node(&h, "Button", &name);
        tab_to(&mut h, control.id);
        let before = undo(&h);
        key(&mut h, Key::Enter);
        assert_eq!(undo(&h), before + 1);
        last(&h, "changes.accept", json!({"story":change["story"],"start":change["start"]}));
        let bulk = node(&h, "Button", "Reject All");
        tab_to(&mut h, bulk.id);
        let before = undo(&h);
        key(&mut h, Key::Space);
        assert_eq!(undo(&h), before + 1);
        last(&h, "changes.rejectAll", json!({}));
        assert!(list(&mut h.state_mut().app).is_empty());
        no_replay(&mut h);
    }

    #[test]
    fn ui223_track_held_row_same_preview_full_span_but_changed_tail_cancels_and_recovers() {
        let mut app = app();
        let sid = frame(&mut app, "");
        app.run("changes.track", json!({"on":true})).unwrap();
        app.run("text.select", json!({"story":sid.0,"anchor":0,"focus":0})).unwrap();
        let first = format!("{}x", "A".repeat(40));
        app.run("text.insert", json!({"text":first})).unwrap();
        let old = list(&mut app).remove(0);
        let name = track_row_name("Accept", &old);
        let mut h = fixture(app, true);
        let held = node(&h, "Button", &name);
        pointer(&mut h, held.rect, true);
        h.state_mut().app.run("text.select", json!({"story":sid.0,"anchor":0,"focus":41})).unwrap();
        h.state_mut().app.run("text.insert", json!({"text":format!("{}y","A".repeat(40))})).unwrap();
        idle(&mut h, 3);
        let new = list(&mut h.state_mut().app).remove(0);
        assert_eq!(old["start"], new["start"]);
        assert_eq!(old["end"], new["end"]);
        assert_eq!(old["kind"], new["kind"]);
        assert_ne!(old["text"], new["text"]);
        assert_eq!(track_row_name("Accept", &new), name);
        let fresh = node(&h, "Button", &name);
        assert_ne!(held.id, fresh.id);
        let before = (journal(&h), undo(&h));
        pointer(&mut h, fresh.rect, false);
        assert_eq!((journal(&h), undo(&h)), before);
        click(&mut h, &name);
        last(&h, "changes.accept", json!({"story":sid.0,"start":0}));
        assert!(list(&mut h.state_mut().app).is_empty());
        no_replay(&mut h);
    }

    #[test]
    fn ui223_track_held_bulk_collection_change_cancels_instead_of_resolving_new_snapshot() {
        let (mut app, _) = tracked();
        let deleted = list(&mut app).into_iter().find(|c| c["kind"] == "deleted").unwrap();
        let mut h = fixture(app, true);
        let held = node(&h, "Button", "Accept All");
        pointer(&mut h, held.rect, true);
        h.state_mut().app.run("changes.reject", json!({"story":deleted["story"],"start":deleted["start"]})).unwrap();
        idle(&mut h, 3);
        assert_eq!(list(&mut h.state_mut().app).len(), 1);
        let fresh = node(&h, "Button", "Accept All");
        assert_ne!(held.id, fresh.id);
        let before = (journal(&h), undo(&h));
        pointer(&mut h, fresh.rect, false);
        assert_eq!((journal(&h), undo(&h)), before);
        click(&mut h, "Accept All");
        last(&h, "changes.acceptAll", json!({}));
        assert!(list(&mut h.state_mut().app).is_empty());
        no_replay(&mut h);
    }

    #[test]
    fn ui223_track_same_uid_replacement_switch_and_close_never_retarget_held_action() {
        for preserve_uid in [true, false] {
            let (mut app, _) = tracked();
            let change = list(&mut app).remove(0);
            let name = track_row_name("Reject", &change);
            let mut h = fixture(app, true);
            let held = node(&h, "Button", &name);
            pointer(&mut h, held.rect, true);
            let old = h.state().app.session.active().unwrap();
            let identity = (old.uid, old.identity_epoch);
            let protected_doc = doc(&h.state().app);
            let mut replacement = designcraft_engine::DocState::new((*old.doc).clone(), None);
            replacement.selection = old.selection.clone();
            if preserve_uid {
                replacement.uid = identity.0;
                *h.state_mut().app.session.active_mut().unwrap() = replacement;
            } else {
                h.state_mut().app.session.add_document(replacement);
            }
            h.state_mut().app.after_engine();
            idle(&mut h, 3);
            let active = h.state().app.session.active().unwrap();
            assert_ne!(active.identity_epoch, identity.1);
            assert_eq!(active.uid == identity.0, preserve_uid);
            let fresh = node(&h, "Button", &name);
            assert_ne!(held.id, fresh.id);
            let before = (journal(&h), undo(&h));
            pointer(&mut h, fresh.rect, false);
            assert_eq!((journal(&h), undo(&h)), before);
            if !preserve_uid {
                assert_eq!(serde_json::to_value(h.state().app.session.documents()[0].doc.as_ref()).unwrap(), protected_doc);
            }
            click(&mut h, &name);
            last(&h, "changes.reject", json!({"story":change["story"],"start":change["start"]}));
            let bulk = node(&h, "Button", "Accept All");
            pointer(&mut h, bulk.rect, true);
            let active = h.state().app.session.active_index().unwrap();
            h.state_mut().app.session.close_document(active);
            h.state_mut().app.after_engine();
            idle(&mut h, 3);
            let before = journal(&h);
            pointer(&mut h, bulk.rect, false);
            assert_eq!(journal(&h), before, "closing cannot release a held bulk action into another document");
        }
    }

    #[test]
    fn ui223_track_disabled_measurement_narrow_parent_and_positive_recovery() {
        let (mut app, _) = tracked();
        let change = list(&mut app).remove(0);
        let name = track_row_name("Accept", &change);
        let mut h = fixture(app, true);
        for element in h.state().controls.as_ref().unwrap().elements.iter().filter(|e| e.part.is_none()) {
            assert!(h.state().parent.expand(0.5).contains_rect(element.rect), "{} {:?}", element.component, element.rect);
        }
        h.state_mut().enabled = false;
        idle(&mut h, 3);
        let action = node(&h, "Button", &name);
        assert!(action.facts.unwrap().disabled);
        let before = (journal(&h), undo(&h));
        click_rect(&mut h, action.rect);
        assert_eq!((journal(&h), undo(&h)), before);
        h.state_mut().enabled = true;
        h.state_mut().measuring = true;
        idle(&mut h, 3);
        let before = (journal(&h), undo(&h));
        click_rect(&mut h, action.rect);
        assert_eq!((journal(&h), undo(&h)), before);
        h.state_mut().measuring = false;
        idle(&mut h, 3);
        click(&mut h, &name);
        last(&h, "changes.accept", json!({"story":change["story"],"start":change["start"]}));
        no_replay(&mut h);
    }

    #[test]
    fn ui223_track_no_document_empty_uninstalled_and_real_native_fallback() {
        let empty = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let mut h = fixture(empty, true);
        assert_eq!(h.query_all_by_label("Track Changes in All Stories").count(), 0);
        assert!(h.state().controls.as_ref().unwrap().elements.is_empty());
        let before = journal(&h);
        key(&mut h, Key::Tab);
        key(&mut h, Key::Enter);
        assert_eq!(journal(&h), before);
        let (mut app, sid) = tracked();
        let change = list(&mut app).into_iter().find(|c| c["kind"] == "deleted").unwrap();
        let mut h = fixture(app, false);
        let name = track_row_name("Reject", &change);
        track_native_click(&mut h, &name);
        last(&h, "changes.reject", json!({"story":change["story"],"start":change["start"]}));
        track_native_click(&mut h, "Accept All");
        last(&h, "changes.acceptAll", json!({}));
        assert_eq!(text(&h.state().app, sid), "The catdog sat");
        assert!(list(&mut h.state_mut().app).is_empty());
        no_replay(&mut h);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui223_tags_xml_workflow_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use designcraft_doc::{ItemId, StoryId};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{Harness, kittest::Queryable};
    use std::{collections::HashSet, sync::Arc};
    struct Window {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        parent: egui::Rect,
        enabled: bool,
        measuring: bool,
    }
    fn app() -> DesignApp {
        let services = crate::Services {
            read: Some(Box::new(|path| std::fs::read(path).map_err(|e| e.to_string()))),
            write: Some(Box::new(|path, bytes| std::fs::write(path, bytes).map_err(|e| e.to_string()))),
            ..Default::default()
        };
        let mut app = DesignApp::new(designcraft_engine::Session::new(), services);
        app.run("file.new", json!({"pages": 1, "title": "Tags workflow"})).unwrap();
        app
    }
    fn frame(app: &mut DesignApp, text: &str, y: f64) -> (StoryId, ItemId) {
        let v = app.run("frame.create", json!({"rect":[72,y,400,100],"content":"text","text":text,"caret":false})).unwrap();
        (StoryId(v["story"].as_u64().unwrap()), ItemId(v["id"].as_u64().unwrap()))
    }
    fn body() -> (DesignApp, StoryId, ItemId) {
        let mut app = app();
        let (sid, id) = frame(&mut app, "The cat sat", 72.0);
        app.run("xml.tag", json!({"tag":"Body", "ids":[id.0]})).unwrap();
        app.run("selection.set", json!({"ids":[id.0]})).unwrap();
        (app, sid, id)
    }
    fn text(app: &DesignApp, sid: StoryId) -> String {
        app.session.active().unwrap().doc.story(sid).unwrap().text.clone()
    }
    fn xml(app: &mut DesignApp) -> String {
        app.session.execute("file.exportXml", &json!({})).unwrap()["text"].as_str().unwrap().to_owned()
    }
    fn names(app: &mut DesignApp) -> Vec<String> {
        app.session.execute("xml.tags", &json!({})).unwrap()["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect()
    }
    fn apply_name(tag: &str) -> String {
        format!("Tag the selection <{tag}>")
    }
    fn structure_name(app: &mut DesignApp, id: ItemId) -> String {
        let structure = app.session.execute("xml.structure", &json!({})).unwrap();
        let el = structure["elements"].as_array().unwrap().iter().find(|el| el["id"] == json!(id.0)).unwrap();
        let preview: String = el["text"].as_str().unwrap_or("").chars().take(40).collect();
        format!("Structure <{}> {} (Object {})", el["tag"].as_str().unwrap(), preview, id.0)
    }
    fn painted(h: &Harness<'_, Window>) -> Vec<String> {
        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => {
                    for s in v {
                        collect(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for shape in &h.output().shapes {
            collect(&shape.shape, &mut out);
        }
        out
    }
    fn fixture(mut app: DesignApp, sdk: bool) -> Harness<'static, Window> {
        app.framework_transform = false;
        app.framework_opacity = sdk;
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        assert_eq!(app.framework_installed, sdk);
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(320.0, 720.0)).build_ui_state(
            move |ui, state: &mut Window| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), "");
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                state.parent = ui.max_rect();
                ui.add_enabled_ui(state.enabled, |ui| {
                    let ctx = ui.ctx().clone();
                    let mut render = || {
                        state.controls = record(ui, egui::Id::NULL.with("tags-xml-workflow"), |ui| tags(&mut state.app, ui)).1;
                    };
                    if state.measuring {
                        craft_ui_widgets::runtime::measure::run(&ctx, render);
                    } else {
                        render();
                    }
                });
            },
            Window { app, controls: None, parent: egui::Rect::NOTHING, enabled: true, measuring: false },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }
    fn step(h: &mut Harness<'_, Window>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn event(h: &mut Harness<'_, Window>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn idle(h: &mut Harness<'_, Window>, n: usize) {
        for _ in 0..n {
            step(h);
        }
    }
    fn key(h: &mut Harness<'_, Window>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        idle(h, 2);
    }
    fn pointer(h: &mut Harness<'_, Window>, rect: egui::Rect, pressed: bool) {
        let pos = rect.center();
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        idle(h, 2);
    }
    fn click_rect(h: &mut Harness<'_, Window>, rect: egui::Rect) {
        let pos = rect.center();
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        assert!(h.ctx.input(|i| i.pointer.any_click()), "a genuine short native click must have occurred");
        idle(h, 2);
    }
    fn click(h: &mut Harness<'_, Window>, name: &str) {
        let rect = h.get_by_label(name).rect();
        click_rect(h, rect);
    }

    fn node(h: &Harness<'_, Window>, component: &str, name: &str) -> Element {
        let nodes: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.part.is_none() && e.component == component && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(name))
            .cloned()
            .collect();
        assert_eq!(nodes.len(), 1, "exactly one actual {component} with full name {name:?}");
        nodes[0].clone()
    }
    fn tab_to(h: &mut Harness<'_, Window>, id: egui::Id) {
        use egui_kittest::kittest::{NodeT as _, Queryable as _};
        let tree = h.root().accesskit_node().locate().1;
        let targets: Vec<_> =
            h.state().controls.as_ref().unwrap().elements.iter().filter(|element| element.id == id && element.part.is_none()).cloned().collect();
        assert_eq!(targets.len(), 1, "ordinary Tab has exactly one current SDK target");
        let target = &targets[0];
        let facts = target.facts.as_ref().expect("the actual target reports its control facts");
        let name = facts.name.as_deref().expect("the actual target has its full name");
        assert!(!facts.disabled);
        let role = match target.component {
            "TextField" => egui::accesskit::Role::TextInput,
            "Button" => egui::accesskit::Role::Button,
            "Link" => egui::accesskit::Role::Link,
            component => panic!("unqualified Tags Tab target {component}"),
        };
        let mut seen = HashSet::new();
        loop {
            let current = node(h, target.component, name);
            assert_eq!(current.id, id, "ordinary Tab stays on the actual current target");
            assert!(current.facts.as_ref().is_some_and(|facts| !facts.disabled));
            let reached = {
                let actual = h.get_by_role_and_label(role, name);
                let access = actual.accesskit_node();
                assert_eq!(access.locate(), (id.accesskit_id(), tree));
                assert!(!access.is_disabled());
                access.is_focused()
            };
            if reached {
                return;
            }
            let focused: Vec<_> = {
                let root = h.queryable_node();
                std::iter::once(root.clone())
                    .chain(root.children_recursive())
                    .filter_map(|node| {
                        let access = node.accesskit_node();
                        access.is_focused().then(|| access.id())
                    })
                    .collect()
            };
            assert!(seen.insert(focused), "ordinary Tab must reach the actual {role:?} named {name:?}");
            key(h, Key::Tab);
        }
    }
    fn undo(h: &Harness<'_, Window>) -> usize {
        h.state().app.session.active().unwrap().history.undo.len()
    }
    fn journal(h: &Harness<'_, Window>) -> Vec<(String, Value)> {
        h.state().app.session.journal.clone()
    }
    fn last(h: &Harness<'_, Window>, command: &str, payload: Value) {
        assert_eq!(h.state().app.session.journal.last(), Some(&(command.to_owned(), payload)));
    }
    fn no_replay(h: &mut Harness<'_, Window>) {
        let before = (journal(h), undo(h));
        idle(h, 4);
        assert_eq!((journal(h), undo(h)), before);
    }

    #[test]
    fn real_parent_new_tag_is_explicit_trimmed_and_has_one_atomic_undo_step() {
        let mut h = fixture(app(), true);
        let original = h.state().app.session.active().unwrap().doc.clone();
        let field = node(&h, "TextField", "New tag");
        let before = (journal(&h), undo(&h));
        tab_to(&mut h, field.id);
        event(&mut h, Event::Text("  Fresh_Tag  ".into()));
        key(&mut h, Key::Enter);
        assert_eq!((journal(&h), undo(&h)), before, "typing/Enter is only a UI draft");
        key(&mut h, Key::Escape);
        let new = node(&h, "Button", "New");
        tab_to(&mut h, new.id);
        key(&mut h, Key::Space);
        last(&h, "xml.newTag", json!({"name":"Fresh_Tag"}));
        assert_eq!(undo(&h), before.1 + 1);
        assert!(names(&mut h.state_mut().app).contains(&"Fresh_Tag".into()));
        let committed = h.state().app.session.active().unwrap().doc.clone();
        let selection = h.state().app.session.active().unwrap().selection.clone();
        let unchanged = journal(&h);
        click(&mut h, "New");
        assert_eq!(journal(&h), unchanged, "success cleared the actual field draft; blank New does not dispatch");
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), original.as_ref());
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), committed.as_ref());
        assert_eq!(h.state().app.session.active().unwrap().selection, selection);
        no_replay(&mut h);
    }

    #[test]
    fn unbounded_plain_name_and_escape_keep_native_drafts_while_engine_refusal_remains_authoritative() {
        fn click(h: &mut Harness<'_, Window>, name: &str) {
            use egui_kittest::kittest::NodeT as _;
            let expected = node(h, "Button", name);
            let tree = h.root().accesskit_node().locate().1;
            let rect = {
                let button = h.get_by_role_and_label(egui::accesskit::Role::Button, name);
                let access = button.accesskit_node();
                assert_eq!(access.locate(), (expected.id.accesskit_id(), tree));
                assert_eq!(access.label().as_deref(), Some(name));
                assert!(!access.is_disabled());
                button.rect()
            };
            click_rect(h, rect);
        }
        let mut h = fixture(app(), true);
        let long = format!("Tag{}", "a".repeat(280));
        let field = node(&h, "TextField", "New tag");
        tab_to(&mut h, field.id);
        event(&mut h, Event::Text(long.clone()));
        key(&mut h, Key::Escape);
        click(&mut h, "New");
        assert!(names(&mut h.state_mut().app).contains(&long));
        let tag = node(&h, "Button", &apply_name(&long));
        let delete = node(&h, "Button", &format!("Delete <{long}>"));
        assert!(tag.rect.right() <= delete.rect.left(), "a real unlimited tag name must not cover its real Delete action");
        for n in [&tag, &delete] {
            assert!(h.state().parent.expand(1.0).contains_rect(n.rect));
        }

        let field = node(&h, "TextField", "New tag");
        tab_to(&mut h, field.id);
        event(&mut h, Event::Text("9 invalid".into()));
        key(&mut h, Key::Escape);
        let before = (journal(&h), undo(&h), h.state().app.session.active().unwrap().doc.clone());
        click(&mut h, "New");
        assert_eq!((journal(&h), undo(&h)), (before.0.clone(), before.1));
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before.2.as_ref());
        assert!(h.state().app.ui.status.contains("Tags:"));
        assert!(h.state().app.ui.status.contains("valid XML name"));
        click(&mut h, "New");
        assert_eq!((journal(&h), undo(&h)), (before.0, before.1), "a refused draft remains refused, not silently cleared or rewritten");
        no_replay(&mut h);
    }

    #[test]
    fn multi_object_tag_untag_and_definition_delete_retain_real_domain_and_atomic_undo() {
        let mut app = app();
        let (_, a) = frame(&mut app, "Alpha", 72.0);
        let (_, b) = frame(&mut app, "Beta", 200.0);
        app.run("xml.newTag", json!({"name":"Body"})).unwrap();
        app.run("selection.set", json!({"ids":[b.0,a.0]})).unwrap();
        let mut h = fixture(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo(&h);
        click(&mut h, &apply_name("Body"));
        last(&h, "xml.tag", json!({"tag":"Body"}));
        for id in [a, b] {
            assert_eq!(h.state().app.session.active().unwrap().doc.item(id).unwrap().xml_tag, "Body");
        }
        assert_eq!(undo(&h), history + 1);
        let after = h.state().app.session.active().unwrap().doc.clone();
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before.as_ref());
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), after.as_ref());
        idle(&mut h, 2);
        click(&mut h, "Untag Selection");
        last(&h, "xml.tag", json!({"tag":null}));
        for id in [a, b] {
            assert!(h.state().app.session.active().unwrap().doc.item(id).unwrap().xml_tag.is_empty());
        }
        click(&mut h, &apply_name("Body"));
        let before_delete = h.state().app.session.active().unwrap().doc.clone();
        let history = undo(&h);
        click(&mut h, "Delete <Body>");
        last(&h, "xml.deleteTag", json!({"name":"Body"}));
        assert!(!names(&mut h.state_mut().app).contains(&"Body".into()));
        for id in [a, b] {
            assert!(h.state().app.session.active().unwrap().doc.item(id).unwrap().xml_tag.is_empty());
        }
        assert_eq!(undo(&h), history + 1);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before_delete.as_ref());
        no_replay(&mut h);
    }

    #[test]
    fn selected_text_has_priority_over_its_selected_frame_and_untags_inline_without_retagging_the_frame() {
        let (mut app, sid, id) = body();
        app.run("xml.newTag", json!({"name":"Em"})).unwrap();
        app.run("text.select", json!({"story":sid.0,"anchor":4,"focus":7})).unwrap();
        let mut h = fixture(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let selection = h.state().app.session.active().unwrap().selection.clone();
        let history = undo(&h);
        click(&mut h, &apply_name("Em"));
        last(&h, "xml.tagText", json!({"tag":"Em"}));
        assert_eq!(undo(&h), history + 1, "nested type.char and the root tag command are one user undo step");
        assert!(journal(&h).iter().any(|(c, p)| c == "type.char" && p == &json!({"xmlTag":"Em"})));
        assert_eq!(h.state().app.session.active().unwrap().doc.item(id).unwrap().xml_tag, "Body");
        assert!(xml(&mut h.state_mut().app).contains("<Body>The <Em>cat</Em> sat</Body>"));
        let after = h.state().app.session.active().unwrap().doc.clone();
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before.as_ref());
        assert_eq!(h.state().app.session.active().unwrap().selection, selection);
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), after.as_ref());
        idle(&mut h, 2);
        click(&mut h, "Untag Selection");
        last(&h, "xml.tagText", json!({"tag":null}));
        assert!(!xml(&mut h.state_mut().app).contains("<Em>"));
        assert_eq!(h.state().app.session.active().unwrap().doc.item(id).unwrap().xml_tag, "Body");
        assert_eq!(text(&h.state().app, sid), "The cat sat");
        no_replay(&mut h);
    }

    #[test]
    fn structure_uses_actual_reading_order_full_object_identity_and_unelided_wrapped_navigation() {
        let mut app = app();
        let prefix = "word ".repeat(8);
        let (_, lower) = frame(&mut app, &format!("{prefix}lower-tail"), 240.0);
        let (_, upper) = frame(&mut app, &format!("{prefix}upper-tail"), 72.0);
        app.run("xml.tag", json!({"tag":"Body","ids":[lower.0,upper.0]})).unwrap();
        let info = app.session.execute("xml.structure", &json!({})).unwrap();
        assert_eq!(info["elements"][0]["id"], json!(upper.0));
        assert_eq!(info["elements"][1]["id"], json!(lower.0));
        let a = structure_name(&mut app, upper);
        let b = structure_name(&mut app, lower);
        assert_ne!(a, b);
        let mut h = fixture(app, true);
        let an = node(&h, "Link", &a);
        let bn = node(&h, "Link", &b);
        assert_ne!(an.id, bn.id);
        assert!(an.rect.top() < bn.rect.top());
        for n in [&an, &bn] {
            assert!(h.state().parent.expand(1.0).contains_rect(n.rect), "full genuine wrapped link fits the narrow real parent");
        }
        assert!(painted(&h).contains(&a));
        assert!(painted(&h).contains(&b));
        let before = undo(&h);
        tab_to(&mut h, an.id);
        key(&mut h, Key::Enter);
        last(&h, "selection.set", json!({"ids":[upper.0]}));
        assert_eq!(undo(&h), before);
        let bn = node(&h, "Link", &b);
        tab_to(&mut h, bn.id);
        key(&mut h, Key::Space);
        last(&h, "selection.set", json!({"ids":[lower.0]}));
        assert_eq!(undo(&h), before);
        no_replay(&mut h);
    }

    #[test]
    fn dtd_real_picker_validate_query_and_delete_keep_original_history_and_explicit_result_lifetime() {
        use crate::app::file_dialogs::testing::answer_with;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("valid.dtd");
        let dtd = "<!ELEMENT Root (ZBody)>\n<!ELEMENT ZBody (#PCDATA)>";
        std::fs::write(&path, dtd).unwrap();
        let (mut app, _, id) = body();
        app.run("xml.tag", json!({"tag":"ZBody", "ids":[id.0]})).unwrap();
        app.answer_dialogs(answer_with(Some(path.to_string_lossy().into_owned())));
        let mut h = fixture(app, true);
        let history = undo(&h);
        click(&mut h, "Load DTD…");
        assert!(h.state().app.file_dialogs_pending());
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 2);
        last(&h, "xml.loadDtd", json!({"path":path.to_string_lossy()}));
        assert_eq!(undo(&h), history + 1);
        assert_eq!(h.state().app.session.active().unwrap().doc.xml.dtd, dtd);
        let before = (journal(&h), undo(&h));
        click(&mut h, "Validate");
        assert_eq!((journal(&h), undo(&h)), before);
        assert!(painted(&h).contains(&"The structure is valid.".to_owned()));
        h.state_mut().app.run("xml.newTag", json!({"name":"Another"})).unwrap();
        idle(&mut h, 2);
        assert!(painted(&h).contains(&"The structure is valid.".to_owned()), "ordinary edits do not silently expire prior validation");
        let before_delete = h.state().app.session.active().unwrap().doc.clone();
        let history = undo(&h);
        click(&mut h, "Delete DTD");
        last(&h, "xml.deleteDtd", json!({}));
        assert_eq!(undo(&h), history + 1);
        assert!(h.state().app.session.active().unwrap().doc.xml.dtd.is_empty());
        assert!(!painted(&h).contains(&"The structure is valid.".to_owned()));
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before_delete.as_ref());
        no_replay(&mut h);
    }

    #[test]
    fn invalid_dtd_problem_paths_are_the_actual_query_results_and_survive_until_an_explicit_clear() {
        let (mut app, _, _) = body();
        app.run("xml.loadDtd", json!({"text":"<!ELEMENT Root (Missing)>\n<!ELEMENT Missing EMPTY>"})).unwrap();
        let actual = app.session.execute("xml.validate", &json!({})).unwrap();
        assert!(!actual["problems"].as_array().unwrap().is_empty());
        let mut h = fixture(app, true);
        let before = (journal(&h), undo(&h));
        click(&mut h, "Validate");
        assert_eq!((journal(&h), undo(&h)), before);
        let messages: Vec<_> = actual["problems"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| format!("⚠ {}: {}", p["path"].as_str().unwrap(), p["message"].as_str().unwrap()))
            .collect();
        for m in &messages {
            assert!(painted(&h).contains(m), "the real engine problem is rendered verbatim: {m}");
        }
        h.state_mut().app.run("xml.newTag", json!({"name":"Unrelated"})).unwrap();
        idle(&mut h, 2);
        for m in &messages {
            assert!(painted(&h).contains(m));
        }
        h.state_mut().app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(None));
        click(&mut h, "Load DTD…");
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 2);
        for m in &messages {
            assert!(!painted(&h).contains(m), "the original Load action clears results even when its real picker is cancelled");
        }
        no_replay(&mut h);
    }

    #[test]
    fn xml_import_and_export_use_real_picker_completions_real_files_and_the_existing_document_binding() {
        use crate::app::file_dialogs::testing::answer_with;
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.xml");
        std::fs::write(&input, "<Root><Body>Replacement &amp; preserved</Body></Root>").unwrap();
        let (mut app, sid, _) = body();
        let original = app.session.active().unwrap().doc.clone();
        let owner = app.session.active().unwrap().uid;
        app.answer_dialogs(answer_with(Some(input.to_string_lossy().into_owned())));
        let mut h = fixture(app, true);
        let history = undo(&h);
        click(&mut h, "Import XML…");
        assert!(h.state().app.file_dialogs_pending());
        h.state_mut().app.run("file.new", json!({"pages":1,"title":"Protected other document"})).unwrap();
        let protected_uid = h.state().app.session.active().unwrap().uid;
        let protected = h.state().app.session.active().unwrap().doc.clone();
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 2);
        assert_eq!(
            h.state().app.session.active().unwrap().uid,
            owner,
            "the existing real document-bound continuation activates its original document"
        );
        last(&h, "file.importXml", json!({"path":input.to_string_lossy()}));
        assert_eq!(text(&h.state().app, sid), "Replacement & preserved");
        assert_eq!(undo(&h), history + 1);
        let other = h.state().app.session.documents().iter().find(|d| d.uid == protected_uid).unwrap();
        assert_eq!(other.doc.as_ref(), protected.as_ref());
        let imported = h.state().app.session.active().unwrap().doc.clone();
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), original.as_ref());
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), imported.as_ref());
        idle(&mut h, 2);
        let expected = xml(&mut h.state_mut().app);
        let output = dir.path().join("output.xml");
        h.state_mut().app.answer_dialogs(answer_with(Some(output.to_string_lossy().into_owned())));
        let history = undo(&h);
        click(&mut h, "Export XML…");
        assert!(h.state().app.file_dialogs_pending());
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 2);
        assert_eq!(std::fs::read_to_string(&output).unwrap(), expected);
        assert_eq!(undo(&h), history);
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), imported.as_ref());
        no_replay(&mut h);
    }

    #[test]
    fn cancelled_failed_and_closed_document_dialog_answers_do_not_mutate_another_document() {
        use crate::app::file_dialogs::testing::answer_with;
        let (mut app, _, _) = body();
        app.answer_dialogs(answer_with(None));
        let mut h = fixture(app, true);
        for label in ["Load DTD…", "Import XML…", "Export XML…"] {
            let before = (journal(&h), undo(&h), h.state().app.session.active().unwrap().doc.clone());
            click(&mut h, label);
            h.state_mut().app.settle_dialogs();
            idle(&mut h, 2);
            assert_eq!(undo(&h), before.1);
            assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before.2.as_ref());
            assert_eq!(journal(&h), before.0, "cancel never executes a domain command");
        }
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("missing.dtd");
        h.state_mut().app.answer_dialogs(answer_with(Some(bad.to_string_lossy().into_owned())));
        let before = (journal(&h), undo(&h), h.state().app.session.active().unwrap().doc.clone());
        click(&mut h, "Load DTD…");
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 2);
        assert_eq!((journal(&h), undo(&h)), (before.0, before.1));
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before.2.as_ref());
        assert!(!h.state().app.ui.status.is_empty());
        let input = dir.path().join("valid.xml");
        std::fs::write(&input, "<Root><Body>must not arrive</Body></Root>").unwrap();
        h.state_mut().app.answer_dialogs(answer_with(Some(input.to_string_lossy().into_owned())));
        click(&mut h, "Import XML…");
        h.state_mut().app.run("file.close", json!({"force":true})).unwrap();
        h.state_mut().app.run("file.new", json!({"pages":1,"title":"Survivor"})).unwrap();
        let protected = h.state().app.session.active().unwrap().doc.clone();
        let before = (journal(&h), undo(&h));
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 2);
        assert_eq!((journal(&h), undo(&h)), before);
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), protected.as_ref());
        assert!(h.state().app.ui.status.contains("closed"));
        no_replay(&mut h);
    }

    #[test]
    fn held_tag_action_cannot_retarget_a_different_ordered_selection_and_fresh_native_input_recovers() {
        let (mut app, _, a) = body();
        let (_, b) = frame(&mut app, "Other", 240.0);
        app.run("selection.set", json!({"ids":[a.0]})).unwrap();
        let mut h = fixture(app, true);
        let held = node(&h, "Button", &apply_name("Body"));
        pointer(&mut h, held.rect, true);
        h.state_mut().app.run("selection.set", json!({"ids":[b.0,a.0]})).unwrap();
        idle(&mut h, 2);
        let fresh = node(&h, "Button", &apply_name("Body"));
        assert_ne!(held.id, fresh.id);
        let before = (journal(&h), undo(&h));
        pointer(&mut h, fresh.rect, false);
        assert_eq!((journal(&h), undo(&h)), before);
        assert!(h.state().app.session.active().unwrap().doc.item(b).unwrap().xml_tag.is_empty());
        click_rect(&mut h, fresh.rect);
        last(&h, "xml.tag", json!({"tag":"Body"}));
        for id in [a, b] {
            assert_eq!(h.state().app.session.active().unwrap().doc.item(id).unwrap().xml_tag, "Body");
        }
        no_replay(&mut h);
    }

    #[test]
    fn held_structure_action_binds_full_content_beyond_the_same_forty_scalar_preview() {
        let mut app = app();
        let prefix = "x".repeat(40);
        let (sid, id) = frame(&mut app, &format!("{prefix}old"), 72.0);
        app.run("xml.tag", json!({"tag":"Body","ids":[id.0]})).unwrap();
        app.run("selection.set", json!({"ids":[]})).unwrap();
        let name = structure_name(&mut app, id);
        let mut h = fixture(app, true);
        let held = node(&h, "Link", &name);
        pointer(&mut h, held.rect, true);
        h.state_mut().app.run("text.select", json!({"story":sid.0,"anchor":40,"focus":43})).unwrap();
        h.state_mut().app.run("text.insert", json!({"text":"new"})).unwrap();
        h.state_mut().app.run("selection.set", json!({"ids":[]})).unwrap();
        idle(&mut h, 2);
        assert_eq!(structure_name(&mut h.state_mut().app, id), name, "a preview is not a semantic identity");
        let fresh = node(&h, "Link", &name);
        assert_ne!(held.id, fresh.id);
        let before = (journal(&h), undo(&h));
        pointer(&mut h, fresh.rect, false);
        assert_eq!((journal(&h), undo(&h)), before);
        click_rect(&mut h, fresh.rect);
        last(&h, "selection.set", json!({"ids":[id.0]}));
        assert_eq!(undo(&h), before.1);
        no_replay(&mut h);
    }

    #[test]
    fn document_replacement_resets_only_the_current_draft_and_cancels_held_tag_creation() {
        fn click(h: &mut Harness<'_, Window>, name: &str) {
            use egui_kittest::kittest::NodeT as _;
            let expected = node(h, "Button", name);
            let tree = h.root().accesskit_node().locate().1;
            let rect = {
                let button = h.get_by_role_and_label(egui::accesskit::Role::Button, name);
                let access = button.accesskit_node();
                assert_eq!(access.locate(), (expected.id.accesskit_id(), tree));
                assert_eq!(access.label().as_deref(), Some(name));
                assert!(!access.is_disabled());
                button.rect()
            };
            click_rect(h, rect);
        }
        for same_uid in [false, true] {
            let mut h = fixture(app(), true);
            let f = node(&h, "TextField", "New tag");
            tab_to(&mut h, f.id);
            event(&mut h, Event::Text("OldDraft".into()));
            key(&mut h, Key::Escape);
            let held = node(&h, "Button", "New");
            pointer(&mut h, held.rect, true);
            let replacement = {
                let old = h.state().app.session.active().unwrap();
                let mut next = designcraft_engine::DocState::new((*old.doc).clone(), None);
                if same_uid {
                    next.uid = old.uid;
                    assert_ne!(next.identity_epoch, old.identity_epoch);
                }
                next
            };
            *h.state_mut().app.session.active_mut().unwrap() = replacement;
            h.state_mut().app.after_engine();
            idle(&mut h, 2);
            let fresh = node(&h, "Button", "New");
            assert_ne!(held.id, fresh.id);
            let before = (journal(&h), undo(&h));
            pointer(&mut h, fresh.rect, false);
            assert_eq!((journal(&h), undo(&h)), before);
            click(&mut h, "New");
            assert_eq!((journal(&h), undo(&h)), before, "the previous document draft never leaks");
            let f = node(&h, "TextField", "New tag");
            tab_to(&mut h, f.id);
            event(&mut h, Event::Text("FreshDraft".into()));
            key(&mut h, Key::Escape);
            click(&mut h, "New");
            assert!(names(&mut h.state_mut().app).contains(&"FreshDraft".into()));
            assert!(!names(&mut h.state_mut().app).contains(&"OldDraft".into()));
            no_replay(&mut h);
        }
    }

    #[test]
    fn narrow_parent_disabled_sdk_measurement_empty_document_and_genuine_native_fallback_remain_honest() {
        let (app, _, _) = body();
        let mut h = fixture(app, true);
        let before = (journal(&h), undo(&h), h.state().app.session.active().unwrap().doc.clone());
        let delete_rect = h.get_by_label("Delete <Body>").rect();
        let markers_rect = h.get_by_label("Show Tag Markers").rect();
        let load_rect = h.get_by_label("Load DTD…").rect();
        h.state_mut().enabled = false;
        idle(&mut h, 2);
        click_rect(&mut h, delete_rect);
        click_rect(&mut h, markers_rect);
        h.state_mut().enabled = true;
        h.state_mut().measuring = true;
        idle(&mut h, 2);
        click_rect(&mut h, delete_rect);
        click_rect(&mut h, load_rect);
        assert_eq!((journal(&h), undo(&h)), (before.0, before.1));
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before.2.as_ref());
        assert!(!h.state().app.file_dialogs_pending());
        h.state_mut().measuring = false;
        idle(&mut h, 2);
        let old = h.state().app.ui.tag_markers;
        let history = undo(&h);
        click(&mut h, "Show Tag Markers");
        assert_ne!(h.state().app.ui.tag_markers, old);
        assert_eq!(undo(&h), history);
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before.2.as_ref());
        for e in h.state().controls.as_ref().unwrap().elements.iter().filter(|e| e.part.is_none()) {
            assert!(h.state().parent.expand(1.0).contains_rect(e.rect), "genuine {} must fit the actual narrow parent", e.component);
        }
        h.state_mut().app.run("file.close", json!({"force":true})).unwrap();
        idle(&mut h, 2);
        assert!(h.state().controls.as_ref().is_none_or(|c| c.elements.is_empty()));
        let (app, _, native_id) = body();
        let mut native = fixture(app, false);
        assert!(native.state().controls.as_ref().is_none_or(|c| c.elements.is_empty()));
        click(&mut native, &apply_name("Body"));
        last(&native, "xml.tag", json!({"tag":"Body"}));
        let label = structure_name(&mut native.state_mut().app, native_id);
        click(&mut native, &label);
        last(&native, "selection.set", json!({"ids":[native_id.0]}));
        no_replay(&mut native);
    }
}
