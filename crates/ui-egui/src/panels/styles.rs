//! Paragraph Styles and Character Styles panels.

use egui::{Sense, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{DesignApp, icons};

fn list(app: &mut DesignApp, ui: &mut egui::Ui, para: bool) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let names = style_names(&st.doc, para);
    let groups = st.doc.styles.style_groups(para);
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
    // Groups as folders (those of `Group/Name` names and the empty ones), ungrouped styles first.
    let picked = picked(ui, para, &names);
    for n in names.iter().filter(|n| !n.contains('/')) {
        if para && n == designcraft_doc::NO_PARA_STYLE {
            continue;
        }
        style_row(app, ui, para, n, current.as_deref(), overrides > 0, &groups, &t, &picked, 0.0);
    }
    for g in &groups {
        if group_row(app, ui, para, g, &t) {
            for n in names.iter().filter(|n| n.rsplit_once('/').is_some_and(|(x, _)| x == g)) {
                style_row(app, ui, para, n, current.as_deref(), overrides > 0, &groups, &t, &picked, 16.0);
            }
        }
    }
    // A drop below the styles takes them out of their group.
    let (rest, rest_resp) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    if hovering(&rest_resp, para) {
        ui.painter().line_segment([rest.left_top(), rest.right_top()], egui::Stroke::new(2.0, t.accent));
    }
    if let Some(names) = dropped(&rest_resp, para) {
        move_to_group(app, para, &names, "");
    }
    bottom_bar(app, ui, para, current.as_deref(), overrides > 0);
}

/// The styles a row drag carries (the panel's selection when the row is in it), and which panel
/// they come from: the two panels' styles can share names.
#[derive(Clone, Debug)]
struct StyleDrag {
    para: bool,
    names: Vec<String>,
}

/// A drag of this panel's kind of styles is over `resp`: one from the other panel isn't.
fn hovering(resp: &egui::Response, para: bool) -> bool {
    resp.dnd_hover_payload::<StyleDrag>().is_some_and(|p| p.para == para)
}

/// The styles of this panel's kind dropped on `resp`. A drop from the other panel is left alone.
fn dropped(resp: &egui::Response, para: bool) -> Option<Vec<String>> {
    if !hovering(resp, para) {
        return None;
    }
    resp.dnd_release_payload::<StyleDrag>().map(|p| p.names.clone())
}

/// The document's paragraph or character style names, in panel order.
fn style_names(doc: &designcraft_doc::Document, para: bool) -> Vec<String> {
    if para { doc.styles.paragraph.iter().map(|s| s.name.clone()).collect() } else { doc.styles.character.iter().map(|s| s.name.clone()).collect() }
}

/// The panel's selected styles: more than one after Ctrl/Cmd- or Shift-clicks.
fn picked_id(para: bool) -> egui::Id {
    egui::Id::unique(("styles_picked", para))
}

/// The picked styles that still exist: a move, rename or delete leaves old names behind.
fn picked(ui: &egui::Ui, para: bool, names: &[String]) -> Vec<String> {
    let picked: Vec<String> = ui.data(|d| d.get_temp(picked_id(para))).unwrap_or_default();
    picked.into_iter().filter(|p| names.contains(p)).collect()
}

fn kind_of(para: bool) -> &'static str {
    if para { "paragraph" } else { "character" }
}

/// Into group `group` ("" = out of any).
fn move_to_group(app: &mut DesignApp, para: bool, names: &[String], group: &str) {
    let names: Vec<&String> = names.iter().filter(|n| !n.starts_with('[')).collect();
    if !names.is_empty() {
        let _ = app.run("style.group", json!({"kind": kind_of(para), "names": names, "group": group}));
    }
}

/// A group's row, as InDesign's: the disclosure triangle (a click opens or closes it), a folder
/// and the name (a double-click renames it); styles dropped on it move into it. Returns whether
/// it is open.
fn group_row(app: &mut DesignApp, ui: &mut egui::Ui, para: bool, g: &str, t: &Tokens) -> bool {
    let open_id = egui::Id::unique(("style_group_open", para, g));
    let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(true);
    let rename_id = egui::Id::unique(("style_group_rename", para));
    // Set when a rename starts: the field takes the focus once, so a click elsewhere commits it.
    let focus_id = egui::Id::unique(("style_group_rename_focus", para));
    let renaming: Option<(String, String)> = ui.data(|d| d.get_temp(rename_id));
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    let dropping = hovering(&resp, para);
    if dropping {
        ui.painter().rect_stroke(row.shrink(1.0), 0.0, egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        ui.painter().rect_filled(row, 0.0, t.hover);
    }
    let tri = egui::Rect::from_min_size(row.min + vec2(4.0, 5.0), vec2(12.0, 12.0));
    icons::paint(ui.painter(), tri, if open { "chevron-down" } else { "chevron-right" }, t.icon);
    icons::paint(ui.painter(), egui::Rect::from_min_size(row.min + vec2(18.0, 1.0), vec2(20.0, 20.0)), "folder", t.icon);
    let name_x = row.min.x + 42.0;
    match renaming {
        Some((rg, mut text)) if rg == g => {
            let field = egui::Rect::from_min_max(egui::pos2(name_x, row.min.y + 1.0), row.max - vec2(4.0, 1.0));
            let r = ui.put(field, egui::TextEdit::singleline(&mut text).font(egui::FontId::proportional(12.5)));
            if ui.data_mut(|d| d.remove_temp::<bool>(focus_id)).is_some() {
                r.request_focus();
            }
            let (enter, escape) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            if escape {
                ui.data_mut(|d| d.remove::<(String, String)>(rename_id));
            } else if enter || r.lost_focus() {
                ui.data_mut(|d| d.remove::<(String, String)>(rename_id));
                if !text.trim().is_empty()
                    && text.trim() != g
                    && let Err(e) = app.run("style.group.rename", json!({"kind": kind_of(para), "name": g, "to": text.trim()}))
                {
                    app.status(e);
                }
            } else {
                ui.data_mut(|d| d.insert_temp(rename_id, (g.to_string(), text)));
            }
        }
        _ => {
            crate::rtl::paint(
                ui.painter(),
                egui::pos2(name_x, row.center().y),
                egui::Align2::LEFT_CENTER,
                g,
                egui::FontId::proportional(12.5),
                t.text,
            );
        }
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, g));
    // A triple click counts too: egui reports quick clicks after others as one.
    if resp.double_clicked() || resp.triple_clicked() {
        ui.data_mut(|d| {
            d.insert_temp(rename_id, (g.to_string(), g.to_string()));
            d.insert_temp(focus_id, true);
        });
    } else if resp.clicked() && resp.interact_pointer_pos().is_none_or(|p| p.x < row.min.x + 40.0) {
        open = !open;
        ui.data_mut(|d| d.insert_temp(open_id, open));
    }
    if let Some(names) = dropped(&resp, para) {
        move_to_group(app, para, &names, g);
    }
    open
}

/// The panel's bottom bar, as InDesign's: Load Styles on the left; Create New Style Group, Clear
/// Overrides (paragraph styles), Create New Style (⌥/Alt-click: the New Paragraph/Character Style
/// dialog, from the selected text) and Delete on the right.
fn bottom_bar(app: &mut DesignApp, ui: &mut egui::Ui, para: bool, current: Option<&str>, overridden: bool) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let (label, cmd) =
            if para { ("Load Paragraph Styles…", "app.loadParagraphStyles") } else { ("Load Character Styles…", "app.loadCharacterStyles") };
        let load = icons::button(ui, "style-load", 20.0, false, &tr(label));
        egui::Popup::menu(&load).show(|ui| {
            for (label, cmd) in [(label, cmd), ("Load All Text Styles…", "app.loadTextStyles")] {
                if ui.button(crate::rtl::widget(ui, tr(label))).clicked() {
                    let _ = app.run(cmd, json!({}));
                    ui.close();
                }
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Delete: the selected style (the text's, or the one picked in the panel). It deletes
            // one style, so with several picked it is off rather than deleting another.
            let names = app.session.active().map(|st| style_names(&st.doc, para)).unwrap_or_default();
            let picked = picked(ui, para, &names);
            let target = match picked.len() {
                0 => current.map(str::to_string),
                1 => picked.first().cloned(),
                _ => None,
            };
            let deletable = target.as_deref().is_some_and(|n| !n.starts_with('['));
            let delete_label = tr(if para { "Delete Paragraph Style…" } else { "Delete Character Style…" });
            if ui.add_enabled_ui(deletable, |ui| icons::button(ui, "trash", 20.0, false, &delete_label)).inner.clicked()
                && let Some(n) = target.filter(|_| deletable)
            {
                let id = if para { "deleteParagraphStyle" } else { "deleteCharacterStyle" };
                app.ui.dialog = Some(crate::dialogs::Dialog::new(id, json!({"name": n})));
            }
            if icons::button(ui, "style-new", 20.0, false, &tr("Create New Style from Selection")).clicked() {
                if ui.input(|i| i.modifiers.alt) {
                    crate::dialogs::open_new_style(app, para);
                } else {
                    let cmd = if para { "style.paragraph.create" } else { "style.character.create" };
                    let base = if para { "Paragraph Style 1" } else { "Character Style 1" };
                    let _ = app.run(cmd, json!({"name": base, "fromSelection": true}));
                }
            }
            if para
                && ui.add_enabled_ui(overridden, |ui| icons::button(ui, "style-clear", 20.0, false, &tr("Clear overrides"))).inner.clicked()
                && let Some(n) = current
            {
                let _ = app.run("style.paragraph.apply", json!({"name": n, "clearOverrides": true}));
            }
            if icons::button(ui, "folder", 20.0, false, &tr("New Style Group")).clicked() {
                let _ = app.run("style.group.create", json!({"kind": kind_of(para)}));
            }
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
#[allow(clippy::too_many_arguments)]
fn style_row(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    para: bool,
    n: &str,
    current: Option<&str>,
    overridden: bool,
    groups: &[String],
    t: &Tokens,
    picked: &[String],
    indent: f32,
) {
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click_and_drag());
    // Several styles picked (Ctrl/Cmd- or Shift-click) show as the selection; else the text's.
    let selected = if picked.len() > 1 { picked.iter().any(|p| p == n) } else { current == Some(n) };
    if selected {
        ui.painter().rect_filled(row, 0.0, t.row_selected);
    } else if resp.hovered() {
        ui.painter().rect_filled(row, 0.0, t.hover);
    }
    // Dragged onto a group (or below the styles), the styles move; built-in ones stay.
    if !n.starts_with('[') {
        let names = if picked.len() > 1 && picked.iter().any(|p| p == n) { picked.to_vec() } else { vec![n.to_string()] };
        resp.dnd_set_drag_payload(StyleDrag { para, names });
    }
    if !n.contains('/')
        && let Some(names) = dropped(&resp, para)
    {
        move_to_group(app, para, &names, "");
    }
    let shown = n.rsplit('/').next().unwrap_or(n);
    let label = if n.contains('/') { shown } else { crate::i18n::style_name(&app.ui.language, shown) };
    let row_label = format!("{label}{}", if current == Some(n) && overridden { "+" } else { "" });
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &row_label));
    crate::rtl::paint(
        ui.painter(),
        row.min + vec2(8.0 + indent, 11.0),
        egui::Align2::LEFT_CENTER,
        &row_label,
        egui::FontId::proportional(12.5),
        t.text,
    );
    let editable = n != if para { designcraft_doc::NO_PARA_STYLE } else { designcraft_doc::NO_CHAR_STYLE };
    let (add, alt) = ui.input(|i| (i.modifiers.command || i.modifiers.shift, i.modifiers.alt));
    if resp.double_clicked() && editable {
        crate::dialogs::open_style_options(app, para, n);
    } else if resp.clicked() && add {
        // Ctrl/Cmd- or Shift-click: one more (or one fewer) picked, to move together.
        let mut picked: Vec<String> = if picked.is_empty() { current.map(|c| vec![c.to_string()]).unwrap_or_default() } else { picked.to_vec() };
        match picked.iter().position(|p| p == n) {
            Some(i) => {
                picked.remove(i);
            }
            None => picked.push(n.to_string()),
        }
        ui.data_mut(|d| d.insert_temp(picked_id(para), picked));
    } else if resp.clicked() {
        let cmd = if para { "style.paragraph.apply" } else { "style.character.apply" };
        let _ = app.run(cmd, json!({"name": n, "clearOverrides": alt}));
        ui.data_mut(|d| d.insert_temp(picked_id(para), vec![n.to_string()]));
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
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    use super::*;

    /// Both panels side by side, at 60 frames a second (two clicks make a double-click).
    fn both_panels(app: DesignApp) -> Harness<'static, DesignApp> {
        let mut h = Harness::builder().with_size(vec2(640.0, 420.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("styles_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        paragraph(app, ui);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        character(app, ui);
                    });
                });
            },
            app,
        );
        h.run_steps(3);
        h
    }

    fn drag(h: &mut Harness<'static, DesignApp>, from: egui::Pos2, to: egui::Pos2) {
        h.hover_at(from);
        h.drag_at(from);
        h.run_steps(1);
        for k in 1..=6 {
            h.hover_at(from + (to - from) * (k as f32 / 6.0));
            h.run_steps(1);
        }
        h.drop_at(to);
        h.run_steps(3);
    }

    /// A paragraph style dragged onto a group of the Character Styles panel changes neither kind
    /// of style, even when a character style has its name; a character style dragged there moves.
    #[test]
    fn a_style_dragged_into_the_other_panel_moves_nothing() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Body", "fromSelection": false})).unwrap();
        app.run("style.character.create", json!({"name": "Body", "fromSelection": false})).unwrap();
        app.run("style.group.create", json!({"kind": "character"})).unwrap();
        let mut h = both_panels(app);
        let names = |h: &Harness<'static, DesignApp>| {
            let st = &h.state().session.doc().unwrap().doc.styles;
            (st.paragraph.iter().map(|s| s.name.clone()).collect::<Vec<_>>(), st.character.iter().map(|s| s.name.clone()).collect::<Vec<_>>())
        };
        let before = names(&h);
        let bodies: Vec<egui::Pos2> = h.get_all_by_label("Body").map(|n| n.rect().center()).collect();
        assert_eq!(bodies.len(), 2);
        let (para_body, char_body) = if bodies[0].x < bodies[1].x { (bodies[0], bodies[1]) } else { (bodies[1], bodies[0]) };
        let group = h.get_by_label("Style Group 1").rect().center();
        drag(&mut h, para_body, group);
        assert_eq!(names(&h), before, "a paragraph style dropped on a character group moves nothing");
        drag(&mut h, char_body, group);
        let (paragraph, character) = names(&h);
        assert!(paragraph.contains(&"Body".to_string()) && character.contains(&"Style Group 1/Body".to_string()), "{character:?}");
    }

    /// The course's chapter 21: style groups as InDesign's. The folder button makes "Style
    /// Group 1"; styles dragged onto it move in (Ctrl-click picks two to drag together); a
    /// double-click renames it; ⌥/Alt-click on the new-style button opens New Paragraph Style
    /// from the selected text.
    #[test]
    fn style_groups_are_made_filled_by_dragging_and_renamed_in_the_panel() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        for n in ["Bullets", "Recipe Categories", "Recipe titles"] {
            app.run("style.paragraph.create", json!({"name": n, "fromSelection": false})).unwrap();
        }
        let f = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Eggs"})).unwrap();
        app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 4})).unwrap();
        // 60 frames a second, so two clicks make a double-click.
        let mut h = Harness::builder().with_size(vec2(320.0, 420.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("styles_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                paragraph(app, ui);
                crate::dialogs::show(app, ui.ctx());
            },
            app,
        );
        h.run_steps(3);
        h.get_by_label("New Style Group").click();
        h.run_steps(3);
        assert_eq!(h.state().session.doc().unwrap().doc.styles.style_groups(true), ["Style Group 1"]);
        let center = |h: &Harness<'_, DesignApp>, l: &str| h.get_by_label(l).rect().center();
        // Ctrl-click picks two styles; dragging one of them moves both into the group.
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        for l in ["Bullets", "Recipe Categories"] {
            let at = center(&h, l);
            h.hover_at(at);
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::COMMAND });
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::COMMAND,
            });
            h.run_steps(2);
        }
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        let (from, to) = (center(&h, "Bullets"), center(&h, "Style Group 1"));
        h.hover_at(from);
        h.drag_at(from);
        h.run_steps(1);
        for k in 1..=6 {
            h.hover_at(from + (to - from) * (k as f32 / 6.0));
            h.run_steps(1);
        }
        h.drop_at(to);
        h.run_steps(3);
        let names: Vec<String> = h.state().session.doc().unwrap().doc.styles.paragraph.iter().map(|s| s.name.clone()).collect();
        assert!(names.contains(&"Style Group 1/Bullets".into()) && names.contains(&"Style Group 1/Recipe Categories".into()), "{names:?}");
        assert!(names.contains(&"Recipe titles".into()), "the style not picked stays");
        // The picked names are gone, so the text's style shows as the selection again.
        let row = h.get_all_by_label("[Basic Paragraph]").find_map(|n| n.accesskit_node().toggled());
        assert_eq!(row, Some(egui::accesskit::Toggled::True), "the text's style is the selection");
        // A double-click renames the group, Enter commits it. (Past egui's triple-click window
        // from the clicks before.)
        h.run_steps(45);
        let at = center(&h, "Style Group 1");
        h.hover_at(at);
        for _ in 0..2 {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
            h.run_steps(1);
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
            h.run_steps(1);
        }
        h.run_steps(2);
        assert!(h.query_by_role(egui::accesskit::Role::TextInput).is_some(), "the name is being edited");
        h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.event(egui::Event::Text("Basic Styles".into()));
        h.run_steps(1);
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        assert_eq!(h.state().session.doc().unwrap().doc.styles.style_groups(true), ["Basic Styles"]);
        assert!(h.query_by_label("Basic Styles").is_some());
        // Renamed again, a click on another row commits the name: the field doesn't keep the focus.
        h.run_steps(45);
        let at = center(&h, "Basic Styles");
        h.hover_at(at);
        for _ in 0..2 {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
            h.run_steps(1);
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
            h.run_steps(1);
        }
        h.run_steps(2);
        h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.event(egui::Event::Text("Kitchen".into()));
        h.run_steps(1);
        let other = center(&h, "Recipe titles");
        h.hover_at(other);
        h.event(egui::Event::PointerButton { pos: other, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
        h.event(egui::Event::PointerButton { pos: other, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
        h.run_steps(3);
        assert_eq!(h.state().session.doc().unwrap().doc.styles.style_groups(true), ["Kitchen"]);
        // ⌥/Alt-click on the new-style button: New Paragraph Style, from the selected text.
        let at = center(&h, "Create New Style from Selection");
        h.hover_at(at);
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::ALT });
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::ALT });
        h.run_steps(3);
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        let d = h.state().ui.dialog.clone().expect("New Paragraph Style");
        assert_eq!((d.id.as_str(), d.b("new")), ("paragraphStyleOptions", true));
    }

    /// Delete removes one style: with the text's style and a Ctrl-clicked one picked, it is off
    /// rather than deleting the text's style; with one picked it deletes that one.
    #[test]
    fn delete_is_off_with_several_styles_picked() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        for n in ["Bullets", "Recipe titles"] {
            app.run("style.paragraph.create", json!({"name": n, "fromSelection": false})).unwrap();
        }
        let f = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Eggs"})).unwrap();
        app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 4})).unwrap();
        app.run("style.paragraph.apply", json!({"name": "Recipe titles"})).unwrap();
        let mut h = Harness::builder().with_size(vec2(320.0, 420.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("styles_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                paragraph(app, ui);
            },
            app,
        );
        h.run_steps(3);
        let delete_off = |h: &Harness<'_, DesignApp>| h.get_by_label("Delete Paragraph Style…").accesskit_node().is_disabled();
        assert!(!delete_off(&h), "the text's style can be deleted");
        // Ctrl-click: Bullets picked with the text's Recipe titles.
        let at = h.get_by_label("Bullets").rect().center();
        h.hover_at(at);
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::COMMAND });
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::COMMAND });
        h.run_steps(2);
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        h.run_steps(2);
        assert!(delete_off(&h), "two picked: Delete is off");
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
                style_row(app, ui, true, "Header", Some("Header"), true, &[], &tokens, &[], 0.0);
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
        let mut h = Harness::builder().with_size(vec2(460.0, 300.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                // The panel's fonts are installed before its first frame (the icons draw with them).
                let id = egui::Id::unique("styles_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                paragraph(app, ui)
            },
            app,
        );
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
        let mut h = Harness::builder().with_size(vec2(360.0, 240.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                // The panel's fonts are installed before its first frame (the icons draw with them).
                let id = egui::Id::unique("styles_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                paragraph(app, ui)
            },
            app,
        );
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
