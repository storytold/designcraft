// Native-dialog tests parked by the wave 4 cleanup (w4-dialogs). Not compiled: this file is not declared as a module.
//
// Each test below drove one of the old native egui dialog windows (or a native widget the framework form replaced) and has no
// framework successor yet (the successors table marks it Pending, or the test was not listed there). The native bodies they
// exercised are deleted, so they cannot run as written. They are kept here, with the helpers only they used, so the
// successors can be written from the original assertions; delete each one when its successor lands.

// ===== from dialogs/tests.rs =====

/// The Justification dialog shows Auto Leading and Single Word Justification, OK sets them on the
/// selected paragraph as one undo step, bad values keep the dialog open, and both values survive
/// saving and opening the native file and an IDML export.
#[test]
fn justification_dialog_sets_auto_leading_and_single_word_justification() {
    use egui_kittest::{Harness, kittest::Queryable};
    let (mut app, story) = two_paragraphs();
    app.run("app.justificationDialog", json!({})).unwrap();
    let d = app.ui.dialog.clone().unwrap();
    let current = d.fields["current"].clone();
    assert_eq!((current["autoLeading"].clone(), current["singleWordJustify"].clone()), (json!(1.2), json!("fullyJustified")));
    let mut h = Harness::builder().with_size(egui::vec2(1000.0, 800.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let key = egui::Id::unique("justification_single_word_fonts");
            if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(key, true));
                return;
            }
            show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(3);
    assert!(h.query_by_label("Auto Leading:").is_some());
    assert!(h.query_by_label("Single Word Justification:").is_some());
    use egui::accesskit::Role;
    // The dropdown shows the paragraph's Full Justify. Picking Full Justify again is no edit.
    h.get_by(|n| n.role() == Role::ComboBox && n.value().as_deref() == Some("Full Justify")).click();
    h.run_steps(3);
    h.get_by(|n| n.role() != Role::ComboBox && n.label().as_deref() == Some("Full Justify")).click();
    h.run_steps(3);
    assert!(!h.state().ui.dialog.as_ref().unwrap().fields.contains_key("singleWordJustify"), "the paragraph's own choice is no edit");
    // Pick Align Center.
    h.get_by(|n| n.role() == Role::ComboBox && n.value().as_deref() == Some("Full Justify")).click();
    h.run_steps(3);
    h.get_by_label("Align Center").click();
    h.run_steps(3);
    let mut app = std::mem::replace(h.state_mut(), DesignApp::new(designcraft_engine::Session::new(), crate::Services::default()));
    drop(h);
    let ctx = crate::test_window::context();
    assert_eq!(app.ui.dialog.as_ref().unwrap().fields["singleWordJustify"], "center", "the choice is the edit");

    // Out of range or not a choice: an error, the dialog stays open, nothing changes.
    let d = app.ui.dialog.clone().unwrap();
    let before = undo_steps(&app);
    let bad = [
        ("autoLeading", json!(5.01)),
        ("autoLeading", json!(-0.01)),
        ("autoLeading", json!(f64::NAN)),
        ("singleWordJustify", json!("leftJustified")),
    ];
    for (key, v) in bad {
        app.ui.dialog = Some(d.clone());
        app.ui.dialog.as_mut().unwrap().fields.insert(key.into(), v.clone());
        assert!(confirm(&mut app).is_err(), "{key} = {v}");
        assert!(app.ui.dialog.as_ref().is_some_and(|d| d.fields.contains_key("justifyError")), "{key}: the dialog shows why");
    }
    assert_eq!(undo_steps(&app), before);

    // A typed percent is stored as the whole percent the field shows, so 500.4% (shown as 500%)
    // and -0.4% (shown as 0%) are accepted.
    for (typed, stored) in [(500.4, 5.0), (-0.4, 0.0), (149.6, 1.5)] {
        assert_eq!(auto_leading_typed(typed), stored, "{typed}%");
        let mut probe = d.clone();
        probe.fields.insert("autoLeading".into(), json!(auto_leading_typed(typed)));
        assert_eq!(check_justification(&probe, "", &current), Ok(()), "{typed}%");
    }

    app.ui.dialog = Some(d);
    app.ui.dialog.as_mut().unwrap().fields.insert("autoLeading".into(), json!(1.5));
    draw(&mut app, &ctx);
    confirm(&mut app).unwrap();
    assert_eq!(undo_steps(&app), before + 1, "one undo step");
    let both = |app: &mut DesignApp, story: &Value, at| (para_attr(app, story, at, "autoLeading"), para_attr(app, story, at, "singleWordJustify"));
    assert_eq!(both(&mut app, &story, 0), (json!(1.5), json!("center")));
    assert_eq!(both(&mut app, &story, 4), (json!(1.2), json!("fullyJustified")), "only the selected paragraph");

    // Both paragraphs selected: the fields that differ show blank, and OK with no edit adds no undo step.
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 7})).unwrap();
    app.run("app.justificationDialog", json!({})).unwrap();
    draw(&mut app, &ctx);
    let current = app.ui.dialog.as_ref().unwrap().fields["current"].clone();
    assert_eq!((current["autoLeading"].clone(), current["singleWordJustify"].clone()), (Value::Null, Value::Null));
    let before = undo_steps(&app);
    confirm(&mut app).unwrap();
    assert_eq!(undo_steps(&app), before);

    // Saved and opened again, natively and through IDML.
    let native = app.run("file.serialize", json!({})).unwrap();
    app.run("file.openBytes", json!({"name": "justify.designcraft", "base64": native["base64"]})).unwrap();
    assert_eq!(both(&mut app, &story, 0), (json!(1.5), json!("center")));
    let idml = app.run("file.exportIdml", json!({})).unwrap();
    app.run("file.openBytes", json!({"name": "justify.idml", "base64": idml["base64"]})).unwrap();
    let doc = &app.session.doc().unwrap().doc;
    let sid = doc.stories.iter().find(|(_, s)| s.text == "One\nTwo").map(|(id, _)| *id).expect("the story comes back");
    let story = serde_json::to_value(sid).unwrap();
    assert_eq!(both(&mut app, &story, 0), (json!(1.5), json!("center")));
    assert_eq!(both(&mut app, &story, 4), (json!(1.2), json!("fullyJustified")));
}


// ===== from dialogs/style_cjk.rs::tests =====

    #[test]
    fn kenten_and_japanese_composition_reach_the_style_and_its_paragraph() {
        let mut app = app_with_text();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        if let Some(st) = app.session.active_mut() {
            let table = designcraft_doc::cjk::MojikumiTable { name: "Body Text".into(), ..Default::default() };
            std::sync::Arc::make_mut(&mut st.doc).styles_mut().mojikumi_tables.push(table);
        }
        let doc = app.session.active().unwrap().doc.clone();
        let hard = kinsoku_sets(&doc).into_iter().find(|k| k.name == "HardKinsoku").unwrap();
        let moji = mojikumi_sets(&doc, "").into_iter().find(|s| s.1 == "Body Text").unwrap().0;
        assert_eq!(moji, "MojikumiTable/Body Text");

        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Body", "section": "cjk.kenten"})));
        painted(&mut app, &ctx);
        {
            let d = app.ui.dialog.as_mut().unwrap();
            pick_kenten(d, &CharFields { base: &Value::Null, sparse: false }, Some(KentenKind::BlackCircle));
            d.fields.insert("section".into(), json!("cjk.composition"));
            d.fields.insert("p.kinsoku".into(), serde_json::to_value(&hard).unwrap());
            d.fields.insert("p.mojikumi".into(), json!(moji));
            d.fields.insert("p.kinsokuHang".into(), json!("regular"));
        }
        painted(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();

        let doc = app.session.active().unwrap().doc.clone();
        let st = doc.styles.para("Body").unwrap();
        assert_eq!(st.para.kinsoku, Some(Some(hard)));
        assert_eq!(st.para.mojikumi.as_deref(), Some("MojikumiTable/Body Text"));
        assert_eq!(st.para.kinsoku_hang, Some(designcraft_doc::cjk::KinsokuHang::Regular));
        assert_eq!((st.chars.kenten, st.chars.kenten_kind), (Some(true), Some(KentenKind::BlackCircle)));
        assert_eq!(st.chars.kenten_character.as_deref(), Some("\u{25CF}"));
        let a = app.run("type.selectionAttrs", json!({})).unwrap();
        assert_eq!(a["para"]["kinsoku"]["name"], "HardKinsoku", "the paragraph follows its style");
        assert_eq!(a["para"]["mojikumi"], "MojikumiTable/Body Text");
        assert_eq!(a["chars"]["kenten"], true);
        assert_eq!(a["chars"]["kentenKind"], "blackCircle");

        // Default Rules (`null`) replaces the set.
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Body", "section": "cjk.composition", "p.kinsoku": null})));
        painted(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.active().unwrap().doc.styles.para("Body").unwrap().para.kinsoku, Some(None));
        assert!(app.run("type.selectionAttrs", json!({})).unwrap()["para"]["kinsoku"].is_null());

        // A character style sets only the kenten attributes.
        app.ui.dialog = Some(Dialog::new("characterStyleOptions", json!({"name": "Emphasis", "section": "cjk.kenten"})));
        painted(&mut app, &ctx);
        {
            let d = app.ui.dialog.as_mut().unwrap();
            let base = json!({});
            pick_kenten(d, &CharFields { base: &base, sparse: true }, Some(KentenKind::SesameDot));
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let st = app.session.active().unwrap().doc.styles.char_style("Emphasis").cloned().unwrap();
        assert_eq!((st.chars.kenten, st.chars.kenten_kind), (Some(true), Some(KentenKind::SesameDot)));
        assert!(st.chars.ruby_alignment.is_none() && st.chars.shatai_angle.is_none(), "nothing else is set");
    }

    /// The text painted by the open dialog once it has settled.
    fn painted(app: &mut DesignApp, ctx: &egui::Context) -> Vec<String> {
        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.trim().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
        let mut texts = Vec::new();
        for _ in 0..4 {
            let time = ctx.input(|i| i.time) + 1.0 / 60.0;
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(time), ..Default::default() }, |ui| {
                crate::dialogs::show(app, ui.ctx());
            });
            output.textures_delta.clear();
            texts.clear();
            for s in &output.shapes {
                collect(&s.shape, &mut texts);
            }
        }
        texts
    }


// ===== from dialogs.rs::color_theme_tests =====

    /// Enter in a multi-line field makes a new line in the Printing Instructions only; every other
    /// dialog keeps Enter as OK, and the flag does not outlive the frame.
    #[test]
    fn enter_in_a_multiline_field_is_local_to_the_printing_instructions() {
        let ctx = egui::Context::default();
        set_multiline_focus(&ctx);
        assert!(take_multiline_focus(&ctx, crate::package_dialog::INSTRUCTIONS));
        assert!(!take_multiline_focus(&ctx, crate::package_dialog::INSTRUCTIONS), "cleared once read");
        set_multiline_focus(&ctx);
        assert!(!take_multiline_focus(&ctx, "insertPages"));
    }


// ===== from dialogs.rs::variable_style_regression =====

    #[test]
    #[ignore = "requires the pinned Bricolage fixture; see docs/testing-variable-fonts.md"]
    fn named_instance_style_widget_and_api_agree() {
        use egui_kittest::{Harness, kittest::Queryable};
        let path = std::env::var_os("DESIGNCRAFT_TEST_VARIABLE_FONT").expect("set DESIGNCRAFT_TEST_VARIABLE_FONT");
        let db = designcraft_fonts::FontDb::global();
        db.add_font(std::fs::read(path).unwrap());
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("style.paragraph.create", json!({"name":"Base VF", "chars":{"fontFamily":"Bricolage Grotesque", "fontStyle":"Regular"}})).unwrap();
        app.run("style.paragraph.create", json!({"name":"UI VF", "basedOn":"Base VF"})).unwrap();
        app.run("style.paragraph.create", json!({"name":"API VF", "basedOn":"Base VF"})).unwrap();
        let base = json!({"fontFamily":"Bricolage Grotesque", "fontStyle":"Regular"});
        let mut h = Harness::builder()
            .with_context_setup(|ctx| {
                crate::theme::install_fonts(ctx, "");
            })
            .with_size(egui::vec2(1000.0, 800.0))
            .build_ui_state(
                move |ui, state: &mut (DesignApp, Dialog)| {
                    basic_character_formats(&mut state.0, ui, &mut state.1, &CharFields { base: &base, sparse: false });
                },
                (app, Dialog::new("paragraphStyleOptions", json!({"name":"UI VF"}))),
            );
        h.run_steps(3);
        {
            let (app, d) = h.state_mut();
            let before = app.session.doc().unwrap().doc.clone();
            let undo = app.session.doc().unwrap().history.undo.len();
            d.fields.insert("preview".into(), json!(true));
            d.fields.insert("c.fontStyle".into(), json!("SemiBold"));
            style_options_preview(app, d);
            assert_eq!(app.session.doc().unwrap().doc.styles.resolve_para_style("UI VF").1.font_style, "SemiBold");
            assert_eq!(app.session.doc().unwrap().history.undo.len(), undo);
            d.fields.insert("preview".into(), json!(false));
            style_options_preview(app, d);
            assert_eq!(app.session.doc().unwrap().doc, before);
            d.fields.insert("preview".into(), json!(true));
            style_options_preview(app, d);
            app.ui.dialog = Some(d.clone());
            cancel(app);
            assert_eq!(app.session.doc().unwrap().doc, before);
            d.fields.remove(PREVIEWED);
            d.fields.insert("preview".into(), json!(true));
            style_options_preview(app, d);
            app.ui.dialog = Some(d.clone());
            confirm(app).unwrap();
            assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
            app.run("edit.undo", json!({})).unwrap();
            assert_eq!(app.session.doc().unwrap().doc, before);
            app.run("edit.redo", json!({})).unwrap();
            assert_eq!(app.session.doc().unwrap().doc.styles.resolve_para_style("UI VF").1.font_style, "SemiBold");
            app.run("edit.undo", json!({})).unwrap();
            d.fields.remove(PREVIEWED);
            d.fields.remove("c.fontStyle");
            d.fields.insert("preview".into(), json!(false));
        }
        h.run_steps(3);
        let mut current = "Regular";
        for selected in ["SemiBold", "Bold", "Regular"] {
            h.get_by_value(current).click();
            h.run_steps(3);
            h.get_by_label(selected).click();
            h.run_steps(3);
            let (app, d) = h.state_mut();
            assert_eq!(d.fields["c.fontStyle"], selected);
            assert!(!d.fields.contains_key("c.fontFamily"), "inherited family remains inherited");
            confirm_paragraph_style_options(app, d).unwrap();
            app.run("style.paragraph.edit", json!({"name":"API VF","chars":{"fontStyle":selected}})).unwrap();
            let styles = &app.session.doc().unwrap().doc.styles;
            assert_eq!(styles.para("UI VF").unwrap().chars, styles.para("API VF").unwrap().chars);
            assert_eq!(styles.resolve_para_style("UI VF").1.font_style, selected);
            assert_eq!(styles.para("UI VF").unwrap().based_on.as_deref(), Some("Base VF"));
            current = selected;
            let native = app.run("file.serialize", json!({})).unwrap();
            app.run("file.openBytes", json!({"name":"vf.designcraft","base64":native["base64"]})).unwrap();
            assert_eq!(app.session.doc().unwrap().doc.styles.resolve_para_style("UI VF").1.font_style, selected);
        }
    }


// ===== from dialogs.rs::variable_cold_style_regression =====

    #[test]
    #[ignore = "requires a fresh process with the pinned Bricolage fixture catalogued; see docs/testing-variable-fonts.md"]
    fn cold_style_combo_contains_named_instances() {
        use egui_kittest::{Harness, kittest::Queryable};
        let fonts = designcraft_fonts::FontDb::global();
        assert!(
            !fonts.styles("Bricolage Grotesque").contains(&"SemiBold".into()),
            "run in a fresh process with the test font catalogued but not loaded"
        );
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let base = json!({"fontFamily":"Bricolage Grotesque", "fontStyle":"Regular"});
        let mut h = Harness::builder()
            .with_context_setup(|ctx| {
                crate::theme::install_fonts(ctx, "");
            })
            .with_size(egui::vec2(1000.0, 800.0))
            .build_ui_state(
                move |ui, state: &mut (DesignApp, Dialog)| {
                    basic_character_formats(&mut state.0, ui, &mut state.1, &CharFields { base: &base, sparse: false });
                },
                (app, Dialog::new("paragraphStyleOptions", json!({}))),
            );
        h.run_steps(3);
        h.get_by_value("Regular").click();
        h.run_steps(3);
        assert!(h.query_by_label("SemiBold").is_some(), "the cold Style combo must offer named instances");
        h.get_by_label("SemiBold").click();
        h.run_steps(3);
        assert_eq!(h.state().1.fields["c.fontStyle"], "SemiBold");
    }


// ===== from dialogs.rs::synthetic_variable_style_regression =====

    #[test]
    fn paragraph_style_formats_select_a_synthetic_named_instance() {
        let family = "Synthetic Paragraph Wiring";
        designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::variable_font(family).unwrap());
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("style.paragraph.create", json!({"name":"Synthetic UI", "chars":{"fontFamily":family,"fontStyle":"Regular"}})).unwrap();
        let base = json!({"fontFamily":family,"fontStyle":"Regular"});
        let mut h = Harness::builder()
            .with_context_setup(|ctx| {
                crate::theme::install_fonts(ctx, "");
            })
            .with_size(egui::vec2(1000.0, 800.0))
            .build_ui_state(
                |ui, state: &mut (DesignApp, Dialog)| {
                    basic_character_formats(&mut state.0, ui, &mut state.1, &CharFields { base: &base, sparse: false });
                },
                (app, Dialog::new("paragraphStyleOptions", json!({"name":"Synthetic UI"}))),
            );
        h.run_steps(3);
        h.get_by_value("Regular").click();
        h.run_steps(3);
        h.get_by_label("SemiBold").click();
        h.run_steps(3);
        let (app, d) = h.state_mut();
        confirm_paragraph_style_options(app, d).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.styles.resolve_para_style("Synthetic UI").1.font_style, "SemiBold");
    }

    #[test]
    fn cold_style_popup_discovers_instances_and_selects_them() {
        let dir = tempfile::tempdir().unwrap();
        let family = "Synthetic Cold Style";
        std::fs::write(dir.path().join("variable.ttf"), designcraft_fonts::testing::variable_font(family).unwrap()).unwrap();
        let db = designcraft_fonts::FontDb::with_font_dirs(vec![dir.path().to_owned()]);
        assert_eq!(db.styles(family), ["Regular"]);
        let mut h = Harness::builder()
            .with_context_setup(|ctx| {
                crate::theme::install_fonts(ctx, "");
            })
            .build_ui_state(
                |ui, d: &mut Dialog| {
                    let style = d.fields.get("c.fontStyle").and_then(Value::as_str).unwrap_or("Regular").to_string();
                    font_style_combo(ui, &db.scoped(0), 0, family, &style, d, &CharFields { base: &Value::Null, sparse: false })
                        .accessible_name("Font Style".to_owned());
                },
                Dialog::new("paragraphStyleOptions", json!({})),
            );
        h.run_steps(3);
        assert_eq!(db.styles(family), ["Regular"], "closed popup must not preload the font");
        h.get_by_value("Regular").click();
        h.run_steps(3);
        assert_eq!(db.styles(family), ["Regular", "SemiBold"]);
        h.get_by_label("SemiBold").click();
        h.run_steps(3);
        assert_eq!(h.state().fields["c.fontStyle"], "SemiBold");
    }

    #[test]
    fn missing_family_styles_are_cached_until_the_popup_reopens() {
        for retry in ["reopen", "hidden", "scope"] {
            let dir = tempfile::tempdir().unwrap();
            let family = "Synthetic Missing Style";
            let path = dir.path().join("variable.ttf");
            std::fs::write(&path, designcraft_fonts::testing::variable_font(family).unwrap()).unwrap();
            let db = designcraft_fonts::FontDb::with_font_dirs(vec![dir.path().to_owned()]);
            assert_eq!(db.styles(family), ["Regular"]);
            std::fs::remove_file(&path).unwrap();
            let mut h = Harness::builder()
                .with_context_setup(|ctx| {
                    crate::theme::install_fonts(ctx, "");
                })
                .build_ui_state(
                    |ui, state: &mut (Dialog, bool, u32)| {
                        if state.1 {
                            font_style_combo(
                                ui,
                                &db.scoped(state.2),
                                state.2,
                                family,
                                "Regular",
                                &mut state.0,
                                &CharFields { base: &Value::Null, sparse: false },
                            )
                            .accessible_name("Font Style".to_owned());
                        }
                    },
                    (Dialog::new("paragraphStyleOptions", json!({})), true, 0),
                );
            h.run_steps(3);
            h.get_by_value("Regular").click();
            h.run_steps(3);
            assert!(h.query_by_label("SemiBold").is_none());
            std::fs::write(&path, designcraft_fonts::testing::variable_font(family).unwrap()).unwrap();
            h.run_steps(3);
            assert_eq!(db.styles(family), ["Regular"], "an open popup must not retry the missing font every frame");
            match retry {
                "hidden" => {
                    h.state_mut().1 = false;
                    h.run_steps(3);
                    h.state_mut().1 = true;
                }
                "scope" => h.state_mut().2 = 1,
                _ => {
                    egui::Popup::close_all(&h.ctx);
                    h.run_steps(3);
                    h.get_by_value("Regular").click();
                }
            }
            h.run_steps(3);
            if h.query_by_label("SemiBold").is_none() {
                h.get_by_value("Regular").click();
                h.run_steps(3);
            }
            assert!(h.query_by_label("SemiBold").is_some(), "{retry} retries the now-readable font");
        }
    }


// ===== from package_dialog.rs::tests =====

    /// The course's chapter 27: File › Package opens InDesign's Package dialog. Summary counts the
    /// fonts, links (one missing, with a warning) and inks; Links and Images lists the link, and
    /// Show Problems Only keeps it; Create Printing Instructions leads to Printing Instructions,
    /// whose Continue asks for the folder and writes the package with the instructions.
    #[test]
    fn package_shows_a_summary_then_the_printing_instructions() {
        let dir = temp_dir("summary");
        let mut app = app_with_missing_link(&dir);
        let out = dir.join("Packed Folder");
        let picked = out.to_string_lossy().to_string();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked.clone())));
        crate::menus::activate(&mut app, "app.packageDialog", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(ID));
        let mut h = harness(app);
        h.run_steps(3);
        assert!(h.query_by_label("Scope:").is_some());
        assert!(h.query_by_label("Links and Images: 1 linked, 0 changed, 1 not found").is_some());
        assert!(h.query_by_label("Colors and Inks: 4 process, 0 spot").is_some());
        let fonts = h.state().ui.dialog.as_ref().unwrap().fields["fonts"].as_array().unwrap().len();
        assert!(h.query_by_label(&format!("Fonts: {fonts} in use, 0 unavailable")).is_some());
        // Links and Images: the link, missing; Show Problems Only keeps it.
        h.get_by_label("Links and Images").click();
        h.run_steps(2);
        assert!(h.query_by_label("gone.png").is_some());
        assert!(h.query_by_label("\u{26A0} Missing").is_some());
        h.get_by_label("Show Problems Only").click();
        h.run_steps(2);
        assert!(h.query_by_label("gone.png").is_some());
        // Back on Summary: Create Printing Instructions, then Package….
        h.get_by_label("Summary").click();
        h.run_steps(2);
        h.get_by_label("Create Printing Instructions").click();
        h.run_steps(2);
        h.get_by_label("Package…").click();
        h.run_steps(3);
        h.state_mut().settle_dialogs();
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some(INSTRUCTIONS));
        for (key, value) in [("contact", "Dave"), ("email", "dave@example.com"), ("instructions", "Call me if anything is wrong.")] {
            h.state_mut().ui.dialog.as_mut().unwrap().fields.insert(key.into(), json!(value));
        }
        h.run_steps(2);
        h.get_by_label("Continue").click();
        h.run_steps(3);
        h.state_mut().settle_dialogs();
        assert!(h.state().ui.dialog.is_none());
        let report = std::fs::read_to_string(out.join("Instructions.txt")).unwrap();
        assert!(
            report.contains("Contact: Dave") && report.contains("Email: dave@example.com") && report.contains("Call me if anything is wrong."),
            "{report}"
        );
        assert!(out.join("Packed.idml").exists(), "Include IDML is on by default");
        assert!(!out.join("Packed.pdf").exists(), "Include PDF is off by default");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Without instructions, Package… goes straight to the folder; Include PDF adds the PDF, made
    /// with the PDF preset chosen, and Include IDML off leaves the IDML out.
    #[test]
    fn package_options_choose_the_idml_and_pdf() {
        let dir = temp_dir("options");
        let mut app = app_with_missing_link(&dir);
        let out = dir.join("Packed Folder");
        let picked = out.to_string_lossy().to_string();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked.clone())));
        open(&mut app).unwrap();
        let mut h = harness(app);
        h.run_steps(3);
        h.get_by_label("Include IDML").click();
        h.run_steps(2);
        h.get_by_label("Include PDF (Print)").click();
        h.run_steps(2);
        // The PDF's preset: PDF/X-4.
        h.get_by_value("Desktop Printing").click();
        h.run_steps(2);
        h.get_by_label("PDF/X-4").click();
        h.run_steps(2);
        h.get_by_label("Package…").click();
        h.run_steps(3);
        h.state_mut().settle_dialogs();
        assert!(h.state().ui.dialog.is_none());
        assert!(out.join("Packed.pdf").exists() && !out.join("Packed.idml").exists());
        let pdf = std::fs::read(out.join("Packed.pdf")).unwrap();
        assert!(pdf.windows(8).any(|w| w == b"GTS_PDFX"), "the PDF/X-4 preset's PDF");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Enter in Address or Instructions makes a new line there; in a one-line field it continues.
    #[test]
    fn enter_in_a_multiline_instruction_field_is_a_new_line() {
        let dir = temp_dir("enter");
        let mut app = app_with_missing_link(&dir);
        let doc = app.session.active().unwrap().uid;
        app.ui.dialog = Some(Dialog::new(INSTRUCTIONS, json!({"idml": true, "pdf": false, "doc": doc})));
        let mut h = harness(app);
        h.run_steps(3);
        let field = |h: &Harness<'static, DesignApp>, n: usize| {
            h.query_all_by(|x| x.role() == egui::accesskit::Role::TextInput || x.role() == egui::accesskit::Role::MultilineTextInput)
                .nth(n)
                .unwrap()
                .rect()
                .center()
        };
        // Address is the third field.
        let address = field(&h, 2);
        h.event(egui::Event::PointerButton { pos: address, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
        h.event(egui::Event::PointerButton { pos: address, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some(INSTRUCTIONS), "still open");
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
        Harness::builder().with_size(egui::vec2(1100.0, 760.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let key = egui::Id::unique("package_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(key, true));
                    return;
                }
                crate::dialogs::show(app, ui.ctx());
            },
            app,
        )
    }


// ===== from effects_dialog.rs::tests =====

    /// With Use Global Light on, the Angle is the document's global light and stays editable:
    /// a change there is applied to the document with the effects (it was greyed out).
    #[test]
    fn the_angle_with_global_light_sets_the_global_light() {
        let (mut app, _) = app_with_frame();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        crate::menus::activate(&mut app, "object.dropShadow", &Value::Null);
        let d = app.ui.dialog.as_mut().unwrap();
        set(d, &["dropShadow", "globalLight"], json!(true));
        frame(&mut app, &ctx);
        // The field shows the global angle and is enabled: a DragValue that isn't disabled.
        let global = app.session.doc().unwrap().doc.settings.global_light;
        let mut d = app.ui.dialog.clone().unwrap();
        let ui_ctx = egui::Context::default();
        let mut enabled = false;
        let mut out = ui_ctx.run_ui(egui::RawInput::default(), |ui| {
            enabled = angle(ui, &mut d, &["dropShadow", "angle"], false, global).enabled();
        });
        out.textures_delta.clear();
        assert!(enabled, "the Angle stays editable with Use Global Light");
        d.fields.insert(GLOBAL_LIGHT.into(), json!(15.0));
        confirm(&mut app, &d).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.settings.global_light, 15.0);
    }

    #[test]
    fn the_effects_dialog_opens_on_the_chosen_effect_previews_and_applies_once() {
        let (mut app, id) = app_with_frame();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        let undo = |app: &DesignApp| app.session.doc().unwrap().history.undo.len();
        let fx = |app: &DesignApp| app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().effects.clone();
        let u0 = undo(&app);
        // Object › Effects › Inner Shadow… (as the menu activates it): the dialog, on that page.
        crate::menus::activate(&mut app, "object.innerShadow", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(ID));
        let texts = frame(&mut app, &ctx);
        for t in [
            "Effects",
            "Settings for:",
            "Transparency",
            "Inner Shadow",
            "Drop Shadow",
            "Blending",
            "Position",
            "Distance:",
            "Angle:",
            "Use Global Light",
            "Options",
            "Size:",
            "Choke:",
            "Preview",
            "OK",
            "Cancel",
        ] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        assert!(texts.iter().any(|x| x.contains("0.0972 in")), "distance in inches: {texts:?}");
        // Preview: the shadow is on the page while the dialog is open, without an undo step.
        frame(&mut app, &ctx);
        assert!(fx(&app).inner_shadow.on);
        assert_eq!(undo(&app), u0);
        // Cancel: as before.
        crate::dialogs::cancel(&mut app);
        assert!(!fx(&app).inner_shadow.on);
        assert_eq!(undo(&app), u0);
        // Again, with a change, then OK: one undo step.
        crate::menus::activate(&mut app, "object.innerShadow", &Value::Null);
        frame(&mut app, &ctx);
        if let Some(d) = app.ui.dialog.as_mut() {
            set(d, &["innerShadow", "size"], json!(9.0));
            d.fields.insert("opacity".into(), json!(0.8));
        }
        frame(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();
        let e = fx(&app);
        assert!(e.inner_shadow.on && e.inner_shadow.size == 9.0);
        assert_eq!(undo(&app), u0 + 1, "one undo step");
        assert!((app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().opacity - 0.8).abs() < 1e-6);
        app.run("edit.undo", json!({})).unwrap();
        assert!(!fx(&app).inner_shadow.on);
    }

    /// The angle dial points where the pointer is, as InDesign's: right is 0°, up 90°, left 180°,
    /// down −90°, upper left 135°.
    #[test]
    fn the_angle_dial_points_at_the_pointer() {
        let c = egui::pos2(100.0, 100.0);
        assert_eq!(dial_angle(c, egui::pos2(120.0, 100.0)), Some(0.0));
        assert_eq!(dial_angle(c, egui::pos2(100.0, 80.0)), Some(90.0));
        assert_eq!(dial_angle(c, egui::pos2(80.0, 100.0)), Some(180.0));
        assert_eq!(dial_angle(c, egui::pos2(100.0, 120.0)), Some(-90.0));
        assert_eq!(dial_angle(c, egui::pos2(90.0, 90.0)), Some(135.0));
        assert_eq!(dial_angle(c, egui::pos2(101.0, 100.0)), None, "too close to the centre");
    }

    /// Without a pointer: Tab focuses the dial, the arrows turn it (Shift: 10°), wrapping at 180°.
    #[test]
    fn the_angle_dial_turns_with_the_keyboard() {
        let ctx = egui::Context::default();
        let key = |key, shift| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: if shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE },
        };
        let mut deg = 175.0;
        let frame = |events: Vec<egui::Event>, deg: &mut f64| {
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                if let Some(a) = angle_dial(ui, *deg, true) {
                    *deg = a;
                }
            });
            out.textures_delta.clear();
        };
        frame(vec![], &mut deg);
        frame(vec![key(egui::Key::Tab, false)], &mut deg);
        frame(vec![key(egui::Key::ArrowRight, false)], &mut deg);
        assert_eq!(deg, 176.0);
        frame(vec![key(egui::Key::ArrowUp, true)], &mut deg);
        assert_eq!(deg, -174.0, "past 180° it comes round to −174°");
        frame(vec![key(egui::Key::ArrowLeft, true)], &mut deg);
        frame(vec![key(egui::Key::ArrowDown, false)], &mut deg);
        assert_eq!(deg, 175.0);
        assert_eq!(wrap_angle(-180.0), 180.0);
        assert_eq!(wrap_angle(540.0), 180.0);
    }

    /// Drop Shadow and Outer Glow have InDesign's Mode under Blending (Multiply and Screen to
    /// start); a mode picked there is what OK applies.
    #[test]
    fn shadows_and_glows_have_a_blend_mode() {
        let (mut app, id) = app_with_frame();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        open(&mut app, "dropShadow");
        let texts = frame(&mut app, &ctx);
        assert!(texts.iter().any(|x| x.trim() == "Mode:") && texts.iter().any(|x| x.trim() == "Multiply"), "{texts:?}");
        crate::dialogs::cancel(&mut app);
        open(&mut app, "outerGlow");
        let texts = frame(&mut app, &ctx);
        assert!(texts.iter().any(|x| x.trim() == "Screen"), "{texts:?}");
        if let Some(d) = app.ui.dialog.as_mut() {
            set(d, &["outerGlow", "mode"], json!("Normal"));
        }
        frame(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();
        let fx = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().effects.clone();
        assert!(fx.outer_glow.on);
        assert_eq!(fx.outer_glow.mode, designcraft_color::BlendMode::Normal);
    }

    fn frame(app: &mut DesignApp, ctx: &egui::Context) -> Vec<String> {
        let mut texts = Vec::new();
        for _ in 0..2 {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()));
            out.textures_delta.clear();
            fn walk(s: egui::epaint::Shape, texts: &mut Vec<String>) {
                match s {
                    egui::epaint::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                    egui::epaint::Shape::Vec(v) => v.into_iter().for_each(|s| walk(s, texts)),
                    _ => {}
                }
            }
            texts.clear();
            for c in out.shapes {
                walk(c.shape, &mut texts);
            }
        }
        texts
    }


// ===== from effects_dialog.rs::tests (helper) =====

/// The angle a dial at `c` points at for the pointer at `p`, in whole degrees (−180 to 180,
/// counter-clockwise from the right); none too close to the centre to tell.
fn dial_angle(c: egui::Pos2, p: egui::Pos2) -> Option<f64> {
    let d = p - c;
    // Straight left is 180° (not −180°, which a negative zero would give).
    (d.length() > 2.0).then(|| f64::from((-d.y).atan2(d.x)).to_degrees().round()).map(|a| if a <= -180.0 { 180.0 } else { a })
}


// ===== from object_style_dialog.rs::tests =====

    /// The course's chapter 15: New Object Style from a frame with rounded corners, a paper
    /// stroke and a drop shadow, named and applied to the selection; then Object Style Options
    /// with a thicker stroke: the frames using the style follow.
    #[test]
    fn a_new_object_style_from_the_selection_and_its_options() {
        let (mut app, a) = app_with_frame();
        app.run("object.cornerOptions", json!({"shape": "rounded", "size": 12, "ids": [a]})).unwrap();
        app.run("object.stroke", json!({"swatch": "[Paper]", "weight": 2.5, "ids": [a]})).unwrap();
        app.run("object.dropShadow", json!({"on": true, "ids": [a]})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        open_new(&mut app);
        let texts = frame(&mut app, &ctx);
        for t in [
            "New Object Style",
            "Style Name:",
            "Basic Attributes",
            "Fill",
            "Stroke & Corner Options",
            "Effects for:",
            "Drop Shadow",
            "Style Settings:",
            "Apply Style to Selection",
        ] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        assert!(texts.iter().any(|x| x.starts_with("Stroke: [Paper], 2.5 pt")), "{texts:?}");
        assert!(texts.iter().any(|x| x.starts_with("Effects: Drop Shadow")), "{texts:?}");
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Rounded Corner Image, White Stroke"));
            d.fields.insert("page".into(), json!("corners"));
        }
        let texts = frame(&mut app, &ctx);
        assert!(texts.iter().any(|x| x.trim() == "Rounded"), "{texts:?}");
        crate::dialogs::confirm(&mut app).unwrap();
        let name = "Rounded Corner Image, White Stroke";
        let doc = |app: &DesignApp| app.session.active().unwrap().doc.clone();
        let item = |app: &DesignApp, id: u64| doc(app).item(designcraft_doc::ItemId(id)).unwrap().clone();
        assert_eq!(item(&app, a).object_style, name, "applied to the selection");
        let os = doc(&app).styles.object_style(name).cloned().unwrap();
        assert!(os.corners.is_some() && os.effects.as_ref().unwrap().drop_shadow.on && os.stroke.as_ref().unwrap().weight == 2.5);

        let b = app.run("frame.create", json!({"rect": [200, 72, 300, 172], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        app.run("style.object.apply", json!({"name": name, "ids": [b]})).unwrap();
        open_edit(&mut app, name).unwrap();
        if let Some(d) = app.ui.dialog.as_mut() {
            assert_eq!(title(d), "Object Style Options");
            set(d, &["stroke", "weight"], json!(5.0));
            set(d, &["inc", "fill"], json!(false));
        }
        frame(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();
        assert_eq!((item(&app, a).stroke.weight, item(&app, b).stroke.weight), (5.0, 5.0));
        assert!(doc(&app).styles.object_style(name).unwrap().fill.is_none(), "Fill unchecked: no longer in the style");
        assert!(doc(&app).styles.object_style(name).unwrap().effects.as_ref().unwrap().drop_shadow.on, "effects kept");
    }

    /// The course's chapter 39: a new object style from a text frame nests the "Drop Cap First
    /// Paragraph" style (Paragraph Styles page), so the frame's text takes it; Object Style
    /// Options › Text Frame General Options then gives the style's frames insets and two columns.
    #[test]
    fn an_object_style_nests_a_paragraph_style_and_sets_text_frame_options() {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Body Text"})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Drop Cap First Paragraph", "nextStyle": "Body Text", "para": {"dropCapLines": 3}}))
            .unwrap();
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 336], "content": "text", "caret": false})).unwrap();
        let id = r["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        open_new(&mut app);
        let d = app.ui.dialog.as_ref().unwrap();
        assert!(included(d, "paragraph") && included(d, "textFrame"), "a text frame's categories are in a new style");
        assert_eq!(d.s("paragraphStyle"), "", "the paragraph style is left to choose");
        let texts = frame(&mut app, &ctx);
        for t in ["Paragraph Styles", "Text Frame General Options"] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Text Box Drop Cap"));
            d.fields.insert("page".into(), json!("paragraph"));
            d.fields.insert("paragraphStyle".into(), json!("Drop Cap First Paragraph"));
        }
        let texts = frame(&mut app, &ctx);
        for t in ["Paragraph Style:", "Apply Next Style", "Drop Cap First Paragraph"] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let doc = |app: &DesignApp| app.session.active().unwrap().doc.clone();
        let story = |app: &DesignApp| {
            let d = doc(app);
            let sid = d.item(designcraft_doc::ItemId(id)).unwrap().text_frame().unwrap().story;
            d.story(sid).unwrap().clone()
        };
        assert_eq!(doc(&app).item(designcraft_doc::ItemId(id)).unwrap().object_style, "Text Box Drop Cap");
        assert_eq!(story(&app).paras[0].style, "Drop Cap First Paragraph", "the frame's text takes the nested style");
        app.run("type.fillWithPlaceholder", json!({"frame": id})).unwrap();
        assert!(story(&app).paras.iter().all(|p| p.style == "Drop Cap First Paragraph"));

        open_edit(&mut app, "Text Box Drop Cap").unwrap();
        let texts = frame(&mut app, &ctx);
        assert!(texts.iter().any(|x| x.starts_with("Paragraph Styles: Drop Cap First Paragraph")), "Style Settings: {texts:?}");
        if let Some(d) = app.ui.dialog.as_mut() {
            assert_eq!(d.s("paragraphStyle"), "Drop Cap First Paragraph");
            d.fields.insert("page".into(), json!("textFrame"));
            for k in ["insetTop", "insetLeft", "insetBottom", "insetRight"] {
                d.fields.insert(k.into(), json!("4 pt"));
            }
            d.fields.insert("columns".into(), json!("2"));
        }
        let texts = frame(&mut app, &ctx);
        for t in ["Inset Spacing", "Vertical Justification", "Ignore Text Wrap"] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let o = doc(&app).item(designcraft_doc::ItemId(id)).unwrap().text_frame().unwrap().options.clone();
        assert_eq!((o.columns, o.inset), (2, [4.0; 4]));
        let os = doc(&app).styles.object_style("Text Box Drop Cap").cloned().unwrap();
        assert_eq!((os.paragraph_style.as_deref(), os.text_frame.map(|t| t.columns)), (Some("Drop Cap First Paragraph"), Some(2)));

        // A word for the gutter, or a fraction of a column, keeps the dialog open with the reason.
        for (field, value, reason) in [("gutter", "wide", "gutter"), ("columns", "2.5", "columns")] {
            open_edit(&mut app, "Text Box Drop Cap").unwrap();
            if let Some(d) = app.ui.dialog.as_mut() {
                d.fields.insert(field.into(), json!(value));
            }
            let e = crate::dialogs::confirm(&mut app).unwrap_err();
            assert!(e.contains(reason), "{e}");
            assert!(app.ui.dialog.is_some());
            app.ui.dialog = None;
        }
        let os = doc(&app).styles.object_style("Text Box Drop Cap").cloned().unwrap();
        assert_eq!(os.text_frame.map(|t| t.columns), Some(2), "2.5 columns changed nothing");
    }

    /// A refused OK (an empty name, a name another style has, in Object Style Options and in New
    /// Object Style) keeps the dialog and its edits, with the reason shown.
    #[test]
    fn a_refused_ok_keeps_the_dialog_open_with_the_reason() {
        let (mut app, _) = app_with_frame();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        app.run("style.object.create", json!({"name": "Taken", "fromSelection": false})).unwrap();
        app.run("style.object.create", json!({"name": "Mine", "fromSelection": false})).unwrap();
        open_new(&mut app);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("  "));
            set(d, &["stroke", "weight"], json!(7.0));
        }
        assert!(crate::dialogs::confirm(&mut app).is_err());
        let d = app.ui.dialog.as_ref().expect("still open");
        assert_eq!(d.s("status"), "Enter a style name.");
        assert_eq!(get(d, &["stroke", "weight"]), &json!(7.0), "the edits are kept");
        assert!(frame(&mut app, &ctx).iter().any(|t| t.trim() == "Enter a style name."));
        open_edit(&mut app, "Mine").unwrap();
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Taken"));
        }
        let e = crate::dialogs::confirm(&mut app).unwrap_err();
        assert!(e.contains("already named"), "{e}");
        assert!(app.ui.dialog.as_ref().is_some_and(|d| d.s("status") == e));
        // New Object Style with a name another style has makes no style under another name.
        app.ui.dialog = None;
        let count = |app: &DesignApp| app.session.active().unwrap().doc.styles.object.len();
        let before = count(&app);
        open_new(&mut app);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Taken"));
        }
        let e = crate::dialogs::confirm(&mut app).unwrap_err();
        assert!(e.contains("already named"), "{e}");
        assert!(app.ui.dialog.as_ref().is_some_and(|d| d.s("status") == e));
        assert_eq!(count(&app), before, "no style was made");
    }

    /// In a document measured in millimeters, a bare number in a Text Frame General Options field
    /// is millimeters, in New Object Style and in Object Style Options.
    #[test]
    fn bare_numbers_are_in_the_documents_ruler_unit() {
        let mut app = app_with_text_frame("millimeters");
        let mm = 72.0 / 25.4;
        let frame = |app: &DesignApp| app.session.active().unwrap().doc.styles.object_style("Metric").and_then(|s| s.text_frame.clone()).unwrap();
        open_new(&mut app);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("name".into(), json!("Metric"));
            for k in ["insetTop", "insetLeft", "insetBottom", "insetRight"] {
                d.fields.insert(k.into(), json!("6"));
            }
        }
        crate::dialogs::confirm(&mut app).unwrap();
        assert!(frame(&app).inset.iter().all(|v| (v - 6.0 * mm).abs() < 1e-6), "{:?}", frame(&app).inset);
        open_edit(&mut app, "Metric").unwrap();
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("columns".into(), json!("2"));
            d.fields.insert("gutter".into(), json!("5"));
        }
        crate::dialogs::confirm(&mut app).unwrap();
        assert!((frame(&app).gutter - 5.0 * mm).abs() < 1e-6, "{}", frame(&app).gutter);
    }

    /// The texts the dialog paints, once its size has settled (it grows over a few frames).
    fn frame(app: &mut DesignApp, ctx: &egui::Context) -> Vec<String> {
        let mut texts = Vec::new();
        for _ in 0..8 {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()));
            fn walk(s: egui::epaint::Shape, texts: &mut Vec<String>) {
                match s {
                    egui::epaint::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                    egui::epaint::Shape::Vec(v) => v.into_iter().for_each(|s| walk(s, texts)),
                    _ => {}
                }
            }
            texts.clear();
            let shapes = std::mem::take(&mut out.shapes);
            out.drop_without_applying_deltas();
            for c in shapes { walk(c.shape, &mut texts); }
        }
        texts
    }

