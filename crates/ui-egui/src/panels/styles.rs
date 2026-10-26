//! Paragraph Styles and Character Styles panels.

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
        style_row(app, ui, para, n, current.as_deref(), overrides > 0, &groups, &t);
    }
    for g in &groups {
        egui::CollapsingHeader::new(g.as_str()).id_salt(("style_group", para, g)).default_open(true).show(ui, |ui| {
            for n in names.iter().filter(|n| n.rsplit_once('/').is_some_and(|(x, _)| x == g)) {
                style_row(app, ui, para, n, current.as_deref(), overrides > 0, &groups, &t);
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

/// One style in the list: click applies (⌥ clears overrides), double-click edits,
/// right-click: edit, apply, break link, move to group.
fn style_row(app: &mut DesignApp, ui: &mut egui::Ui, para: bool, n: &str, current: Option<&str>, overridden: bool, groups: &[String], t: &Tokens) {
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
                style_row(app, ui, true, "Header", Some("Header"), true, &[], &tokens);
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
