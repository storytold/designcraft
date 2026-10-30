//! Paragraph Styles, Character Styles and Object Styles panels.

use egui::{Sense, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{DesignApp, icons};

fn list(app: &mut DesignApp, ui: &mut egui::Ui, para: bool) {
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
            if icons::button(
                ui,
                "plus",
                20.0,
                false,
                crate::i18n::tr(&app.ui.language, crate::i18n::tr(&app.ui.language, "Create New Style from Selection")),
            )
            .clicked()
            {
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

/// The selected object's style, and whether the object differs from it (an override: `+`): its
/// own attributes, or a text frame's paragraphs in other styles than the object style gives them.
fn object_style_of_selection(app: &DesignApp) -> Option<(String, bool)> {
    let st = app.session.active()?;
    let it = st.selection.items.first().and_then(|i| st.doc.item(*i))?;
    let mut styled = it.clone();
    let mut over = false;
    if let Some(os) = st.doc.styles.object_style(&it.object_style) {
        os.apply_to(&mut styled);
        if let Some(tf) = it.text_frame() {
            over = st.doc.story(tf.story).is_some_and(|s| !os.story_follows(&st.doc.styles, s));
        }
    }
    Some((it.object_style.clone(), over || styled != *it))
}

/// Window › Styles › Object Styles, as InDesign's: the selected object's style on top (`+` when
/// it is overridden), the document's object styles (a click applies one to the selection, a
/// double-click opens its options; the default graphic and text frame styles are marked), and at
/// the bottom Clear Overrides and Create New Style (⌥/Alt-click: the New Object Style dialog).
pub fn object(app: &mut DesignApp, ui: &mut egui::Ui) {
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
            let over = current.as_ref().is_some_and(|c| c.1);
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
}
