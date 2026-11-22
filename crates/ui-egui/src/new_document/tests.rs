use super::*;

fn app() -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("app.newDocumentDialog", json!({})).unwrap();
    app
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx, "");
    crate::theme::apply(&ctx, &Tokens::for_brightness(crate::theme::Brightness::default()));
    ctx
}

/// One frame of the open dialog → the painted texts and where.
fn frame(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<(String, Rect)> {
    frame_after(app, ctx, events, 1.0 / 60.0)
}

/// One frame `dt` seconds after the last.
fn frame_after(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>, dt: f64) -> Vec<(String, Rect)> {
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 900.0))),
        time: Some(ctx.input(|i| i.time) + dt),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| crate::dialogs::show(app, ui.ctx()));
    out.textures_delta.clear();
    fn collect(shape: &egui::Shape, texts: &mut Vec<(String, Rect)>) {
        match shape {
            egui::Shape::Text(t) => texts.push((t.galley.job.text.clone(), t.visual_bounding_rect())),
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, texts)),
            _ => {}
        }
    }
    let mut texts = Vec::new();
    out.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
    texts
}

fn settle(app: &mut DesignApp, ctx: &egui::Context) -> Vec<(String, Rect)> {
    for _ in 0..3 {
        frame(app, ctx, vec![]);
    }
    frame(app, ctx, vec![])
}

fn click(app: &mut DesignApp, ctx: &egui::Context, pos: egui::Pos2, times: usize) {
    frame(app, ctx, vec![egui::Event::PointerMoved(pos)]);
    for _ in 0..times {
        for pressed in [true, false] {
            frame(
                app,
                ctx,
                vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }],
            );
        }
    }
}

/// The card whose name is `name` (its page, above the name).
fn card_at(texts: &[(String, Rect)], name: &str) -> egui::Pos2 {
    let (_, r) = texts.iter().find(|(t, _)| t == name).unwrap_or_else(|| panic!("card {name}"));
    r.center() - vec2(0.0, 50.0)
}

#[test]
fn a_print_preset_fills_the_details_and_creates_its_document() {
    let mut app = app();
    let ctx = context();
    app.ui.dialog.as_mut().unwrap().fields.insert("tab".into(), json!("print"));
    let texts = settle(&mut app, &ctx);
    // A4 sits below the poster sizes: scroll the presets to it.
    let over = card_at(&texts, "Letter");
    frame(&mut app, &ctx, vec![egui::Event::PointerMoved(over)]);
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -400.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
        source: egui::MouseWheelSource::Unknown,
    };
    frame(&mut app, &ctx, vec![wheel]);
    let texts = settle(&mut app, &ctx);
    click(&mut app, &ctx, card_at(&texts, "A4"), 1);
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!((d.s("preset"), d.s("width"), d.s("height"), units(d)), ("A4".into(), "210 mm".into(), "297 mm".into(), Unit::Millimeters));
    let d = app.ui.dialog.as_mut().unwrap();
    for (k, v) in [("startPage", json!(3)), ("bleedTop", json!("3 mm")), ("slugBottom", json!("10 mm")), ("name", json!("Flyer"))] {
        d.fields.insert(k.into(), v);
    }
    crate::dialogs::confirm(&mut app).unwrap();
    let doc = &app.session.active().unwrap().doc;
    assert_eq!(doc.title, "Flyer");
    assert!((doc.settings.page_width - 210.0 * PT_PER_MM).abs() < 1e-6);
    assert_eq!(doc.settings.horizontal_units, Unit::Millimeters);
    assert_eq!(doc.page_name(0), "3");
    assert!((doc.settings.bleed[0] - 3.0 * PT_PER_MM).abs() < 1e-6 && doc.settings.bleed[1] == 0.0);
    assert!((doc.settings.slug[1] - 10.0 * PT_PER_MM).abs() < 1e-6);
    // The Recent tab now shows it, by its preset's name.
    let recent = cards(&app, "recent");
    assert_eq!(recent.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), [DEFAULT_CARD, "A4"]);
    assert_eq!(recent[1].nd.start_page, 3);
}

#[test]
fn a_tab_shows_its_first_preset_as_indesign_does() {
    let mut app = app();
    let ctx = context();
    // A tab's name, a little above its painted text's box (which reaches below the strip).
    let tab = |texts: &[(String, Rect)], name: &str| texts.iter().find(|(t, _)| t == name).map(|(_, r)| r.center() - vec2(0.0, 12.0)).unwrap();
    let texts = settle(&mut app, &ctx);
    app.ui.dialog.as_mut().unwrap().fields.insert("name".into(), json!("Poster"));
    click(&mut app, &ctx, tab(&texts, "Web"), 1);
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!((d.s("tab"), d.s("preset")), ("web".into(), "Web 1920 \u{d7} 1080".into()));
    assert_eq!((d.s("width"), d.s("height"), units(d)), ("1920 px".into(), "1080 px".into(), Unit::Pixels), "Web is in pixels");
    assert!(!d.b("facingPages") && !d.b("primaryTextFrame"));
    assert_eq!(d.s("name"), "Poster", "the typed name stays");
    // Mobile is in pixels too, without a primary text frame (its autoflow is an open gap).
    let texts = settle(&mut app, &ctx);
    click(&mut app, &ctx, tab(&texts, "Mobile"), 1);
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!((d.s("tab"), units(d), d.b("primaryTextFrame")), ("mobile".into(), Unit::Pixels, false));
    let texts = settle(&mut app, &ctx);
    click(&mut app, &ctx, tab(&texts, "Print"), 1);
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!((d.s("preset"), units(d)), ("Letter".into(), Unit::Picas));
    // Clicking the open tab again keeps what was picked on it.
    let texts = settle(&mut app, &ctx);
    click(&mut app, &ctx, card_at(&texts, "A4"), 1);
    click(&mut app, &ctx, tab(&texts, "Print"), 1);
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("preset"), "A4");
    // Saved starts with [Default], as in InDesign, and selects it.
    let texts = settle(&mut app, &ctx);
    click(&mut app, &ctx, tab(&texts, "Saved"), 1);
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!((d.s("preset"), d.s("width")), (DEFAULT_CARD.into(), "51p0".into()));
    let texts = settle(&mut app, &ctx);
    assert!(texts.iter().any(|(t, _)| t == "SAVED PRESETS (1)"));
}

#[test]
fn double_clicking_a_card_creates_the_document() {
    let mut app = app();
    let ctx = context();
    app.ui.dialog.as_mut().unwrap().fields.insert("tab".into(), json!("web"));
    let texts = settle(&mut app, &ctx);
    click(&mut app, &ctx, card_at(&texts, "Web 1366 \u{d7} 768"), 2);
    assert!(app.ui.dialog.is_none(), "the dialog closed");
    let doc = &app.session.active().unwrap().doc;
    assert_eq!((doc.settings.page_width, doc.settings.intent), (1366.0, Intent::Web));
    assert!(!doc.settings.facing_pages);
}

#[test]
fn card_sizes_follow_the_dialog_units() {
    let card = NewDocument::from_preset("Business Card").unwrap();
    assert_eq!(size_text(&card, Unit::Millimeters), "88.9 × 50.8 mm");
    assert_ne!(size_text(&card, Unit::Millimeters), size_text(&card, Unit::Picas));
}

#[test]
fn units_reformat_the_measures_and_linked_edges_follow() {
    // InDesign's Units list.
    assert_eq!(NEW_DOCUMENT_UNITS.map(|u| u.label()), ["Points", "Picas", "Inches", "Millimeters", "Centimeters", "Ciceros", "Agates", "Pixels"]);
    let mut d = Dialog::new("newDocument", json!({}));
    set_units(&mut d, Unit::Millimeters);
    assert_eq!((d.s("width"), d.s("marginTop")), ("215.9 mm".into(), "12.7 mm".into()));
    d.fields.insert("marginInside".into(), json!("20 mm"));
    link_edges(&mut d, "margin", "marginInside");
    assert!(EDGES.iter().all(|e| d.s(&format!("margin{e}")) == "20 mm"));
    let p = params(&d);
    assert!((p["margins"]["outside"].as_f64().unwrap() - 20.0 * PT_PER_MM).abs() < 1e-9);
    // Typed nonsense falls back to the defaults instead of reaching the document.
    d.fields.insert("gutter".into(), json!("wide"));
    d.fields.insert("pages".into(), json!("-4"));
    let p = params(&d);
    assert_eq!((p["gutter"].as_f64(), p["pages"].as_u64()), (Some(12.0), Some(1)));
}

/// The app's window (with a document) with the New Document dialog and its Save Document Preset
/// popup open.
fn window_with_preset_popup() -> egui_kittest::Harness<'static, crate::test_window::Window> {
    use egui_kittest::kittest::Queryable as _;
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let mut h = crate::test_window::open(app, vec2(1440.0, 900.0));
    // The dialog takes a couple of frames to settle where it opens.
    h.run_steps(4);
    let button = h.get_by_label("Save Document Preset").rect().center();
    crate::test_window::click_at(&mut h, button);
    assert!(egui::Popup::is_any_open(&h.ctx), "the popup opened");
    h
}

#[test]
fn enter_in_the_preset_popup_saves_the_preset_and_keeps_the_dialog() {
    use egui_kittest::kittest::Queryable as _;
    let mut h = window_with_preset_popup();
    let field = h.get_by_label("Preset Name").rect().center();
    crate::test_window::click_at(&mut h, field);
    h.get_by_label("Preset Name").type_text("Brochure");
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let app = &mut h.state_mut().app;
    let d = app.ui.dialog.as_ref().expect("Enter saves the preset; it doesn't create the document");
    assert_eq!(app.session.documents().len(), 1, "no document was made (the window opens with one)");
    assert_eq!(app.session.prefs.document_presets.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Brochure"]);
    assert_eq!((d.s("tab"), d.s("preset")), ("saved".into(), "saved:Brochure".into()));
    // The saved preset makes documents by name, as file.new {preset} does from anywhere.
    app.run("file.new", json!({"preset": "Brochure"})).unwrap();
    assert!(!egui::Popup::is_any_open(&h.ctx), "the popup closed");
}

#[test]
fn saved_preset_delete_buttons_keep_the_preview_and_never_create_a_document() {
    use egui_kittest::kittest::Queryable as _;
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.savePreset", json!({"name":"Portrait draft","pages":3,"facingPages":false})).unwrap();
    app.run("file.savePreset", json!({"name":"Landscape draft","width":792,"height":612,"pages":2})).unwrap();
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let mut h = crate::test_window::open(app, vec2(1440.0, 900.0));
    h.run_steps(4);
    let saved = h.get_by_label("Saved").rect().center();
    crate::test_window::click_at(&mut h, saved);
    let portrait = h.get_by_label("Portrait draft").rect().center();
    crate::test_window::click_at(&mut h, portrait);
    h.get_by_label("Preview").click();
    h.run_steps(20);
    assert!(h.query_by_label("Delete Preset: Portrait draft").is_some(), "the selected preset has its action");
    assert!(h.query_by_label("Delete Preset: [Default]").is_none(), "the factory card cannot be deleted");
    let before = h.state().app.session.active().unwrap().doc.clone();
    let preview = h.state().app.session.new_document_preview();
    let count = h.state().app.session.documents().len();
    let fields = params(h.state().app.ui.dialog.as_ref().unwrap());
    let landscape = h.get_by_label("Landscape draft").rect().center();
    h.event(egui::Event::PointerMoved(landscape));
    h.run_steps(3);
    let remove = h.get_by_label("Delete Preset: Landscape draft").rect().center();
    crate::test_window::click_at(&mut h, remove);
    assert_eq!(h.state().app.session.prefs.document_presets.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Portrait draft"]);
    let remove = h.get_by_label("Delete Preset: Portrait draft").rect().center();
    crate::test_window::click_at(&mut h, remove);
    let app = &h.state().app;
    assert!(app.session.prefs.document_presets.is_empty());
    assert_eq!(app.session.new_document_preview(), preview);
    assert_eq!(app.session.documents().len(), count);
    assert_eq!(*app.session.active().unwrap().doc, *before);
    assert_eq!(params(app.ui.dialog.as_ref().unwrap()), fields, "deletion does not select another preset or create the preview");
}

#[test]
fn escape_in_the_preset_popup_closes_only_the_popup() {
    let mut h = window_with_preset_popup();
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!egui::Popup::is_any_open(&h.ctx), "Escape closed the popup");
    assert!(h.state().app.ui.dialog.is_some(), "and left the dialog open");
    // With no popup open, Escape closes the dialog as before.
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(h.state().app.ui.dialog.is_none());
}

/// Set dialog fields, then let the preview settle.
fn set(app: &mut DesignApp, ctx: &egui::Context, fields: &[(&str, Value)]) {
    let d = app.ui.dialog.as_mut().unwrap();
    for (k, v) in fields {
        d.fields.insert((*k).into(), v.clone());
    }
    frame(app, ctx, vec![]);
    frame_after(app, ctx, vec![], 2.0 * SETTLE_SECONDS);
}

fn shown(app: &DesignApp) -> (usize, f64, u32) {
    let doc = &app.session.active().unwrap().doc;
    (app.session.documents().len(), doc.settings.page_width, doc.page(0).unwrap().columns.count)
}

fn escape(app: &mut DesignApp, ctx: &egui::Context) {
    frame(
        app,
        ctx,
        vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
    );
    assert!(app.ui.dialog.is_none());
}

/// Documents A and B with A active, and the New Document window open over them.
fn two_documents() -> (DesignApp, egui::Context, designcraft_engine::DocState) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "A"})).unwrap();
    app.run("file.new", json!({"title": "B"})).unwrap();
    app.session.set_active(0);
    let a = app.session.active().unwrap().clone();
    app.run("app.newDocumentDialog", json!({})).unwrap();
    (app, context(), a)
}

#[test]
fn preview_shows_the_settings_live_and_close_restores_the_workspace() {
    let (mut app, ctx, a) = two_documents();
    settle(&mut app, &ctx);
    assert_eq!(app.session.documents().len(), 2, "Preview starts off");
    set(&mut app, &ctx, &[("preview", json!(true))]);
    assert_eq!(shown(&app), (3, 612.0, 1), "a third tab shows the dialog's settings");
    let uid = app.session.new_document_preview();
    settle(&mut app, &ctx);
    assert_eq!(app.session.new_document_preview(), uid, "unchanged settings keep the same preview");
    // A change waits for the typing to stop.
    app.ui.dialog.as_mut().unwrap().fields.insert("width".into(), json!("300 pt"));
    frame(&mut app, &ctx, vec![]);
    assert_eq!(shown(&app).1, 612.0);
    set(&mut app, &ctx, &[("columns", json!("3"))]);
    assert_eq!(shown(&app), (3, 300.0, 3), "and then follows them");
    // Half-typed and refused values keep the last preview.
    for width in ["", "-5", "0"] {
        set(&mut app, &ctx, &[("width", json!(width))]);
        assert_eq!(shown(&app), (3, 300.0, 3), "{width:?}");
    }
    // Escape closes the window: the preview goes and A is shown again, untouched.
    escape(&mut app, &ctx);
    let back = app.session.active().unwrap();
    assert_eq!(app.session.documents().len(), 2);
    assert_eq!((back.uid, back.revision, back.history.undo.len(), back.is_dirty()), (a.uid, a.revision, 0, false));
    // Preview is remembered; turning it off removes the preview.
    app.run("app.newDocumentDialog", json!({})).unwrap();
    frame(&mut app, &ctx, vec![]);
    assert_eq!(app.session.documents().len(), 3);
    set(&mut app, &ctx, &[("preview", json!(false))]);
    assert_eq!((app.session.documents().len(), app.session.active().unwrap().uid), (2, a.uid));
    // `ui.dialog.cancel` removes it too.
    set(&mut app, &ctx, &[("preview", json!(true))]);
    assert_eq!(app.session.documents().len(), 3);
    let (req, _) = crate::control::ControlRequest::new("ui.dialog.cancel", json!({}));
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        crate::control::handle(&mut app, ui.ctx(), &req);
    });
    out.textures_delta.clear();
    assert_eq!((app.session.documents().len(), app.session.active().unwrap().uid), (2, a.uid));
}

#[test]
fn the_update_loop_ends_a_preview_whose_window_has_gone() {
    let (mut app, ctx, a) = two_documents();
    set(&mut app, &ctx, &[("preview", json!(true))]);
    assert_eq!(app.session.documents().len(), 3);
    app.ui.dialog = None;
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.logic(ui.ctx()));
    out.textures_delta.clear();
    assert_eq!((app.session.documents().len(), app.session.active().unwrap().uid), (2, a.uid));
}

#[test]
fn closing_leaves_a_document_activated_or_edited_meanwhile() {
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

#[test]
fn closing_the_start_screen_preview_leaves_no_document() {
    let mut app = app();
    let ctx = context();
    set(&mut app, &ctx, &[("preview", json!(true))]);
    assert_eq!(app.session.documents().len(), 1);
    escape(&mut app, &ctx);
    assert!(app.session.documents().is_empty() && app.session.active().is_none());
}

#[test]
fn create_keeps_the_previewed_document_with_the_final_settings() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "A"})).unwrap();
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let ctx = context();
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

#[test]
fn presets_are_saved_by_name_and_recent_documents_once() {
    let mut app = app();
    let mut d = app.ui.dialog.clone().unwrap();
    save_preset(&mut app, &mut d, " Newsletter ").unwrap();
    d.fields.insert("pages".into(), json!(8));
    save_preset(&mut app, &mut d, "Newsletter").unwrap();
    assert_eq!(app.session.prefs.document_presets.len(), 1, "the same name replaces the preset");
    assert!(save_preset(&mut app, &mut d, "Letter").is_err(), "a built-in name is refused");
    assert_eq!((d.s("tab"), d.s("preset")), ("saved".into(), "saved:Newsletter".into()));
    let saved = cards(&app, "saved");
    assert_eq!(saved.len(), 2);
    assert_eq!((saved[0].key.as_str(), saved[0].nd.pages), (DEFAULT_CARD, 1), "[Default] comes first, as in InDesign");
    assert_eq!((saved[1].label.as_str(), saved[1].nd.pages), ("Newsletter", 8));
    let p = params(&d);
    remember(&mut app, &p);
    remember(&mut app, &p);
    assert_eq!(app.ui.recent_new_documents.len(), 1, "a repeat moves to the front");
}
