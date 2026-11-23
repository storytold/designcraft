//! Window › Output › Separations Preview: the plates the print PDF of the spread nearest the
//! view center puts on film, each ink's percentage under the pointer, one plate or all inks,
//! and the ink limit.

use craft_prepress::{Alternate, Ink, Process};
use serde_json::json;

use crate::DesignApp;
use crate::i18n::tr;
use crate::theme::Tokens;

/// A swatch for an ink's row.
fn ink_color(ink: &Ink) -> egui::Color32 {
    match ink {
        Ink::Process(Process::Cyan) => egui::Color32::from_rgb(0, 174, 239),
        Ink::Process(Process::Magenta) => egui::Color32::from_rgb(236, 0, 140),
        Ink::Process(Process::Yellow) => egui::Color32::from_rgb(255, 242, 0),
        Ink::Process(Process::Black) => egui::Color32::from_rgb(35, 31, 32),
        Ink::Spot(s) => {
            let rgb = match s.alternate {
                Alternate::Lab([l, a, b]) => designcraft_color::cms::lab::lab_to_srgb(designcraft_color::cms::Lab { l, a, b }),
                Alternate::Cmyk([c, m, y, k]) => designcraft_color::Color::Cmyk { c, m, y, k }.to_rgb(),
            };
            let [r, g, b] = rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            egui::Color32::from_rgb(r, g, b)
        }
    }
}

fn mode_button(ui: &mut egui::Ui, framework: bool, kind: &str, label: &str, on: bool) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::buttons::Button;
        use craft_ui_widgets::contract::{Props as _, Show as _};
        use craft_ui_widgets::style::Variant;

        return Button::new(label.to_owned()).id_salt(("separations-mode", kind)).variant(Variant::GHOST).selected(on).show(ui).clicked();
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (framework, kind);
    ui.selectable_label(on, crate::rtl::widget(ui, label)).clicked()
}

fn mode_actions(ui: &mut egui::Ui, lang: &str, framework: bool, on: bool, limit: Option<f32>) -> Option<serde_json::Value> {
    let mut run = None;
    let modes = [
        ("off", "Off", !on, json!({"plate": null, "inkLimit": null})),
        ("separations", "Separations", on && limit.is_none(), json!({"plate": "all", "inkLimit": null})),
        ("ink-limit", "Ink Limit", limit.is_some(), json!({"inkLimit": limit.unwrap_or(300.0)})),
    ];
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::contract::{Props as _, ShowContainer as _};
        use craft_ui_widgets::layout_components::Row;

        Row::new().id_salt("separations_modes").show(ui, |ui, _| {
            for (kind, label, selected, params) in &modes {
                if mode_button(ui, true, kind, tr(lang, label), *selected) && !*selected {
                    run = Some(params.clone());
                }
            }
        });
        return run;
    }
    ui.horizontal(|ui| {
        for (kind, label, selected, params) in modes {
            if mode_button(ui, framework, kind, tr(lang, label), selected) && !selected {
                run = Some(params);
            }
        }
    });
    run
}

// SEPARATIONS_PLATE_ADAPTER_BEGIN
/// A plate action belongs to the real displayed sheet and document incarnation, not an ink ordinal
/// from a previous result. Keep the preview's intentional last-known-sheet display unchanged.
fn ink_plate_button(
    app: &DesignApp,
    ui: &mut egui::Ui,
    sheet: &std::sync::Arc<designcraft_engine::separations::Sheet>,
    spread: usize,
    plate: Option<usize>,
    name: &str,
    selected: bool,
) -> bool {
    let owner = app.session.active().map(|st| (st.uid, st.identity_epoch, st.revision));
    let view = app.ui.separations.as_ref().map(|sep| (sep.plate.as_deref(), sep.ink_limit.map(f32::to_bits)));
    let id = ui.make_persistent_id(("separations-plate", owner, spread, std::sync::Arc::as_ptr(sheet) as usize, plate, name, selected, view));
    let enabled = app.session.active().is_some_and(|st| spread < st.doc.spreads.len()) && app.ui.separations.is_some();
    // The composite action and a real spot ink named “All Inks” must not share an action name.
    let label = if plate.is_some() { format!("{}: {name}", tr(&app.ui.language, "Plate")) } else { name.to_owned() };
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::contract::{Props as _, Show as _};
        return craft_ui_widgets::buttons::Button::new(label)
            .painted(name.to_owned())
            .id(id)
            .enabled(enabled)
            .selected(selected)
            .size(craft_theme::ControlSize::Sm)
            .variant(craft_ui_widgets::style::Variant::GHOST)
            .show(ui)
            .clicked();
    }
    ui.push_id(id, |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            let response = ui.selectable_label(selected, crate::rtl::widget(ui, name));
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, selected, label.clone()));
            response.clicked()
        })
        .inner
    })
    .inner
}
// SEPARATIONS_PLATE_ADAPTER_END

/// The panel on the framework: the mode buttons, the ink limit field, and a row for each plate (its swatch, its button and
/// the percentage under the pointer). The commands are the native panel's.
#[cfg(not(target_arch = "wasm32"))]
fn show_framework(app: &mut DesignApp, ui: &mut egui::Ui) {
    use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _, Sizing};
    use craft_ui_widgets::display::{Divider, Picture, Shade, Text};
    use craft_ui_widgets::form::Field;
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::list::ThumbAspect;
    use craft_ui_widgets::style::TypeRole;
    use craft_ui_widgets::text::NumberField;

    let lang = app.ui.language.clone();
    let sep = app.ui.separations.clone();
    let mut run: Option<serde_json::Value> = None;
    Stack::new().id_salt("separations").fill_width().show(ui, |ui, _| {
        if let Some(p) = mode_actions(ui, &lang, true, sep.is_some(), sep.as_ref().and_then(|s| s.ink_limit)) {
            run = Some(p);
        }
        if let Some(limit) = sep.as_ref().and_then(|s| s.ink_limit) {
            let label = tr(&lang, "Ink Limit");
            Field::new(label.to_owned()).id_salt("separations_ink_limit_field").sizing(Sizing::Content).show(ui, |ui, _| {
                let out = NumberField::new(label.to_owned(), Bind::given(Some(f64::from(limit))))
                    .id_salt("separations_ink_limit")
                    .unit("%".to_owned())
                    .range(0.0..=400.0)
                    .step(1.0)
                    .show(ui);
                if let Some(value) = out.edit.events.committed().copied().flatten().filter(|v| v.is_finite()) {
                    run = Some(json!({"inkLimit": value}));
                }
            });
        }
    });
    if let Some(p) = run.take() {
        if let Err(e) = app.run("view.separations", p) {
            app.status_error("Separations Preview", e);
        }
        return;
    }
    let note = |ui: &mut egui::Ui, text: String| {
        Text::new(text).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
    };
    let Some(sep) = sep else {
        Stack::new()
            .id_salt("separations_empty")
            .fill_width()
            .show(ui, |ui, _| note(ui, tr(&lang, "Shows the plates the exported PDF prints.").to_owned()));
        return;
    };
    let Some(designcraft_doc::SpreadRef::Doc(si)) = crate::canvas::current_spread(app) else { return };
    let sheet = match app.canvas.separations.sheet(si) {
        None => {
            Stack::new().id_salt("separations_wait").fill_width().show(ui, |ui, _| note(ui, tr(&lang, "Separating…").to_owned()));
            return;
        }
        Some(Err(e)) => {
            let message = e.to_string();
            Stack::new().id_salt("separations_error").fill_width().show(ui, |ui, _| note(ui, message.clone()));
            return;
        }
        Some(Ok(sheet)) => sheet.clone(),
    };
    // Each ink's percentage under the pointer.
    let k = f64::from(sheet.plates.dpi) / 72.0;
    let coverage = app.canvas.separations.pointer.filter(|(psi, _)| *psi == si).and_then(|(_, p)| {
        let (x, y) = ((p.x - sheet.area.x0) * k, (p.y - sheet.area.y0) * k);
        (x >= 0.0 && y >= 0.0).then(|| sheet.plates.coverage_at(x as u32, y as u32, &[])).flatten()
    });
    let pct = |v: Option<f32>| v.map_or_else(String::new, |v| format!("{v:.0}%"));
    let app_ref: &DesignApp = app;
    let row = |ui: &mut egui::Ui, selected: bool, color: Option<egui::Color32>, plate: Option<usize>, name: &str, value: String| -> bool {
        let mut clicked = false;
        Row::new().id_salt(("separations_row", plate)).fill_width().show(ui, |ui, _| {
            Picture::painter("", ThumbAspect::SQUARE, move |cx, rect| {
                if let Some(c) = color {
                    cx.painter.rect_filled(egui::Rect::from_center_size(rect.center(), egui::vec2(12.0, 12.0)), 1.0, c);
                }
            })
            .id_salt(("separations_swatch", plate))
            .show(ui);
            clicked |= ink_plate_button(app_ref, ui, &sheet, si, plate, name, selected);
            Text::new(value.clone()).role(TypeRole::Caption).show(ui);
        });
        clicked
    };
    let mut pick: Option<serde_json::Value> = None;
    Stack::new().id_salt("separations_plates").fill_width().show(ui, |ui, _| {
        Divider::horizontal().show(ui);
        if row(ui, sep.plate.is_none(), None, None, tr(&lang, "All Inks"), pct(coverage.as_ref().map(|c| c.total))) {
            pick = Some(json!({"plate": "all"}));
        }
        for (id, ink) in sheet.plates.inks.iter() {
            let selected = sep.plate.as_deref().and_then(|n| sheet.ink(n)) == Some(id);
            let value = pct(coverage.as_ref().and_then(|c| c.inks.get(id.index()).copied()));
            if row(ui, selected, Some(ink_color(ink)), Some(id.index()), ink.name(), value) {
                pick = Some(json!({"plate": if selected { "all" } else { ink.name() }}));
            }
        }
        for name in &sheet.plates.unseparated {
            note(ui, format!("{name} ({})", tr(&lang, "not separated")));
        }
    });
    if let Some(p) = pick
        && let Err(e) = app.run("view.separations", p)
    {
        app.status_error("Separations Preview", e);
    }
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        show_framework(app, ui);
        return;
    }
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let sep = app.ui.separations.clone();
    #[cfg(not(target_arch = "wasm32"))]
    let framework = app.framework_installed;
    #[cfg(target_arch = "wasm32")]
    let framework = false;
    let mut run = mode_actions(ui, &lang, framework, sep.is_some(), sep.as_ref().and_then(|s| s.ink_limit));
    if let Some(mut limit) = sep.as_ref().and_then(|s| s.ink_limit) {
        ui.horizontal(|ui| {
            ui.label(crate::rtl::widget(ui, tr(&lang, "Ink Limit")));
            if ui.add(egui::DragValue::new(&mut limit).range(0.0..=400.0).suffix("%")).changed() {
                run = Some(json!({"inkLimit": limit}));
            }
        });
    }
    if let Some(p) = run.take() {
        if let Err(e) = app.run("view.separations", p) {
            app.status_error("Separations Preview", e);
        }
        return;
    }
    let Some(sep) = sep else {
        ui.label(egui::RichText::new(tr(&lang, "Shows the plates the exported PDF prints.")).size(11.0).color(t.text_dim));
        return;
    };
    ui.separator();
    let Some(designcraft_doc::SpreadRef::Doc(si)) = crate::canvas::current_spread(app) else { return };
    let sheet = match app.canvas.separations.sheet(si) {
        None => {
            ui.label(egui::RichText::new(tr(&lang, "Separating…")).size(11.0).color(t.text_dim));
            return;
        }
        Some(Err(e)) => {
            ui.label(egui::RichText::new(e.as_str()).size(11.0).color(t.text_dim));
            return;
        }
        Some(Ok(sheet)) => sheet.clone(),
    };
    // Each ink's percentage under the pointer.
    let k = f64::from(sheet.plates.dpi) / 72.0;
    let coverage = app.canvas.separations.pointer.filter(|(psi, _)| *psi == si).and_then(|(_, p)| {
        let (x, y) = ((p.x - sheet.area.x0) * k, (p.y - sheet.area.y0) * k);
        (x >= 0.0 && y >= 0.0).then(|| sheet.plates.coverage_at(x as u32, y as u32, &[])).flatten()
    });
    let pct = |v: Option<f32>| v.map_or_else(String::new, |v| format!("{v:.0}%"));
    let row = |ui: &mut egui::Ui, selected: bool, color: Option<egui::Color32>, plate: Option<usize>, name: &str, value: String| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            if let Some(c) = color {
                ui.painter().rect_filled(r, 1.0, c);
            }
            let clicked = ink_plate_button(app, ui, &sheet, si, plate, name, selected);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new(value).size(11.0)));
            clicked
        })
        .inner
    };
    if row(ui, sep.plate.is_none(), None, None, tr(&lang, "All Inks"), pct(coverage.as_ref().map(|c| c.total))) {
        run = Some(json!({"plate": "all"}));
    }
    for (id, ink) in sheet.plates.inks.iter() {
        let selected = sep.plate.as_deref().and_then(|n| sheet.ink(n)) == Some(id);
        let value = pct(coverage.as_ref().and_then(|c| c.inks.get(id.index()).copied()));
        if row(ui, selected, Some(ink_color(ink)), Some(id.index()), ink.name(), value) {
            run = Some(json!({"plate": if selected { "all" } else { ink.name() }}));
        }
    }
    for name in &sheet.plates.unseparated {
        ui.label(egui::RichText::new(format!("{name} ({})", tr(&lang, "not separated"))).size(11.0).color(t.text_dim));
    }
    if let Some(p) = run
        && let Err(e) = app.run("view.separations", p)
    {
        app.status_error("Separations Preview", e);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::Value;

    struct State {
        app: DesignApp,
        requests: Vec<Value>,
    }

    fn harness(language: &str) -> Harness<'static, State> {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.ui.language = language.to_owned();
        app.framework_opacity = true;
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, language);
        app.install_framework(&ctx);
        assert!(app.framework_installed);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(420.0, 140.0)).build_ui_state(
            |ui, state: &mut State| {
                let sep = state.app.ui.separations.clone();
                let lang = state.app.ui.language.clone();
                if let Some(params) =
                    mode_actions(ui, &lang, state.app.framework_installed, sep.is_some(), sep.as_ref().and_then(|sep| sep.ink_limit))
                {
                    state.requests.push(params.clone());
                    state.app.run("view.separations", params).unwrap();
                }
            },
            State { app, requests: Vec::new() },
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn steps(h: &mut Harness<'_, State>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn event(h: &mut Harness<'_, State>, event: Event) {
        h.input_mut().events.push(event);
        steps(h, 1);
    }

    fn key(h: &mut Harness<'_, State>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        steps(h, 2);
    }

    fn button_rect(h: &Harness<'_, State>, label: &str) -> egui::Rect {
        let name = tr(&h.state().app.ui.language, label);
        let buttons: Vec<_> = h.query_all_by(|node| node.role() == egui::accesskit::Role::Button && node.label().as_deref() == Some(name)).collect();
        assert_eq!(buttons.len(), 1, "the translated bare mode name identifies one actual Button");
        buttons[0].rect()
    }

    fn assert_focused(h: &Harness<'_, State>, label: &str) {
        let name = tr(&h.state().app.ui.language, label);
        assert!(h.get_by(|node| node.role() == egui::accesskit::Role::Button && node.label().as_deref() == Some(name)).is_focused());
    }

    fn click(h: &mut Harness<'_, State>, label: &str) {
        let pos = button_rect(h, label).center();
        event(h, Event::PointerMoved(pos));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        steps(h, 2);
    }

    fn assert_requests(h: &Harness<'_, State>) {
        assert_eq!(
            h.state().requests,
            [json!({"plate": "all", "inkLimit": null}), json!({"inkLimit": 300.0}), json!({"plate": null, "inkLimit": null})],
            "one request per actual mode change, with no selected-mode or empty-pass replay"
        );
    }

    #[test]
    fn installed_mode_pointer_sequence_preserves_view_only_state_and_ignores_repeated_picks() {
        let mut h = harness("hu");
        let doc = serde_json::to_value(h.state().app.session.active().unwrap().doc.as_ref()).unwrap();
        let undo = h.state().app.session.active().unwrap().history.undo.len();
        let journal = h.state().app.session.journal.len();
        assert_eq!(h.query_all_by(|node| node.role() == egui::accesskit::Role::Button).count(), 3);
        assert!(h.state().app.ui.separations.is_none());
        click(&mut h, "Off");
        assert!(h.state().requests.is_empty());
        click(&mut h, "Separations");
        assert_eq!(h.state().app.ui.separations, Some(crate::separations::Separations { plate: None, ink_limit: None }));
        click(&mut h, "Separations");
        assert_eq!(h.state().requests.len(), 1);
        click(&mut h, "Ink Limit");
        assert_eq!(h.state().app.ui.separations, Some(crate::separations::Separations { plate: None, ink_limit: Some(300.0) }));
        click(&mut h, "Ink Limit");
        assert_eq!(h.state().requests.len(), 2);
        click(&mut h, "Off");
        assert!(h.state().app.ui.separations.is_none());
        steps(&mut h, 4);
        assert_requests(&h);
        assert_eq!(serde_json::to_value(h.state().app.session.active().unwrap().doc.as_ref()).unwrap(), doc, "view modes never mutate the document");
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), undo);
        assert_eq!(h.state().app.session.journal.len(), journal, "UI-only view commands do not enter the engine journal");
    }

    #[test]
    fn installed_mode_tab_space_enter_sequence_changes_modes_without_document_history() {
        let mut h = harness("");
        let doc = serde_json::to_value(h.state().app.session.active().unwrap().doc.as_ref()).unwrap();
        let undo = h.state().app.session.active().unwrap().history.undo.len();
        key(&mut h, Key::Tab, Modifiers::NONE);
        assert_focused(&h, "Off");
        key(&mut h, Key::Space, Modifiers::NONE);
        assert!(h.state().requests.is_empty(), "Space on the selected Off action does not send a command");
        key(&mut h, Key::Tab, Modifiers::NONE);
        assert_focused(&h, "Separations");
        key(&mut h, Key::Space, Modifiers::NONE);
        assert_eq!(h.state().app.ui.separations.as_ref().unwrap().ink_limit, None);
        key(&mut h, Key::Space, Modifiers::NONE);
        assert_eq!(h.state().requests.len(), 1);
        key(&mut h, Key::Tab, Modifiers::NONE);
        assert_focused(&h, "Ink Limit");
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(h.state().app.ui.separations.as_ref().unwrap().ink_limit, Some(300.0));
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(h.state().requests.len(), 2);
        key(&mut h, Key::Tab, Modifiers::SHIFT);
        key(&mut h, Key::Tab, Modifiers::SHIFT);
        assert_focused(&h, "Off");
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert!(h.state().app.ui.separations.is_none());
        steps(&mut h, 4);
        assert_requests(&h);
        assert_eq!(serde_json::to_value(h.state().app.session.active().unwrap().doc.as_ref()).unwrap(), doc);
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), undo);
    }
}

// SEPARATIONS_PLATE_TESTS_BEGIN
#[cfg(all(test, not(target_arch = "wasm32")))]
mod plate_action_tests {
    use super::*;
    use designcraft_doc::{Document, Selection};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use serde_json::Value;
    use std::sync::Arc;
    type History = Vec<(String, Arc<Document>, Selection)>;
    struct Quiet {
        document: Arc<Document>,
        saved: Arc<Document>,
        undo: History,
        redo: History,
        revision: u64,
        saved_revision: u64,
        epoch: u64,
        uid: u64,
        dirty: bool,
        selection: Selection,
        limit: usize,
        journal: Vec<(String, Value)>,
    }
    fn quiet(app: &DesignApp) -> Quiet {
        let st = app.session.active().unwrap();
        let history = |entries: &[designcraft_engine::HistoryEntry]| -> History {
            entries.iter().map(|entry| (entry.label.clone(), entry.doc.clone(), entry.selection.clone())).collect()
        };
        Quiet {
            document: st.doc.clone(),
            saved: st.saved_doc.clone(),
            undo: history(&st.history.undo),
            redo: history(&st.history.redo),
            revision: st.revision,
            saved_revision: st.saved_revision,
            epoch: st.identity_epoch,
            uid: st.uid,
            dirty: st.is_dirty(),
            selection: st.selection.clone(),
            limit: st.history.limit,
            journal: app.session.journal.clone(),
        }
    }
    fn assert_quiet(app: &DesignApp, before: &Quiet) {
        let now = quiet(app);
        assert!(Arc::ptr_eq(&now.document, &before.document));
        assert_eq!(now.document.as_ref(), before.document.as_ref());
        assert!(Arc::ptr_eq(&now.saved, &before.saved));
        assert_eq!(now.undo, before.undo);
        assert_eq!(now.redo, before.redo);
        assert_eq!(
            (now.revision, now.saved_revision, now.epoch, now.uid, now.dirty, now.limit),
            (before.revision, before.saved_revision, before.epoch, before.uid, before.dirty, before.limit)
        );
        assert_eq!(now.selection, before.selection);
        assert_eq!(now.journal, before.journal);
    }
    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        if installed {
            app.framework_opacity = true;
            app.install_framework(&ctx);
            assert!(app.framework_installed);
        }
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(680.0, 780.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                let _ = craft_ui_widgets::runtime::inspect::record(ui, egui::Id::NULL.with("separations_plate_test_region"), |ui| show(app, ui));
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 3);
        h
    }
    fn frame(h: &mut Harness<'_, DesignApp>, event: Option<Event>) {
        let mut raw = std::mem::take(h.input_mut());
        if let Some(event) = event {
            raw.events.push(event);
        }
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, DesignApp>, count: usize) {
        for _ in 0..count {
            frame(h, None);
        }
    }
    fn key_event(h: &mut Harness<'_, DesignApp>, key: Key, pressed: bool) {
        frame(h, Some(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }));
    }
    fn key(h: &mut Harness<'_, DesignApp>, key: Key) {
        key_event(h, key, true);
        key_event(h, key, false);
        idle(h, 2);
    }
    fn pointer(h: &mut Harness<'_, DesignApp>, pos: egui::Pos2, pressed: bool) {
        frame(h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
    }
    fn click_at(h: &mut Harness<'_, DesignApp>, pos: egui::Pos2) {
        frame(h, Some(Event::PointerMoved(pos)));
        pointer(h, pos, true);
        pointer(h, pos, false);
        idle(h, 2);
    }
    fn click(h: &mut Harness<'_, DesignApp>, role: egui::accesskit::Role, label: &str) {
        let pos = h.get_by_role_and_label(role, label).rect().center();
        click_at(h, pos);
    }
    fn tab_to(h: &mut Harness<'_, DesignApp>, role: egui::accesskit::Role, label: &str) {
        let stops = h
            .query_all_by(|n| {
                matches!(
                    n.role(),
                    egui::accesskit::Role::Button
                        | egui::accesskit::Role::ComboBox
                        | egui::accesskit::Role::TextInput
                        | egui::accesskit::Role::CheckBox
                        | egui::accesskit::Role::RadioButton
                        | egui::accesskit::Role::SpinButton
                        | egui::accesskit::Role::Slider
                )
            })
            .count();
        for _ in 0..=stops {
            if h.get_by_role_and_label(role, label).is_focused() {
                let target = h.get_by_role_and_label(role, label).accesskit_node().id();
                let focus: Vec<_> = h.query_all_by(|node| node.is_focused()).map(|node| node.accesskit_node().id()).collect();
                assert_eq!(focus, vec![target], "exactly one actual AX focus stop before activation");
                return;
            }
            key(h, Key::Tab);
        }
        panic!("natural Tab did not reach exact {label} within the rendered focus cycle");
    }
    fn sdk(h: &Harness<'_, DesignApp>, component: &str, role: egui::accesskit::Role, label: &str) -> craft_ui_widgets::runtime::inspect::Element {
        let recorded = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with("separations_plate_test_region")).unwrap();
        let matches: Vec<_> = recorded
            .elements
            .iter()
            .filter(|e| e.component == component && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(label))
            .collect();
        assert_eq!(matches.len(), 1, "one real SDK {component} with complete action name");
        assert_eq!(h.get_all_by_role_and_label(role, label).count(), 1);
        assert!(!matches[0].facts.as_ref().unwrap().disabled);
        matches[0].clone()
    }

    const SPOT: &str = "Gold — complete second spot";
    fn fixture() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 2, "facing": false})).unwrap();
        for (name, rect) in [("Gold", [72, 72, 172, 172]), (SPOT, [220, 72, 320, 172]), ("All Inks", [370, 72, 470, 172])] {
            app.run("swatch.create", json!({"name": name, "spot": true, "color": {"c": 0, "m": 20, "y": 80, "k": 10}})).unwrap();
            let id = app.run("frame.create", json!({"spread": 0, "rect": rect})).unwrap()["id"].clone();
            app.run("object.fill", json!({"swatch": name, "ids": [id]})).unwrap();
        }
        for page in [0, 1] {
            let id = app.run("frame.create", json!({"spread": page, "rect": [72, 240, 172, 340]})).unwrap()["id"].clone();
            app.run("object.fill", json!({"swatch": "C=100 M=0 Y=0 K=0", "ids": [id]})).unwrap();
        }
        centre(&mut app);
        let result = app.run("view.separations", json!({"plate": "all", "inkLimit": null})).unwrap();
        assert!(result.get("error").is_none(), "the genuine print-PDF separation must succeed: {result}");
        assert_eq!(result["spread"], 0);
        let sheet = real_sheet(&app);
        assert!(sheet.ink("Cyan").is_some() && sheet.ink("Gold").is_some() && sheet.ink(SPOT).is_some() && sheet.ink("All Inks").is_some());
        assert!(tint(&sheet, "Gold", 122.0, 122.0) > 245, "the real spot plane actually paints its source square");
        assert_eq!(tint(&sheet, "Gold", 122.0, 290.0), 0, "the real spot plane does not paint the cyan square");
        assert!(tint(&sheet, "Cyan", 122.0, 290.0) > 245);
        app.run("view.nextPage", json!({})).unwrap();
        let second = app.run("view.separations", json!({"plate": "all", "inkLimit": null})).unwrap();
        assert_eq!(second["spread"], 1);
        let second_sheet = real_sheet(&app);
        assert!(second_sheet.ink("Gold").is_none(), "the other real spread has no spot objects");
        assert!(tint(&second_sheet, "Cyan", 122.0, 290.0) > 245, "Cyan really paints the second spread too");
        centre(&mut app);
        app.run("view.separations", json!({"plate": "all", "inkLimit": null})).unwrap();
        app
    }
    fn centre(app: &mut DesignApp) {
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0)));
        app.view_mut().unwrap().zoom = 0.125;
        crate::canvas::go_to_page(app, 0);
    }
    fn real_sheet(app: &DesignApp) -> Arc<designcraft_engine::separations::Sheet> {
        app.canvas
            .separations
            .sheet(
                crate::canvas::current_spread(app).and_then(|s| if let designcraft_doc::SpreadRef::Doc(si) = s { Some(si) } else { None }).unwrap(),
            )
            .unwrap()
            .as_ref()
            .unwrap()
            .clone()
    }
    fn tint(sheet: &designcraft_engine::separations::Sheet, ink: &str, x: f64, y: f64) -> u8 {
        let scale = f64::from(sheet.plates.dpi) / 72.0;
        sheet.plates.tint(sheet.ink(ink).unwrap(), ((x - sheet.area.x0) * scale) as u32, ((y - sheet.area.y0) * scale) as u32).unwrap()
    }
    fn plate_name(app: &DesignApp, ink: &str) -> String {
        format!("{}: {ink}", tr(&app.ui.language, "Plate"))
    }
    fn assert_view(app: &DesignApp, plate: Option<&str>, limit: Option<f32>) {
        let actual = app.ui.separations.as_ref().expect("preview remains on");
        assert_eq!((actual.plate.as_deref(), actual.ink_limit), (plate, limit));
        if let Some(name) = plate {
            assert!(real_sheet(app).ink(name).is_some(), "the selected name denotes a genuine displayed plate");
        }
    }
    fn change_owner(app: &mut DesignApp, change: &str) {
        match change {
            "switch" => {
                app.run("file.new", json!({"pages": 2, "facing": false})).unwrap();
                centre(app);
            }
            "replace" => {
                let old = app.session.active().unwrap();
                let uid = old.uid;
                let epoch = old.identity_epoch;
                let mut replacement = designcraft_engine::DocState::new(old.doc.as_ref().clone(), None);
                replacement.uid = uid;
                assert_ne!(replacement.identity_epoch, epoch);
                *app.session.active_mut().unwrap() = replacement;
                centre(app);
            }
            "page" => {
                app.run("view.nextPage", json!({})).unwrap();
                assert_eq!(crate::canvas::current_spread(app), Some(designcraft_doc::SpreadRef::Doc(1)));
            }
            "sheet" => {
                app.ui.pdf_export.compress_images = !app.ui.pdf_export.compress_images;
            }
            "alias" => {
                app.run("ink.options", json!({"ink": "Gold", "alias": SPOT})).unwrap();
            }
            "close" => {
                app.session.execute("file.close", &json!({"force": true})).unwrap();
                assert!(app.session.active().is_none());
                return;
            }
            "off" => {
                app.run("view.separations", json!({"plate": null, "inkLimit": null})).unwrap();
                assert!(app.ui.separations.is_none());
                return;
            }
            _ => unreachable!(),
        }
        let result = app.run("view.separations", json!({"plate": "all", "inkLimit": null})).unwrap();
        assert!(result.get("error").is_none(), "the replacement target must use a genuine computed sheet: {result}");
    }

    #[test]
    fn ui_separations_real_process_and_spot_plate_pointer_actions_toggle_and_keep_document_history_saved_state_unchanged() {
        let mut h = harness(fixture(), true);
        let before = quiet(h.state());
        for ink in ["Cyan", "Gold", SPOT, "All Inks"] {
            let label = plate_name(h.state(), ink);
            sdk(&h, "Button", egui::accesskit::Role::Button, &label);
            click(&mut h, egui::accesskit::Role::Button, &label);
            assert_view(h.state(), Some(ink), None);
            assert_quiet(h.state(), &before);
            click(&mut h, egui::accesskit::Role::Button, &label);
            assert_view(h.state(), None, None);
            assert_quiet(h.state(), &before);
        }
        sdk(&h, "Button", egui::accesskit::Role::Button, "All Inks");
        for _ in 0..2 {
            click(&mut h, egui::accesskit::Role::Button, "All Inks");
            assert_view(h.state(), None, None);
        }
        assert_eq!(
            h.get_all_by_role_and_label(egui::accesskit::Role::Button, "All Inks").count(),
            1,
            "composite and an actual same-named spot have distinct full action names"
        );
        assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::Button, "Plate: All Inks").count(), 1);
        idle(&mut h, 3);
        assert_quiet(h.state(), &before);
    }
    #[test]
    fn ui_separations_plate_natural_tab_space_enter_uses_full_names_and_preserves_actual_ink_limit() {
        let mut app = fixture();
        app.run("view.separations", json!({"inkLimit": 275})).unwrap();
        let mut h = harness(app, true);
        let before = quiet(h.state());
        for (ink, key_value) in [(SPOT, Key::Space), ("Cyan", Key::Enter)] {
            let label = plate_name(h.state(), ink);
            tab_to(&mut h, egui::accesskit::Role::Button, &label);
            assert_quiet(h.state(), &before);
            key(&mut h, key_value);
            assert_view(h.state(), Some(ink), Some(275.0));
            assert_quiet(h.state(), &before);
            tab_to(&mut h, egui::accesskit::Role::Button, &label);
            key(&mut h, key_value);
            assert_view(h.state(), None, Some(275.0));
            assert_quiet(h.state(), &before);
        }
        let all = tr(&h.state().ui.language, "All Inks").to_owned();
        tab_to(&mut h, egui::accesskit::Role::Button, &all);
        key(&mut h, Key::Enter);
        assert_view(h.state(), None, Some(275.0));
        assert_quiet(h.state(), &before);
    }
    #[test]
    fn ui_separations_held_plate_pointer_drops_changed_document_spread_sheet_alias_or_absence_and_fresh_input_recovers() {
        for change in ["switch", "replace", "page", "sheet", "alias", "close", "off"] {
            let mut h = harness(fixture(), true);
            let label = plate_name(h.state(), "Cyan");
            let pos = h.get_by_role_and_label(egui::accesskit::Role::Button, &label).rect().center();
            let old_sheet = real_sheet(h.state());
            frame(&mut h, Some(Event::PointerMoved(pos)));
            pointer(&mut h, pos, true);
            change_owner(h.state_mut(), change);
            if change == "sheet" {
                assert!(
                    !Arc::ptr_eq(&old_sheet, &real_sheet(h.state())),
                    "the changed print setting really recomputed the cached result without a fake sheet"
                );
            }
            let before = h.state().session.active().map(|_| quiet(h.state()));
            let view = h.state().ui.separations.clone();
            idle(&mut h, 2);
            pointer(&mut h, pos, false);
            idle(&mut h, 2);
            assert_eq!(h.state().ui.separations, view, "old sheet/owner press cannot retarget after {change}");
            if let Some(before) = &before {
                assert_quiet(h.state(), before);
            }
            if h.state().session.active().is_none() {
                h.state_mut().run("file.new", json!({})).unwrap();
                centre(h.state_mut());
            }
            h.state_mut().run("view.separations", json!({"plate": "all", "inkLimit": null})).unwrap();
            idle(&mut h, 2);
            let fresh = quiet(h.state());
            let label = plate_name(h.state(), "Cyan");
            click(&mut h, egui::accesskit::Role::Button, &label);
            assert_view(h.state(), Some("Cyan"), None);
            assert_quiet(h.state(), &fresh);
        }
    }
    #[test]
    fn ui_separations_held_space_cannot_activate_replaced_plate_owner_and_fresh_natural_keyboard_recovers() {
        for change in ["switch", "replace", "page", "sheet"] {
            let mut h = harness(fixture(), true);
            let label = plate_name(h.state(), "Cyan");
            tab_to(&mut h, egui::accesskit::Role::Button, &label);
            key_event(&mut h, Key::Space, true);
            change_owner(h.state_mut(), change);
            let before = quiet(h.state());
            idle(&mut h, 2);
            key_event(&mut h, Key::Space, false);
            idle(&mut h, 2);
            assert_view(h.state(), None, None);
            assert_quiet(h.state(), &before);
            let label = plate_name(h.state(), "Cyan");
            tab_to(&mut h, egui::accesskit::Role::Button, &label);
            key(&mut h, Key::Space);
            assert_view(h.state(), Some("Cyan"), None);
            assert_quiet(h.state(), &before);
        }
    }
    #[test]
    fn ui_separations_plate_off_no_document_and_uninstalled_fallback_remain_real_and_nonediting() {
        let mut empty = harness(DesignApp::new(designcraft_engine::Session::new(), crate::Services::default()), true);
        assert!(empty.state().session.active().is_none());
        assert_eq!(empty.query_all_by(|node| node.role() == egui::accesskit::Role::Button && node.label().as_deref() == Some("All Inks")).count(), 0);
        let mut app = fixture();
        app.run("view.separations", json!({"plate": null, "inkLimit": null})).unwrap();
        let mut off = harness(app, true);
        let before = quiet(off.state());
        assert_eq!(
            off.query_all_by(|node| node.role() == egui::accesskit::Role::Button && node.label().as_deref() == Some("Plate: Cyan")).count(),
            0
        );
        idle(&mut off, 2);
        assert_quiet(off.state(), &before);
        let mut h = harness(fixture(), false);
        assert!(!h.state().framework_installed);
        let before = quiet(h.state());
        let label = plate_name(h.state(), SPOT);
        click(&mut h, egui::accesskit::Role::Button, &label);
        assert_view(h.state(), Some(SPOT), None);
        assert_quiet(h.state(), &before);
        click(&mut h, egui::accesskit::Role::Button, "All Inks");
        assert_view(h.state(), None, None);
        assert_quiet(h.state(), &before);
    }
    #[test]
    fn ui_separations_localized_plate_action_keeps_complete_actual_ink_name_and_unedited_domain() {
        let mut app = fixture();
        app.ui.language = "hu".to_owned();
        let mut h = harness(app, true);
        let before = quiet(h.state());
        let label = plate_name(h.state(), SPOT);
        assert!(label.ends_with(SPOT));
        sdk(&h, "Button", egui::accesskit::Role::Button, &label);
        tab_to(&mut h, egui::accesskit::Role::Button, &label);
        key(&mut h, Key::Enter);
        assert_view(h.state(), Some(SPOT), None);
        assert_quiet(h.state(), &before);
        let all = tr(&h.state().ui.language, "All Inks").to_owned();
        click(&mut h, egui::accesskit::Role::Button, &all);
        assert_view(h.state(), None, None);
        assert_quiet(h.state(), &before);
    }

    #[test]
    fn ui_separations_whole_app_parent_opens_real_preview_and_selects_actual_spot_without_document_edit() {
        let mut app = fixture();
        app.framework_opacity = true;
        let mut h = crate::test_window::open(app, egui::vec2(1400.0, 1000.0));
        let ctx = h.ctx.clone();
        h.state_mut().app.install_framework(&ctx);
        assert!(h.state().app.framework_installed);
        h.state_mut().app.run("window.panel", json!({"panel": "separations"})).unwrap();
        h.state_mut().app.run("view.firstPage", json!({})).unwrap();
        h.state_mut().app.run("view.separations", json!({"plate": "all", "inkLimit": null})).unwrap();
        fn step(h: &mut Harness<'_, crate::test_window::Window>, event: Option<Event>) {
            let mut raw = std::mem::take(h.input_mut());
            if let Some(event) = event {
                raw.events.push(event);
            }
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
        for _ in 0..3 {
            step(&mut h, None);
        }
        let label = plate_name(&h.state().app, SPOT);
        assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::Button, &label).count(), 1);
        let before = quiet(&h.state().app);
        let pos = h.get_by_role_and_label(egui::accesskit::Role::Button, &label).rect().center();
        step(&mut h, Some(Event::PointerMoved(pos)));
        for pressed in [true, false] {
            step(&mut h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
        }
        for _ in 0..2 {
            step(&mut h, None);
        }
        assert_view(&h.state().app, Some(SPOT), None);
        assert_quiet(&h.state().app, &before);
    }
}
// SEPARATIONS_PLATE_TESTS_END
