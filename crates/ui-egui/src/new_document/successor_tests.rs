//! Successors of the native New Document tests of `new_document/tests.rs` that check the preview window's life: they keep the
//! old bodies and drive the same steps through the framework's New Document form, in the real dialog host over an installed
//! framework (`Rig` dereferences to the app, so a test reads `app.session` and `app.ui.dialog` as the old ones did).

use super::*;
use crate::dialogs::table_layout_kit as kit;
use std::ops::{Deref, DerefMut};

/// The app in a harness showing its dialogs through the framework host.
struct Rig(egui_kittest::Harness<'static, DesignApp>);

impl Deref for Rig {
    type Target = DesignApp;
    fn deref(&self) -> &DesignApp {
        self.0.state()
    }
}

impl DerefMut for Rig {
    fn deref_mut(&mut self) -> &mut DesignApp {
        self.0.state_mut()
    }
}

/// The app with New Document open, shown by the framework host in a 1440 x 900 window.
fn rig(app: DesignApp) -> (Rig, egui::Context) {
    let h = kit::harness_sized(app, vec2(1440.0, 900.0));
    let ctx = h.ctx.clone();
    (Rig(h), ctx)
}

/// The app with no document and New Document open.
fn app() -> DesignApp {
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".to_owned();
    app.run("app.newDocumentDialog", json!({})).unwrap();
    app
}

fn two_documents() -> (Rig, egui::Context, designcraft_engine::DocState) {
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".to_owned();
    app.run("file.new", json!({"title": "A"})).unwrap();
    app.run("file.new", json!({"title": "B"})).unwrap();
    app.session.set_active(0);
    let a = app.session.active().unwrap().clone();
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let (app, ctx) = rig(app);
    (app, ctx, a)
}

/// A frame of the open dialog (the harness's frames are longer than the preview's settling time).
fn frame(app: &mut Rig, _ctx: &egui::Context, _events: Vec<egui::Event>) {
    kit::steps(&mut app.0, 1);
}

/// Sets draft fields and lets the preview settle.
fn set(app: &mut Rig, _ctx: &egui::Context, fields: &[(&str, Value)]) {
    let d = app.ui.dialog.as_mut().unwrap();
    for (k, v) in fields {
        d.fields.insert((*k).into(), v.clone());
    }
    kit::steps(&mut app.0, 3);
}

fn escape(app: &mut Rig, _ctx: &egui::Context) {
    kit::key(&mut app.0, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(app.ui.dialog.is_none());
}

/// Successor of `the_update_loop_ends_a_preview_whose_window_has_gone`.
#[test]
fn the_update_loop_ends_a_preview_whose_window_has_gone_in_the_framework_form() {
    let (mut app, ctx, a) = two_documents();
    set(&mut app, &ctx, &[("preview", json!(true))]);
    assert_eq!(app.session.documents().len(), 3);
    app.ui.dialog = None;
    let mut out = ctx.run_ui(egui::RawInput { time: Some(ctx.input(|i| i.time) + 1.0), ..Default::default() }, |ui| app.logic(ui.ctx()));
    out.textures_delta.clear();
    assert_eq!((app.session.documents().len(), app.session.active().unwrap().uid), (2, a.uid));
}
/// Successor of `closing_leaves_a_document_activated_or_edited_meanwhile`.
#[test]
fn closing_leaves_a_document_activated_or_edited_meanwhile_in_the_framework_form() {
    let (mut app, ctx, _) = two_documents();
    set(&mut app, &ctx, &[("preview", json!(true))]);
    // A file opened (here: made) by a script while the window is open stays active.
    app.run("file.new", json!({"title": "Opened"})).unwrap();
    escape(&mut app, &ctx);
    let titles: Vec<String> = app.session.documents().iter().map(|d| d.title()).collect();
    assert_eq!((titles, app.session.active().unwrap().title()), (vec!["A".into(), "B".into(), "Opened".into()], "Opened".into()));
    // A preview edited by a menu or script is kept as a document, with its Untitled number.
    app.run("app.newDocumentDialog", json!({})).unwrap();
    frame(&mut app, &ctx, vec![]);
    let preview = app.session.new_document_preview();
    app.run("frame.create", json!({"rect": [10, 10, 100, 100]})).unwrap();
    escape(&mut app, &ctx);
    let st = app.session.active().unwrap();
    assert_eq!((app.session.documents().len(), Some(st.uid), st.is_dirty()), (4, preview, true));
    assert_ne!(st.doc.title, app.session.next_untitled_title());
}
/// Successor of `closing_the_start_screen_preview_leaves_no_document`.
#[test]
fn closing_the_start_screen_preview_leaves_no_document_in_the_framework_form() {
    let (mut app, ctx) = rig(app());
    set(&mut app, &ctx, &[("preview", json!(true))]);
    assert_eq!(app.session.documents().len(), 1);
    escape(&mut app, &ctx);
    assert!(app.session.documents().is_empty() && app.session.active().is_none());
}
/// Successor of `create_keeps_the_previewed_document_with_the_final_settings`.
#[test]
fn create_keeps_the_previewed_document_with_the_final_settings_in_the_framework_form() {
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".to_owned();
    app.run("file.new", json!({"title": "A"})).unwrap();
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let (mut app, ctx) = rig(app);
    set(&mut app, &ctx, &[("preview", json!(true)), ("width", json!("400 pt"))]);
    set(&mut app, &ctx, &[("pages", json!("4")), ("facingPages", json!(false))]);
    let preview = app.session.new_document_preview();
    // An invalid field refuses Create: the window and the preview stay.
    set(&mut app, &ctx, &[("pages", json!(""))]);
    assert!(crate::dialogs::confirm(&mut app).is_err());
    assert!(app.ui.dialog.as_ref().unwrap().s("status").contains("Invalid value"));
    assert_eq!((app.session.documents().len(), app.session.new_document_preview()), (2, preview));
    // A size file.new refuses also keeps them.
    set(&mut app, &ctx, &[("pages", json!("4")), ("width", json!("0"))]);
    assert!(crate::dialogs::confirm(&mut app).is_err());
    assert_eq!((app.session.documents().len(), app.session.new_document_preview()), (2, preview));
    // With the previewed settings, the preview becomes the document.
    set(&mut app, &ctx, &[("width", json!("400 pt"))]);
    crate::dialogs::confirm(&mut app).unwrap();
    let st = app.session.active().unwrap();
    assert_eq!((app.session.documents().len(), st.doc.settings.page_width, st.doc.page_count()), (2, 400.0, 4));
    assert!(!st.doc.settings.facing_pages && app.ui.dialog.is_none());
    assert_eq!((Some(st.uid), st.doc.title.as_str(), st.history.undo.len(), st.is_dirty()), (preview, "Untitled-2", 0, false));
    assert_eq!(app.session.new_document_preview(), None);
    // Settings changed just before Create (the preview not made yet) make a new document.
    app.run("app.newDocumentDialog", json!({})).unwrap();
    set(&mut app, &ctx, &[]);
    app.ui.dialog.as_mut().unwrap().fields.insert("pages".into(), json!("2"));
    frame(&mut app, &ctx, vec![]);
    crate::dialogs::confirm(&mut app).unwrap();
    let st = app.session.active().unwrap();
    assert_eq!((app.session.documents().len(), st.doc.page_count(), st.doc.title.as_str()), (3, 2, "Untitled-3"));
}
