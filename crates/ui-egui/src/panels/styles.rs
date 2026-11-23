//! Paragraph Styles, Character Styles and Object Styles panels.

use egui::{Sense, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{DesignApp, icons};

fn list(app: &mut DesignApp, ui: &mut egui::Ui, para: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::list(app, ui, para);
        return;
    }
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let names: Vec<String> = if para {
        st.doc.styles.paragraph.iter().map(|s| s.name.clone()).collect()
    } else {
        st.doc.styles.character.iter().map(|s| s.name.clone()).collect()
    };
    let attrs = super::text_attrs(app);
    let current = attrs.as_ref().and_then(|a| a[if para { "paragraphStyle" } else { "characterStyle" }].as_str().map(str::to_string));
    let overrides = attrs
        .as_ref()
        .map(|a| {
            if para {
                a["charOverrides"].as_u64().unwrap_or(0) + a["paraOverrides"].as_u64().unwrap_or(0)
            } else {
                a["characterStyleOverrides"].as_u64().unwrap_or(0)
            }
        })
        .unwrap_or(0);
    crate::rtl::label(
        ui,
        egui::RichText::new(match &current {
            Some(c) => format!("{}{}", crate::i18n::style_name(&app.ui.language, c), if overrides > 0 { "+" } else { "" }),
            None => crate::i18n::tr(&app.ui.language, "No text selected").into(),
        })
        .size(11.0)
        .color(t.text_dim),
    );
    // Groups (`Group/Name`) as folders, ungrouped styles first.
    let groups: Vec<String> = {
        let mut g: Vec<String> = names.iter().filter_map(|n| n.rsplit_once('/').map(|(g, _)| g.to_string())).collect();
        g.sort();
        g.dedup();
        g
    };
    for n in names.iter().filter(|n| !n.contains('/')) {
        if para && n == designcraft_doc::NO_PARA_STYLE {
            continue;
        }
        style_row(app, ui, para, n, (current.as_deref(), overrides > 0), &groups, &t);
    }
    for g in &groups {
        egui::CollapsingHeader::new(g.as_str()).id_salt(("style_group", para, g)).default_open(true).show(ui, |ui| {
            for n in names.iter().filter(|n| n.rsplit_once('/').is_some_and(|(x, _)| x == g)) {
                style_row(app, ui, para, n, (current.as_deref(), overrides > 0), &groups, &t);
            }
        });
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(
                &app.ui.language,
                if cfg!(target_os = "macos") { "⌥-click clears overrides" } else { "Alt-click clears overrides" },
            ))
            .size(10.5)
            .color(t.text_disabled),
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if text_style_create_button(app, ui, para) {
                let cmd = if para { "style.paragraph.create" } else { "style.character.create" };
                let base = if para { "Paragraph Style 1" } else { "Character Style 1" };
                let _ = app.run(cmd, json!({"name": base, "fromSelection": true}));
            }
            crate::menus::menu_button(ui, "☰", |ui| {
                let (label, cmd) = if para {
                    ("Load Paragraph Styles…", "app.loadParagraphStyles")
                } else {
                    ("Load Character Styles…", "app.loadCharacterStyles")
                };
                for (label, cmd) in [(label, cmd), ("Load All Text Styles…", "app.loadTextStyles")] {
                    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                        let _ = app.run(cmd, json!({}));
                        ui.close();
                    }
                }
            });
        });
    });
}

pub fn paragraph(app: &mut DesignApp, ui: &mut egui::Ui) {
    list(app, ui, true);
}

pub fn character(app: &mut DesignApp, ui: &mut egui::Ui) {
    list(app, ui, false);
}

/// The compact text-style create action of the native list; commands and list/row behavior stay with the app.
fn text_style_create_button(app: &DesignApp, ui: &mut egui::Ui, para: bool) -> bool {
    let label = crate::i18n::tr(&app.ui.language, crate::i18n::tr(&app.ui.language, "Create New Style from Selection"));
    let _ = para;
    icons::button(ui, "plus", 20.0, false, label).clicked()
}

/// The panels on the framework: the same rows, menus, commands and Undo steps as the native lists, drawn with its
/// components. Each pass collects what the user asked for and runs it after the components are shown.
#[cfg(not(target_arch = "wasm32"))]
mod framework {
    use craft_ui_panels::sections::Section;
    use craft_ui_widgets::buttons::{Button, IconButton};
    use craft_ui_widgets::contract::{Bind, PopupKind, Props as _, Show as _, ShowContainer as _, Sizing, Trigger};
    use craft_ui_widgets::display::{Divider, Picture, Shade, Text};
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::list::ThumbAspect;
    use craft_ui_widgets::menu::{Menu, MenuItem};
    use craft_ui_widgets::primitives::{HAlign, IconSpec};
    use craft_ui_widgets::style::{TypeRole, Variant};
    use craft_ui_widgets::text::TextField;
    use serde_json::{Value, json};

    use super::{object_style_of_selection, selection_overrides_styles};
    use crate::DesignApp;
    use crate::panels::tabs::TabGlyph;
    use crate::theme::Tokens;

    /// What a pass of a panel asks for.
    enum Act {
        /// A command.
        Run(&'static str, Value),
        /// The options dialog of a text style (paragraph when true).
        Options(bool, String),
        /// The options dialog of an object style.
        ObjectOptions(String),
        /// The New Object Style dialog.
        ObjectNew,
        /// A dialog that asks before deleting the named style.
        Delete(&'static str, String),
    }

    fn finish(app: &mut DesignApp, acts: Vec<Act>) {
        for act in acts {
            match act {
                Act::Run(id, params) => {
                    let _ = app.run(id, params);
                }
                Act::Options(para, name) => {
                    crate::dialogs::open_style_options(app, para, &name);
                }
                Act::ObjectOptions(name) => {
                    let _ = crate::object_style_dialog::open_edit(app, &name);
                }
                Act::ObjectNew => {
                    crate::object_style_dialog::open_new(app);
                }
                Act::Delete(dialog, name) => app.ui.dialog = Some(crate::dialogs::Dialog::new(dialog, json!({"name": name}))),
            }
        }
    }

    /// A row of a style menu; whether it ran.
    fn item(ui: &mut egui::Ui, salt: &str, label: String, enabled: bool) -> bool {
        MenuItem::new(label).id_salt(salt.to_owned()).enabled(enabled).show(ui).ran
    }

    /// A button at a panel's foot with the app's glyph; one that opens a menu says so.
    fn glyph_button(
        ui: &mut egui::Ui,
        salt: &str,
        glyph: &'static str,
        label: &str,
        enabled: bool,
        menu: bool,
    ) -> craft_ui_widgets::buttons::ButtonOutput {
        let face = TabGlyph(glyph);
        let button =
            IconButton::new(IconSpec::glyph(&face)).id_salt(salt.to_owned()).label(label.to_owned()).tooltip(label.to_owned()).enabled(enabled);
        if menu { button.popup(PopupKind::Menu).show(ui) } else { button.show(ui) }
    }

    pub(super) fn list(app: &mut DesignApp, ui: &mut egui::Ui, para: bool) {
        let Some(st) = app.session.active() else { return };
        let identity = (st.uid, st.identity_epoch);
        let names: Vec<String> = if para {
            st.doc.styles.paragraph.iter().map(|s| s.name.clone()).collect()
        } else {
            st.doc.styles.character.iter().map(|s| s.name.clone()).collect()
        };
        let attrs = crate::panels::text_attrs(app);
        let current = attrs.as_ref().and_then(|a| a[if para { "paragraphStyle" } else { "characterStyle" }].as_str().map(str::to_string));
        let overrides = attrs
            .as_ref()
            .map(|a| {
                if para {
                    a["charOverrides"].as_u64().unwrap_or(0) + a["paraOverrides"].as_u64().unwrap_or(0)
                } else {
                    a["characterStyleOverrides"].as_u64().unwrap_or(0)
                }
            })
            .unwrap_or(0);
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
        let summary = match &current {
            Some(c) => format!("{}{}", crate::i18n::style_name(&lang, c), if overrides > 0 { "+" } else { "" }),
            None => tr("No text selected"),
        };
        // Groups (`Group/Name`) as folders, ungrouped styles first.
        let groups: Vec<String> = {
            let mut g: Vec<String> = names.iter().filter_map(|n| n.rsplit_once('/').map(|(g, _)| g.to_string())).collect();
            g.sort();
            g.dedup();
            g
        };
        let state = (current.as_deref(), overrides > 0);
        let mut acts: Vec<Act> = Vec::new();
        let mut create = false;
        ui.push_id(("framework-styles", para, identity), |ui| {
            Stack::new().id_salt("styles").fill_width().show(ui, |ui, _| {
                Text::new(summary.clone()).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
                for n in names.iter().filter(|n| !n.contains('/')) {
                    if para && n == designcraft_doc::NO_PARA_STYLE {
                        continue;
                    }
                    text_row(&lang, ui, para, n, state, &groups, &mut acts);
                }
                for g in &groups {
                    Section::new(g.clone()).id_salt(("style_group", para, g)).show(ui, |ui, _| {
                        for n in names.iter().filter(|n| n.rsplit_once('/').is_some_and(|(x, _)| x == g)) {
                            text_row(&lang, ui, para, n, state, &groups, &mut acts);
                        }
                    });
                }
                Row::new().id_salt("styles_footer").fill_width().show(ui, |ui, _| {
                    let hint = if cfg!(target_os = "macos") { "⌥-click clears overrides" } else { "Alt-click clears overrides" };
                    Text::new(tr(hint)).role(TypeRole::Caption).shade(Shade::Faint).sizing(Sizing::FillWidth).show(ui);
                    let (load, command) = if para {
                        ("Load Paragraph Styles…", "app.loadParagraphStyles")
                    } else {
                        ("Load Character Styles…", "app.loadCharacterStyles")
                    };
                    let opener = glyph_button(ui, "styles_menu_opener", "menu", &tr("Load Styles"), true, true);
                    let _ =
                        Menu::new(tr("Load Styles")).id_salt(("styles_menu", para)).opened_by(&opener.response, Trigger::Click).show(ui, |ui, _| {
                            for (label, id) in [(load, command), ("Load All Text Styles…", "app.loadTextStyles")] {
                                if item(ui, id, tr(label), true) {
                                    acts.push(Act::Run(id, json!({})));
                                }
                            }
                        });
                    create |= create_button(app_label(&lang), ui, para, identity);
                });
            });
        });
        if create {
            let cmd = if para { "style.paragraph.create" } else { "style.character.create" };
            let base = if para { "Paragraph Style 1" } else { "Character Style 1" };
            acts.push(Act::Run(cmd, json!({"name": base, "fromSelection": true})));
        }
        finish(app, acts);
    }

    fn app_label(lang: &str) -> String {
        crate::i18n::tr(lang, crate::i18n::tr(lang, "Create New Style from Selection")).to_string()
    }

    /// The compact create action; its label is also its accessible name.
    fn create_button(label: String, ui: &mut egui::Ui, para: bool, identity: (u64, u64)) -> bool {
        Button::new(label.clone())
            .id_salt(("text-style-create", para, Some(identity)))
            .painted("+")
            .variant(Variant::GHOST)
            .tooltip(label)
            .show(ui)
            .clicked()
    }

    /// One text style: a click applies it (⌥ clears overrides), a double click edits it, and the context menu holds
    /// edit, apply, redefine, break link, move to group and delete.
    fn text_row(
        lang: &str,
        ui: &mut egui::Ui,
        para: bool,
        n: &str,
        (current, overridden): (Option<&str>, bool),
        groups: &[String],
        acts: &mut Vec<Act>,
    ) {
        let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
        let shown = n.rsplit('/').next().unwrap_or(n);
        let label = if n.contains('/') { shown } else { crate::i18n::style_name(lang, shown) };
        let is_current = current == Some(n);
        let row_label = format!("{label}{}", if is_current && overridden { "+" } else { "" });
        let editable = n != if para { designcraft_doc::NO_PARA_STYLE } else { designcraft_doc::NO_CHAR_STYLE };
        let apply = if para { "style.paragraph.apply" } else { "style.character.apply" };
        let kind = if para { "paragraph" } else { "character" };
        let out = Button::new(row_label.clone())
            .id_salt(("style_row", para, n.to_owned()))
            .variant(if is_current { Variant::SECONDARY } else { Variant::GHOST })
            .align(HAlign::Start)
            .sizing(Sizing::FillWidth)
            .show(ui);
        if out.clicks >= 2 && editable {
            acts.push(Act::Options(para, n.to_owned()));
        } else if out.clicked() {
            acts.push(Act::Run(apply, json!({"name": n, "clearOverrides": out.mods.alt})));
        }
        let _ = Menu::context(row_label, &out.response).id_salt(("style_menu", para, n.to_owned())).show(ui, |ui, _| {
            if editable && item(ui, "edit", tr("Edit \"{name}\"…").replace("{name}", label), true) {
                acts.push(Act::Options(para, n.to_owned()));
            }
            if item(ui, "apply", format!("{} \"{}\"", tr("Apply"), label), true) {
                acts.push(Act::Run(apply, json!({"name": n})));
            }
            if editable && is_current && item(ui, "redefine", tr("Redefine Style"), true) {
                let command = if para { "style.paragraph.redefine" } else { "style.character.redefine" };
                acts.push(Act::Run(command, json!({"name": n})));
            }
            if para && item(ui, "clear", tr("Apply \"{name}\", Clear Overrides").replace("{name}", label), true) {
                acts.push(Act::Run("style.paragraph.apply", json!({"name": n, "clearOverrides": true})));
            }
            if is_current && item(ui, "break", tr("Break Link to Style"), true) {
                acts.push(Act::Run("style.breakLink", json!({"kind": kind})));
            }
            if !n.starts_with('[') {
                let _ = Menu::submenu_row(ui, tr("Move to Group"), |ui, _| {
                    if n.contains('/') && item(ui, "no_group", tr("[No Group]"), true) {
                        acts.push(Act::Run("style.group", json!({"kind": kind, "names": [n], "group": ""})));
                    }
                    for g in groups {
                        if n.rsplit_once('/').map(|(x, _)| x) != Some(g.as_str()) && item(ui, &format!("group:{g}"), g.clone(), true) {
                            acts.push(Act::Run("style.group", json!({"kind": kind, "names": [n], "group": g})));
                        }
                    }
                    Divider::horizontal().show(ui);
                    let id = ui.make_persistent_id(("new_style_group", para));
                    let mut name: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
                    let hint = tr("New group…");
                    let out = TextField::new(hint.clone(), Bind::bound(&mut name))
                        .id_salt(("new_style_group", para))
                        .placeholder(hint)
                        .limit(None)
                        .submits()
                        .show(ui);
                    if out.events.committed().is_some() && !name.trim().is_empty() {
                        acts.push(Act::Run("style.group", json!({"kind": kind, "names": [n], "group": name.trim()})));
                        name.clear();
                    }
                    ui.data_mut(|d| d.insert_temp(id, name));
                });
                Divider::horizontal().show(ui);
                let (dialog, label) =
                    if para { ("deleteParagraphStyle", "Delete Paragraph Style…") } else { ("deleteCharacterStyle", "Delete Character Style…") };
                if item(ui, "delete", tr(label), true) {
                    acts.push(Act::Delete(dialog, n.to_owned()));
                }
            }
        });
    }

    pub(super) fn object(app: &mut DesignApp, ui: &mut egui::Ui) {
        let t = Tokens::get(ui.ctx());
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
        let Some(st) = app.session.active() else { return };
        let identity = (st.uid, st.identity_epoch);
        let names: Vec<String> = st.doc.styles.object.iter().map(|s| s.name.clone()).collect();
        let (default_graphic, default_text) = (st.doc.styles.default_graphic_frame.clone(), st.doc.styles.default_text_frame.clone());
        let current = object_style_of_selection(app);
        let selected = current.is_some();
        let over = selection_overrides_styles(app);
        let shown = |n: &str, over: bool| format!("{}{}", crate::i18n::style_name(&lang, n), if over { "+" } else { "" });
        let summary = current.as_ref().map_or_else(String::new, |(n, over)| shown(n, *over));
        let icon = t.icon;
        let mut acts: Vec<Act> = Vec::new();
        let (mut create, mut clear) = (None, false);
        ui.push_id(("framework-object-styles", identity), |ui| {
            Stack::new().id_salt("object_styles").fill_width().show(ui, |ui, _| {
                if !summary.is_empty() {
                    Text::new(summary.clone()).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
                }
                for n in &names {
                    let is_current = current.as_ref().is_some_and(|(c, _)| c == n);
                    let label = shown(n, is_current && current.as_ref().is_some_and(|c| c.1));
                    let editable = n != designcraft_doc::NO_OBJECT_STYLE;
                    let (graphic, text) = (*n == default_graphic, *n == default_text);
                    Row::new().id_salt(("object_style_row", n.clone())).fill_width().show(ui, |ui, _| {
                        let out = Button::new(label.clone())
                            .id_salt(("object_style", n.clone()))
                            .variant(if is_current { Variant::SECONDARY } else { Variant::GHOST })
                            .align(HAlign::Start)
                            .sizing(Sizing::FillWidth)
                            .show(ui);
                        // The default styles of new graphic and text frames, as InDesign marks them.
                        Picture::painter("", ThumbAspect::SQUARE, move |cx, rect| {
                            if graphic || text {
                                let mark = egui::Rect::from_center_size(rect.center(), egui::vec2(11.0, 11.0));
                                cx.painter.rect_stroke(mark, 0.0, egui::Stroke::new(1.0, icon), egui::StrokeKind::Inside);
                                if text {
                                    cx.painter.text(mark.center(), egui::Align2::CENTER_CENTER, "T", egui::FontId::proportional(8.5), icon);
                                }
                            }
                        })
                        .id_salt(("object_style_mark", n.clone()))
                        .show(ui);
                        if out.clicks >= 2 && editable {
                            acts.push(Act::ObjectOptions(n.clone()));
                        } else if out.clicked() && selected {
                            acts.push(Act::Run("style.object.apply", json!({"name": n})));
                        }
                        let name = crate::i18n::style_name(&lang, n);
                        let _ = Menu::context(label.clone(), &out.response).id_salt(("object_style_menu", n.clone())).show(ui, |ui, _| {
                            if editable && item(ui, "edit", tr("Edit \"{name}\"…").replace("{name}", name), true) {
                                acts.push(Act::ObjectOptions(n.clone()));
                            }
                            if item(ui, "apply", format!("{} \"{}\"", tr("Apply"), name), selected) {
                                acts.push(Act::Run("style.object.apply", json!({"name": n})));
                            }
                        });
                    });
                }
                Row::new().id_salt("object_styles_footer").fill_width().show(ui, |ui, _| {
                    let opener = glyph_button(ui, "object_styles_menu_opener", "menu", &tr("Object Styles"), true, true);
                    let _ =
                        Menu::new(tr("Object Styles")).id_salt("object_styles_menu").opened_by(&opener.response, Trigger::Click).show(ui, |ui, _| {
                            if item(ui, "new", tr("New Object Style…"), true) {
                                acts.push(Act::ObjectNew);
                            }
                            if let Some((n, _)) = current.as_ref().filter(|(n, _)| n != designcraft_doc::NO_OBJECT_STYLE)
                                && item(ui, "options", tr("Object Style Options…"), true)
                            {
                                acts.push(Act::ObjectOptions(n.clone()));
                            }
                        });
                    clear |= glyph_button(ui, "object_styles_clear", "style-clear", &tr("Clear Overrides"), over, false).clicked() && over;
                    let new = glyph_button(ui, "object_styles_new", "style-new", &tr("Create New Style"), true, false);
                    if new.clicked() {
                        create = Some(new.mods.alt);
                    }
                });
            });
        });
        if clear {
            acts.push(Act::Run("style.object.clearOverrides", json!({})));
        }
        // Alt-click, or nothing selected: the New Object Style dialog; otherwise the selection's style is captured.
        match create {
            Some(alt) if alt || !selected => acts.push(Act::ObjectNew),
            Some(_) => acts.push(Act::Run("style.object.create", json!({"name": "Object Style 1", "fromSelection": true}))),
            None => {}
        }
        finish(app, acts);
    }
}

/// The selected object's style, and whether the object differs from it (an override: `+`): its
/// own attributes, or a text frame's paragraphs in other styles than the object style gives them.
fn object_style_of_selection(app: &DesignApp) -> Option<(String, bool)> {
    let st = app.session.active()?;
    let it = st.selection.items.first().and_then(|i| st.doc.item(*i))?;
    Some((it.object_style.clone(), overrides_its_style(&st.doc, it)))
}

/// Whether `it` differs from its object style: its own attributes, or a text frame's paragraphs
/// in other styles than the object style gives them.
fn overrides_its_style(doc: &designcraft_doc::Document, it: &designcraft_doc::Item) -> bool {
    let Some(os) = doc.styles.object_style(&it.object_style) else { return false };
    let mut styled = it.clone();
    os.apply_to(&mut styled);
    styled != *it || it.text_frame().is_some_and(|tf| doc.story(tf.story).is_some_and(|s| !os.story_follows(&doc.styles, s)))
}

/// Whether any selected object differs from its object style (Clear Overrides clears them all).
fn selection_overrides_styles(app: &DesignApp) -> bool {
    app.session.active().is_some_and(|st| st.selection.items.iter().filter_map(|i| st.doc.item(*i)).any(|it| overrides_its_style(&st.doc, it)))
}

/// Window › Styles › Object Styles, as InDesign's: the selected object's style on top (`+` when
/// it is overridden), the document's object styles (a click applies one to the selection, a
/// double-click opens its options; the default graphic and text frame styles are marked), and at
/// the bottom Clear Overrides and Create New Style (⌥/Alt-click: the New Object Style dialog).
pub fn object(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::object(app, ui);
        return;
    }
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let Some(st) = app.session.active() else { return };
    let names: Vec<String> = st.doc.styles.object.iter().map(|s| s.name.clone()).collect();
    let (default_graphic, default_text) = (st.doc.styles.default_graphic_frame.clone(), st.doc.styles.default_text_frame.clone());
    let current = object_style_of_selection(app);
    let selected = current.is_some();
    let shown = |n: &str, over: bool| format!("{}{}", crate::i18n::style_name(&lang, n), if over { "+" } else { "" });
    crate::rtl::label(ui, egui::RichText::new(current.as_ref().map_or_else(String::new, |(n, over)| shown(n, *over))).size(11.0).color(t.text_dim));
    for n in &names {
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
        let is_current = current.as_ref().is_some_and(|(c, _)| c == n);
        if is_current {
            ui.painter().rect_filled(row, 0.0, t.row_selected);
        } else if resp.hovered() {
            ui.painter().rect_filled(row, 0.0, t.hover);
        }
        let label = shown(n, is_current && current.as_ref().is_some_and(|c| c.1));
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, is_current, &label));
        crate::rtl::paint(ui.painter(), row.min + vec2(24.0, 11.0), egui::Align2::LEFT_CENTER, &label, egui::FontId::proportional(12.5), t.text);
        // The default styles of new graphic and text frames, as InDesign marks them.
        let mark = egui::Rect::from_center_size(egui::pos2(row.max.x - 14.0, row.center().y), vec2(11.0, 11.0));
        if *n == default_graphic {
            ui.painter().rect_stroke(mark, 0.0, egui::Stroke::new(1.0, t.icon), egui::StrokeKind::Inside);
        }
        if *n == default_text {
            ui.painter().rect_stroke(mark, 0.0, egui::Stroke::new(1.0, t.icon), egui::StrokeKind::Inside);
            ui.painter().text(mark.center(), egui::Align2::CENTER_CENTER, "T", egui::FontId::proportional(8.5), t.icon);
        }
        let editable = n != designcraft_doc::NO_OBJECT_STYLE;
        if resp.double_clicked() && editable {
            let _ = crate::object_style_dialog::open_edit(app, n);
        } else if resp.clicked() && selected {
            let _ = app.run("style.object.apply", json!({"name": n}));
        }
        resp.context_menu(|ui| {
            let label = crate::i18n::style_name(&lang, n);
            if editable && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Edit \"{name}\"…").replace("{name}", label))).clicked() {
                let _ = crate::object_style_dialog::open_edit(app, n);
                ui.close();
            }
            if ui
                .add_enabled(selected, egui::Button::new(crate::rtl::widget(ui, format!("{} \"{}\"", crate::i18n::tr(&lang, "Apply"), label))))
                .clicked()
            {
                let _ = app.run("style.object.apply", json!({"name": n}));
                ui.close();
            }
        });
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, "style-new", 20.0, false, crate::i18n::tr(&lang, "Create New Style")).clicked() {
                if ui.input(|i| i.modifiers.alt) {
                    crate::object_style_dialog::open_new(app);
                } else if selected {
                    let _ = app.run("style.object.create", json!({"name": "Object Style 1", "fromSelection": true}));
                } else {
                    crate::object_style_dialog::open_new(app);
                }
            }
            let over = selection_overrides_styles(app);
            if ui.add_enabled_ui(over, |ui| icons::button(ui, "style-clear", 20.0, false, crate::i18n::tr(&lang, "Clear Overrides"))).inner.clicked()
                && over
            {
                let _ = app.run("style.object.clearOverrides", json!({}));
            }
            crate::menus::menu_button(ui, "☰", |ui| {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "New Object Style…"))).clicked() {
                    crate::object_style_dialog::open_new(app);
                    ui.close();
                }
                if let Some((n, _)) = current.as_ref().filter(|(n, _)| n != designcraft_doc::NO_OBJECT_STYLE)
                    && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Object Style Options…"))).clicked()
                {
                    let _ = crate::object_style_dialog::open_edit(app, n);
                    ui.close();
                }
            });
        });
    });
}

/// One style in the list: click applies (⌥ clears overrides), double-click edits,
/// right-click: edit, apply, break link, move to group.
fn style_row(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    para: bool,
    n: &str,
    (current, overridden): (Option<&str>, bool),
    groups: &[String],
    t: &Tokens,
) {
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    if current == Some(n) {
        ui.painter().rect_filled(row, 0.0, t.row_selected);
    } else if resp.hovered() {
        ui.painter().rect_filled(row, 0.0, t.hover);
    }
    let shown = n.rsplit('/').next().unwrap_or(n);
    let label = if n.contains('/') { shown } else { crate::i18n::style_name(&app.ui.language, shown) };
    let row_label = format!("{label}{}", if current == Some(n) && overridden { "+" } else { "" });
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, current == Some(n), &row_label));
    crate::rtl::paint(ui.painter(), row.min + vec2(8.0, 11.0), egui::Align2::LEFT_CENTER, &row_label, egui::FontId::proportional(12.5), t.text);
    let editable = n != if para { designcraft_doc::NO_PARA_STYLE } else { designcraft_doc::NO_CHAR_STYLE };
    if resp.double_clicked() && editable {
        crate::dialogs::open_style_options(app, para, n);
    } else if resp.clicked() {
        let cmd = if para { "style.paragraph.apply" } else { "style.character.apply" };
        let clear = ui.input(|i| i.modifiers.alt);
        let _ = app.run(cmd, json!({"name": n, "clearOverrides": clear}));
    }
    let kind = if para { "paragraph" } else { "character" };
    resp.context_menu(|ui| {
        let label = if n.contains('/') { shown } else { crate::i18n::style_name(&app.ui.language, shown) };
        if editable && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Edit \"{name}\"…").replace("{name}", label))).clicked() {
            crate::dialogs::open_style_options(app, para, n);
            ui.close();
        }
        if ui.button(crate::rtl::widget(ui, format!("{} \"{}\"", crate::i18n::tr(&app.ui.language, "Apply"), label))).clicked() {
            let cmd = if para { "style.paragraph.apply" } else { "style.character.apply" };
            let _ = app.run(cmd, json!({"name": n}));
            ui.close();
        }
        if editable && current == Some(n) && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Redefine Style"))).clicked() {
            let command = if para { "style.paragraph.redefine" } else { "style.character.redefine" };
            let _ = app.run(command, json!({"name":n}));
            ui.close();
        }
        if para
            && ui
                .button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Apply \"{name}\", Clear Overrides").replace("{name}", label)))
                .clicked()
        {
            let _ = app.run("style.paragraph.apply", json!({"name": n, "clearOverrides": true}));
            ui.close();
        }
        if current == Some(n) && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Break Link to Style"))).clicked() {
            let _ = app.run("style.breakLink", json!({"kind": kind}));
            ui.close();
        }
        if !n.starts_with('[') {
            crate::menus::menu_button(ui, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Move to Group")), |ui| {
                if n.contains('/') && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "[No Group]"))).clicked() {
                    let _ = app.run("style.group", json!({"kind": kind, "names": [n], "group": ""}));
                    ui.close();
                }
                for g in groups {
                    if n.rsplit_once('/').map(|(x, _)| x) != Some(g.as_str()) && ui.button(g).clicked() {
                        let _ = app.run("style.group", json!({"kind": kind, "names": [n], "group": g}));
                        ui.close();
                    }
                }
                ui.separator();
                let id = egui::Id::unique(("new_style_group", para));
                let mut name: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
                let r = ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New group…")))
                        .desired_width(140.0),
                );
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !name.trim().is_empty() {
                    let _ = app.run("style.group", json!({"kind": kind, "names": [n], "group": name.trim()}));
                    name.clear();
                    ui.close();
                }
                ui.data_mut(|d| d.insert_temp(id, name));
            });
        }
        if para && !n.starts_with('[') {
            ui.separator();
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Paragraph Style…"))).clicked() {
                app.ui.dialog = Some(crate::dialogs::Dialog::new("deleteParagraphStyle", json!({"name": n})));
                ui.close();
            }
        }
        if !para && !n.starts_with('[') {
            ui.separator();
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Character Style…"))).clicked() {
                app.ui.dialog = Some(crate::dialogs::Dialog::new("deleteCharacterStyle", json!({"name": n})));
                ui.close();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use egui_kittest::{Harness, kittest::Queryable};

    use super::*;

    /// A text frame whose paragraphs left its object style's paragraph style shows the override
    /// mark, and Clear Overrides puts them back.
    #[test]
    fn a_paragraph_style_change_is_an_object_style_override() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", serde_json::json!({})).unwrap();
        app.run("style.paragraph.create", serde_json::json!({"name": "Caption"})).unwrap();
        app.run("style.object.create", serde_json::json!({"name": "Captioned", "fromSelection": false, "paragraphStyle": "Caption"})).unwrap();
        let f = app.run("frame.create", serde_json::json!({"rect": [36, 36, 300, 200], "content": "text", "text": "One"})).unwrap();
        app.run("style.object.apply", serde_json::json!({"name": "Captioned", "ids": [f["id"]]})).unwrap();
        app.run("selection.set", serde_json::json!({"ids": [f["id"]]})).unwrap();
        assert_eq!(object_style_of_selection(&app), Some(("Captioned".into(), false)));
        app.run("text.select", serde_json::json!({"story": f["story"], "anchor": 0, "focus": 3})).unwrap();
        app.run("style.paragraph.apply", serde_json::json!({"name": designcraft_doc::BASIC_PARAGRAPH})).unwrap();
        app.run("selection.set", serde_json::json!({"ids": [f["id"]]})).unwrap();
        assert_eq!(object_style_of_selection(&app), Some(("Captioned".into(), true)), "the + mark");
        app.run("style.object.clearOverrides", serde_json::json!({})).unwrap();
        assert_eq!(object_style_of_selection(&app), Some(("Captioned".into(), false)));
    }

    #[test]
    fn selected_style_context_menu_redefines_its_format() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let frame = app.run("frame.create", json!({"rect":[36,36,300,200],"content":"text","text":"Headline"})).unwrap();
        app.run("text.select", json!({"story":frame["story"],"anchor":0,"focus":8})).unwrap();
        app.run("style.paragraph.create", json!({"name":"Header","chars":{"size":18}})).unwrap();
        app.run("style.paragraph.apply", json!({"name":"Header"})).unwrap();
        app.run("type.char", json!({"size":24})).unwrap();
        let mut h = Harness::builder().with_size(vec2(360.0, 240.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let tokens = Tokens::get(ui.ctx());
                style_row(app, ui, true, "Header", (Some("Header"), true), &[], &tokens);
            },
            app,
        );
        h.run_steps(3);
        let at = egui::pos2(30.0, 15.0);
        h.event(egui::Event::PointerMoved(at));
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() });
            h.run_steps(2);
        }
        h.get_by_label("Redefine Style").click();
        h.run_steps(3);
        assert_eq!(h.state().session.doc().unwrap().doc.styles.para("Header").unwrap().chars.size, Some(24.0));
    }

    /// The Object Styles panel: the selected frame's style is marked (`+` once overridden), a
    /// click applies a style, the eraser clears the overrides, + makes a style from the selection.
    #[test]
    fn object_styles_panel_applies_marks_overrides_and_creates() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [36, 36, 136, 136], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        app.run("style.object.create", json!({"name": "Framed", "fromSelection": false, "stroke": {"swatch": "[Paper]", "weight": 3}})).unwrap();
        let mut h = Harness::builder().with_size(vec2(320.0, 260.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                object(app, ui);
            },
            app,
        );
        h.run_steps(4);
        h.get_by_label("Framed").click();
        h.run_steps(3);
        // The style on top and its row.
        assert_eq!(h.get_all_by_label("Framed").count(), 2);
        let style = |h: &Harness<DesignApp>| h.state().session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().clone();
        assert_eq!((style(&h).object_style.as_str(), style(&h).stroke.weight), ("Framed", 3.0));
        h.state_mut().run("object.stroke", json!({"weight": 1})).unwrap();
        h.run_steps(3);
        assert_eq!(h.get_all_by_label("Framed+").count(), 2);
        h.get_by_label("Clear Overrides").click();
        h.run_steps(3);
        assert_eq!(style(&h).stroke.weight, 3.0);
        assert_eq!(h.get_all_by_label("Framed").count(), 2);
        h.get_by_label("Create New Style").click();
        h.run_steps(3);
        let styles = h.state().session.active().unwrap().doc.styles.object.iter().map(|o| o.name.clone()).collect::<Vec<_>>();
        assert!(styles.contains(&"Object Style 1".to_string()), "{styles:?}");
    }

    /// Clear Overrides is on when any selected frame differs from its style, not only the first;
    /// the label on top describes the first.
    #[test]
    fn clear_overrides_is_on_when_a_later_selected_frame_is_overridden() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let frame = |app: &mut DesignApp, x: f64| {
            app.run("frame.create", json!({"rect": [x, 36, x + 100.0, 136], "content": "graphic"})).unwrap()["id"].as_u64().unwrap()
        };
        let (a, b) = (frame(&mut app, 36.0), frame(&mut app, 200.0));
        app.run("style.object.create", json!({"name": "Framed", "fromSelection": false, "stroke": {"swatch": "[Paper]", "weight": 3}})).unwrap();
        app.run("style.object.apply", json!({"name": "Framed", "ids": [a, b]})).unwrap();
        app.run("object.stroke", json!({"weight": 1, "ids": [b]})).unwrap();
        app.run("selection.set", json!({"ids": [a, b]})).unwrap();
        let mut h = Harness::builder().with_size(vec2(320.0, 260.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                object(app, ui);
            },
            app,
        );
        h.run_steps(4);
        assert_eq!(h.get_all_by_label("Framed").count(), 2, "the first frame follows its style");
        h.get_by_label("Clear Overrides").click();
        h.run_steps(3);
        let weight = |h: &Harness<DesignApp>| h.state().session.active().unwrap().doc.item(designcraft_doc::ItemId(b)).unwrap().stroke.weight;
        assert_eq!(weight(&h), 3.0);
    }

    #[test]
    fn paragraph_color_override_is_marked_and_cleared_from_the_row_menu() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let f = app.run("frame.create", json!({"rect": [36,36,300,200], "content": "text", "text": "Header"})).unwrap();
        app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 6})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Header", "chars": {"fill": "[Black]"}})).unwrap();
        app.run("style.paragraph.apply", json!({"name": "Header"})).unwrap();
        app.run("style.character.create", json!({"name": "Emphasis", "chars": {"underline": true}})).unwrap();
        app.run("style.character.apply", json!({"name": "Emphasis"})).unwrap();
        app.run("type.char", json!({"fill": "[Paper]"})).unwrap();
        let mut h = Harness::builder().with_size(vec2(460.0, 300.0)).build_ui_state(|ui, app: &mut DesignApp| paragraph(app, ui), app);
        h.run_steps(3);
        let at = h.get_all_by_label("Header+").next_back().unwrap().rect().center();
        h.event(egui::Event::PointerMoved(at));
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() });
            h.run_steps(2);
        }
        h.get_by_label("Apply \"Header\", Clear Overrides").click();
        h.run_steps(3);
        let app = h.state_mut();
        let a = app.run("type.selectionAttrs", json!({})).unwrap();
        assert_eq!(a["chars"]["fill"], "[Black]");
        assert_eq!(a["chars"]["underline"], true);
        assert_eq!(a["characterStyle"], "Emphasis");
        assert_eq!(a["charOverrides"], 0);
        assert_eq!(a["paraOverrides"], 0);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.run("type.selectionAttrs", json!({})).unwrap()["chars"]["fill"], "[Paper]");
    }

    #[test]
    fn named_character_styles_and_redundant_local_values_are_not_paragraph_overrides() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let f = app.run("frame.create", json!({"rect": [36,36,300,200], "content": "text", "text": "Body text"})).unwrap();
        app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 4})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Body", "para": {"spaceAfter": 6}, "chars": {"size": 10}})).unwrap();
        app.run("style.paragraph.apply", json!({"name": "Body"})).unwrap();
        app.run("style.character.create", json!({"name": "Italic", "chars": {"fontStyle": "Italic"}})).unwrap();
        app.run("style.character.apply", json!({"name": "Italic"})).unwrap();
        app.run("type.char", json!({"size": 10, "fontStyle": "Italic"})).unwrap();
        app.run("type.para", json!({"spaceAfter": 6})).unwrap();
        let a = app.run("type.selectionAttrs", json!({})).unwrap();
        assert_eq!((a["paraOverrides"].as_u64(), a["charOverrides"].as_u64()), (Some(0), Some(0)));
        let mut h = Harness::builder().with_size(vec2(360.0, 240.0)).build_ui_state(|ui, app: &mut DesignApp| paragraph(app, ui), app);
        h.run_steps(3);
        assert!(h.query_by_label("Body+").is_none());
        assert!(h.query_all_by_label("Body").next().is_some());
    }

    #[test]
    fn character_style_plus_captures_selection_for_reuse() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let frame = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "alpha beta"})).unwrap();
        app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 5})).unwrap();
        app.run("type.char", json!({"underline": true})).unwrap();
        let selection = app.session.doc().unwrap().selection.clone();
        let undo_len = app.session.doc().unwrap().history.undo.len();
        let mut h = Harness::builder().with_size(vec2(360.0, 240.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                character(app, ui);
            },
            app,
        );
        h.run_steps(3);
        h.get_by_label("Create New Style from Selection").click();
        h.run_steps(3);
        let app = h.state_mut();
        let style = app.session.doc().unwrap().doc.styles.char_style("Character Style 1").unwrap();
        assert_eq!(style.chars, designcraft_doc::CharAttrs { underline: Some(true), ..Default::default() });
        assert_eq!(app.session.doc().unwrap().selection, selection);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), undo_len + 1);
        app.run("text.select", json!({"story": frame["story"], "anchor": 6, "focus": 10})).unwrap();
        app.run("style.character.apply", json!({"name": "Character Style 1"})).unwrap();
        assert_eq!(app.run("type.selectionAttrs", json!({})).unwrap()["chars"]["underline"], true);
    }

    #[cfg(not(target_arch = "wasm32"))]
    use egui_kittest::kittest::NodeT as _;

    #[cfg(not(target_arch = "wasm32"))]
    fn text_style_harness(mut app: DesignApp, para: bool, sdk: bool, enabled: bool) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        app.framework_transform = sdk;
        app.framework_opacity = false;
        app.install_framework(&ctx);
        assert_eq!(app.framework_installed, sdk, "the harness must exercise the requested SDK or fallback branch");
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(380.0, 520.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), "");
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                ui.add_enabled_ui(enabled, |ui| if para { paragraph(app, ui) } else { character(app, ui) });
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        text_style_steps(&mut h, 3);
        h
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_style_event(h: &mut Harness<'_, DesignApp>, event: egui::Event) {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(event);
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_style_steps(h: &mut Harness<'_, DesignApp>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_style_pointer(h: &mut Harness<'_, DesignApp>, at: egui::Pos2, pressed: bool) {
        text_style_event(h, egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_style_click(h: &mut Harness<'_, DesignApp>, label: &str) {
        let at = h.get_by_role_and_label(egui::accesskit::Role::Button, label).rect().center();
        text_style_event(h, egui::Event::PointerMoved(at));
        text_style_pointer(h, at, true);
        text_style_pointer(h, at, false);
        text_style_steps(h, 3);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_style_space(h: &mut Harness<'_, DesignApp>) {
        for pressed in [true, false] {
            text_style_event(
                h,
                egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE },
            );
        }
        text_style_steps(h, 3);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_style_commands(h: &Harness<'_, DesignApp>) -> Vec<(String, serde_json::Value)> {
        h.state().session.journal.iter().filter(|(id, _)| id.starts_with("style.")).cloned().collect()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_style_undo_len(h: &Harness<'_, DesignApp>) -> usize {
        h.state().session.doc().unwrap().history.undo.len()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_style_select(app: &mut DesignApp, story: designcraft_doc::StoryId, anchor: usize, focus: usize) {
        app.run("text.select", json!({"story": story.0, "anchor": anchor, "focus": focus})).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_style_fixture(para: bool) -> (DesignApp, designcraft_doc::StoryId, designcraft_doc::StoryId) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let source = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "source\nunchanged"})).unwrap();
        let target = app.run("frame.create", json!({"rect": [36, 220, 300, 400], "content": "text", "text": "zero béta tail\nother"})).unwrap();
        let source = designcraft_doc::StoryId(source["story"].as_u64().unwrap());
        let target = designcraft_doc::StoryId(target["story"].as_u64().unwrap());
        text_style_select(&mut app, source, 0, 6);
        if para {
            app.run("type.char", json!({"size": 23, "tracking": 42, "underline": true})).unwrap();
            app.run("type.para", json!({"align": "center", "spaceAfter": 9})).unwrap();
        } else {
            // A character style must capture local character attributes, not the paragraph's size/spacing.
            app.run("style.paragraph.create", json!({"name": "Source base", "chars": {"size": 29}, "para": {"spaceAfter": 17}})).unwrap();
            app.run("style.paragraph.apply", json!({"name": "Source base"})).unwrap();
            app.run("type.char", json!({"tracking": 37, "underline": true})).unwrap();
        }
        (app, source, target)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_style_create_apply_journey(para: bool) {
        let (app, source, target) = text_style_fixture(para);
        let source_selection = app.session.doc().unwrap().selection.clone();
        let source_before = app.session.doc().unwrap().doc.story(source).unwrap().clone();
        let target_before = app.session.doc().unwrap().doc.story(target).unwrap().clone();
        let mut h = text_style_harness(app, para, true, true);
        let before = text_style_undo_len(&h);
        let create = if para { "style.paragraph.create" } else { "style.character.create" };
        let apply = if para { "style.paragraph.apply" } else { "style.character.apply" };
        let name = if para { "Paragraph Style 1" } else { "Character Style 1" };
        text_style_click(&mut h, "Create New Style from Selection");
        assert_eq!(text_style_commands(&h).last().unwrap(), &(create.into(), json!({"name": name, "fromSelection": true})));
        assert_eq!(text_style_undo_len(&h), before + 1);
        assert_eq!(h.state().session.doc().unwrap().selection, source_selection, "creation must not apply or retarget the selection");
        assert_eq!(h.state().session.doc().unwrap().doc.story(source).unwrap(), &source_before);
        assert_eq!(h.state().session.doc().unwrap().doc.story(target).unwrap(), &target_before);
        if para {
            let style = h.state().session.doc().unwrap().doc.styles.para(name).unwrap();
            assert_eq!((style.chars.size, style.chars.tracking, style.chars.underline), (Some(23.0), Some(42.0), Some(true)));
            assert_eq!(style.para.space_after, Some(9.0));
        } else {
            let style = h.state().session.doc().unwrap().doc.styles.char_style(name).unwrap();
            assert_eq!(style.chars, designcraft_doc::CharAttrs { tracking: Some(37.0), underline: Some(true), ..Default::default() });
        }
        let commands = text_style_commands(&h);
        text_style_steps(&mut h, 3);
        assert_eq!(text_style_commands(&h), commands, "an idle pass must not replay creation");
        text_style_select(h.state_mut(), target, 5, "zero béta".len());
        let selection = h.state().session.doc().unwrap().selection.clone();
        let target_attrs = h.state_mut().run("type.selectionAttrs", json!({})).unwrap();
        text_style_steps(&mut h, 3);
        // This is the existing production style_row, not a replacement apply button or direct apply setter.
        text_style_click(&mut h, name);
        assert_eq!(text_style_commands(&h).last().unwrap(), &(apply.into(), json!({"name": name, "clearOverrides": false})));
        assert_eq!(text_style_undo_len(&h), before + 2, "create and apply are independent Undo steps");
        assert_eq!(h.state().session.doc().unwrap().selection, selection);
        let attrs = h.state_mut().run("type.selectionAttrs", json!({})).unwrap();
        let story = h.state().session.doc().unwrap().doc.story(target).unwrap();
        assert_eq!(story.text, target_before.text);
        if para {
            assert_eq!(story.paras[0].style, name);
            assert_eq!(story.paras[1], target_before.paras[1], "the other paragraph is unchanged");
            assert_eq!(attrs["paragraphStyle"], name);
            assert_eq!(attrs["chars"]["size"], 23.0);
            assert_eq!(attrs["chars"]["tracking"], 42.0);
            assert_eq!(attrs["para"]["spaceAfter"], 9.0);
            assert_eq!(attrs["para"]["align"], "center");
        } else {
            assert_eq!(story.paras, target_before.paras, "a character style must not freeze source paragraph formatting");
            assert_eq!(attrs["characterStyle"], name);
            assert_eq!(attrs["chars"]["size"], target_attrs["chars"]["size"], "size stays inherited from the target");
            assert_eq!(attrs["chars"]["tracking"], 37.0);
            assert_eq!(attrs["chars"]["underline"], true);
            for (pos, _) in story.text.char_indices() {
                assert_eq!(story.char_format_of(pos).style == name, (5.."zero béta".len()).contains(&pos), "exact byte {pos}");
            }
        }
        assert_eq!(h.state().session.doc().unwrap().doc.story(source).unwrap(), &source_before);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        text_style_steps(&mut h, 3);
        assert_eq!(h.state().session.doc().unwrap().doc.story(target).unwrap(), &target_before);
        assert_eq!(text_style_undo_len(&h), before + 1);
        let styles = &h.state().session.doc().unwrap().doc.styles;
        assert!(if para { styles.para(name).is_some() } else { styles.char_style(name).is_some() });
        h.state_mut().run("edit.undo", json!({})).unwrap();
        text_style_steps(&mut h, 3);
        let styles = &h.state().session.doc().unwrap().doc.styles;
        assert!(if para { styles.para(name).is_none() } else { styles.char_style(name).is_none() });
        assert_eq!(text_style_undo_len(&h), before);
        assert_eq!(h.state().session.doc().unwrap().selection, source_selection);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_paragraph_create_from_selection_applies_through_real_row_and_undo() {
        text_style_create_apply_journey(true);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_character_create_from_selection_applies_through_real_row_and_undo() {
        text_style_create_apply_journey(false);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_text_style_create_no_selection_unique_names_keyboard_and_no_replay() {
        for para in [true, false] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
            app.run("file.new", json!({})).unwrap();
            let mut h = text_style_harness(app, para, true, true);
            let before = text_style_undo_len(&h);
            let name = if para { "Paragraph Style 1" } else { "Character Style 1" };
            let command = if para { "style.paragraph.create" } else { "style.character.create" };
            assert!(!h.get_by_label("Create New Style from Selection").accesskit_node().is_disabled(), "no new no-selection gate");
            text_style_click(&mut h, "Create New Style from Selection");
            // SDK Button has a keyboard stop but no TAKE_FOCUS sense: its pointer click must not
            // be treated as keyboard focus. Reach the actual control with an ordinary Tab before
            // Space, keeping the same unique-name, command, Undo-count and no-replay assertions.
            assert!(
                !h.get_by_label("Create New Style from Selection").is_focused(),
                "the pointer click alone must not be mistaken for keyboard focus"
            );
            // The style rows are keyboard stops of their own now (they are framework buttons), so natural Tab
            // passes through them before it reaches the create control: press it until the control is focused.
            let create_focused =
                |h: &Harness<'_, DesignApp>| h.get_by_role_and_label(egui::accesskit::Role::Button, "Create New Style from Selection").is_focused();
            for _ in 0..24 {
                if create_focused(&h) {
                    break;
                }
                for pressed in [true, false] {
                    text_style_event(
                        &mut h,
                        egui::Event::Key { key: egui::Key::Tab, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE },
                    );
                }
                text_style_steps(&mut h, 3);
            }
            assert!(
                h.get_by_role_and_label(egui::accesskit::Role::Button, "Create New Style from Selection").is_focused(),
                "natural Tab reaches the create control before Space; focused widget: {:?}",
                h.ctx.memory(|m| m.focused())
            );
            text_style_space(&mut h);
            assert_eq!(text_style_undo_len(&h), before + 2);
            assert_eq!(
                text_style_commands(&h),
                vec![(command.into(), json!({"name": name, "fromSelection": true})), (command.into(), json!({"name": name, "fromSelection": true})),]
            );
            let styles = &h.state().session.doc().unwrap().doc.styles;
            if para {
                assert_eq!(styles.para(name).unwrap().chars, designcraft_doc::CharAttrs::default());
                assert!(styles.para(&format!("{name} 2")).is_some());
            } else {
                assert_eq!(styles.char_style(name).unwrap().chars, designcraft_doc::CharAttrs::default());
                assert!(styles.char_style(&format!("{name} 2")).is_some());
            }
            let commands = text_style_commands(&h);
            text_style_steps(&mut h, 3);
            assert_eq!(text_style_commands(&h), commands);
            assert_eq!(text_style_undo_len(&h), before + 2);
            for remaining in [before + 1, before] {
                h.state_mut().run("edit.undo", json!({})).unwrap();
                text_style_steps(&mut h, 3);
                assert_eq!(text_style_undo_len(&h), remaining);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_text_style_create_captured_invalid_paragraph_is_refused_without_edit() {
        let (mut app, source, target) = text_style_fixture(true);
        // Synthetic imported-document fixture: unlike a UI edit, imported formatting can be invalid.
        // Only fixture setup writes it directly; the refusal is exercised through the real create control.
        let doc = std::sync::Arc::make_mut(&mut app.session.doc_mut().unwrap().doc);
        let story = std::sync::Arc::make_mut(doc.stories.get_mut(&source).unwrap());
        story.paras[0].para.left_indent = Some(-1.0);
        let original = app.session.doc().unwrap().doc.clone();
        let selection = app.session.doc().unwrap().selection.clone();
        let mut h = text_style_harness(app, true, true, true);
        let before = text_style_undo_len(&h);
        let commands = text_style_commands(&h);
        text_style_click(&mut h, "Create New Style from Selection");
        assert!(h.state().ui.status.contains("style.paragraph.create"));
        assert!(h.state().ui.status.contains("leftIndent"));
        assert_eq!(h.state().session.doc().unwrap().doc, original);
        assert_eq!(h.state().session.doc().unwrap().selection, selection);
        assert_eq!(h.state().session.doc().unwrap().doc.story(target).unwrap(), original.story(target).unwrap());
        assert_eq!(text_style_undo_len(&h), before);
        assert_eq!(text_style_commands(&h), commands, "a refusal is not journaled as a successful create");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_text_style_create_compact_localized_disabled_and_uninstalled_fallback() {
        for para in [true, false] {
            for sdk in [true, false] {
                for enabled in [true, false] {
                    let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
                    app.run("file.new", json!({})).unwrap();
                    app.ui.language = "ja".into();
                    let mut h = text_style_harness(app, para, sdk, enabled);
                    let label = crate::i18n::tr("ja", "Create New Style from Selection");
                    let control = h.get_by_role_and_label(egui::accesskit::Role::Button, label);
                    let size = control.rect().size();
                    if sdk {
                        assert!(
                            size.x > 0.0 && size.x <= 64.0 && size.y > 0.0 && size.y <= 48.0,
                            "the plus stays compact at the SDK default footprint: {size:?}"
                        );
                    } else {
                        assert_eq!(size, vec2(20.0, 20.0), "the existing fallback footprint is unchanged");
                    }
                    assert!(control.rect().right() <= 380.0, "the action fits the actual panel parent");
                    assert_eq!(control.accesskit_node().is_disabled(), !enabled);
                    let before = text_style_undo_len(&h);
                    text_style_click(&mut h, label);
                    assert_eq!(text_style_undo_len(&h), before + usize::from(enabled));
                    assert_eq!(text_style_commands(&h).len(), usize::from(enabled));
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_text_style_create_document_identity_rejects_old_pointer_release() {
        for para in [true, false] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
            app.run("file.new", json!({})).unwrap();
            app.run("file.new", json!({})).unwrap();
            app.run("file.activate", json!({"index": 0})).unwrap();
            let first = app.session.doc().unwrap().uid;
            let mut h = text_style_harness(app, para, true, true);
            let label = "Create New Style from Selection";
            let at = h.get_by_role_and_label(egui::accesskit::Role::Button, label).rect().center();
            text_style_event(&mut h, egui::Event::PointerMoved(at));
            text_style_pointer(&mut h, at, true);
            h.state_mut().run("file.activate", json!({"index": 1})).unwrap();
            text_style_steps(&mut h, 3);
            assert_ne!(h.state().session.doc().unwrap().uid, first);
            text_style_pointer(&mut h, at, false);
            text_style_steps(&mut h, 3);
            assert!(text_style_commands(&h).is_empty(), "a pressed control from another document cannot create here");
            assert_eq!(text_style_undo_len(&h), 0);
            text_style_click(&mut h, label);
            assert_eq!(text_style_commands(&h).len(), 1, "positive control on the current document");
            assert_eq!(text_style_undo_len(&h), 1);
            h.state_mut().run("file.activate", json!({"index": 0})).unwrap();
            text_style_steps(&mut h, 3);
            assert_eq!(h.state().session.doc().unwrap().uid, first);
            assert_eq!(text_style_undo_len(&h), 0);
            let styles = &h.state().session.doc().unwrap().doc.styles;
            assert!(if para { styles.para("Paragraph Style 1").is_none() } else { styles.char_style("Character Style 1").is_none() });
        }
    }
}
