use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

use super::*;

/// A link to a panel whose flyout is open leaves the flyout open.
#[test]
fn a_panel_link_keeps_its_open_flyout() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    show_panel(&mut app, "stroke");
    assert_eq!(app.ui.docking_flyout.as_deref(), Some("stroke"));
    show_panel(&mut app, "stroke");
    assert!(crate::panel_docking::showing(&app, "stroke"));
}

#[test]
fn links_panel_can_choose_external_storage_and_undo_it() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let mut harness = Harness::builder().with_size(vec2(320.0, 300.0)).build_ui_state(|ui, app: &mut DesignApp| links_panel(app, ui), app);
    harness.get_by_label("Linked-only saves need the original files. Keep them with the document: moving, deleting or changing them can leave artwork missing or changed when reopening. Use Package to collect originals, or Embed for a self-contained file.");
    harness.get_by_label("Keep linked artwork external on save").click();
    harness.run_steps(2);
    assert!(harness.state().session.doc().unwrap().doc.settings.external_links);
    harness.state_mut().run("edit.undo", json!({})).unwrap();
    harness.run_steps(2);
    assert!(!harness.state().session.doc().unwrap().doc.settings.external_links);
}

fn language(app: &mut DesignApp) -> Value {
    text_attrs(app).unwrap()["chars"]["language"].clone()
}

fn linked_graphics(paths: &[Option<&str>]) -> (DesignApp, Vec<designcraft_doc::ItemId>, Vec<designcraft_doc::AssetId>) {
    use designcraft_doc::{Asset, AssetId, Content, Graphic, ItemId};
    use std::sync::Arc;
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "Linked graphics"})).unwrap();
    let mut items = Vec::new();
    let mut assets = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let x = 72 + index * 100;
        let id = ItemId(app.run("frame.create", json!({"rect": [x, 72, x + 72, 144]})).unwrap()["id"].as_u64().unwrap());
        let doc = Arc::make_mut(&mut app.session.doc_mut().unwrap().doc);
        let aid = AssetId(doc.alloc());
        doc.assets.insert(
            aid,
            Arc::new(Asset {
                id: aid,
                name: "photo.svg".into(),
                mime: "image/svg+xml".into(),
                link: path.map(str::to_owned),
                data: Arc::new(
                    br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#4488cc"/></svg>"##
                        .to_vec(),
                ),
                pixels: Some((32, 32)),
                ..Default::default()
            }),
        );
        doc.item_mut(id).unwrap().content = Content::Graphic(Graphic {
            layer_options: None,
            asset: aid,
            size: (72.0, 72.0),
            xf: designcraft_geom::Affine::IDENTITY,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        items.push(id);
        assets.push(aid);
    }
    (app, items, assets)
}

fn links_harness(app: DesignApp, width: f32) -> Harness<'static, DesignApp> {
    let mut ready = false;
    let mut h = Harness::builder().with_size(vec2(width, 600.0)).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), "");
                crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                ready = true;
                return;
            }
            links_panel(app, ui);
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    h.run_steps(3);
    h
}

/// With the caret in text, the Stroke panel offers the text's stroke options, not an empty panel.
/// The first and last line indents take negative values, as InDesign's (and the Paragraph
/// panel's) do; the left and right indents and the spacing don't.
#[test]
fn last_line_indent_may_be_negative() {
    for key in ["firstLineIndent", "lastLineIndent"] {
        assert!(super::para_field_min(key) < 0.0, "{key}");
    }
    for key in ["leftIndent", "rightIndent", "spaceBefore", "spaceAfter"] {
        assert_eq!(super::para_field_min(key), 0.0, "{key}");
    }
}

#[test]
fn stroke_panel_shows_the_text_stroke_options_while_text_is_selected() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
    let mut ready = false;
    let mut h = Harness::builder().with_size(vec2(320.0, 400.0)).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), "");
                crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                ready = true;
                return;
            }
            stroke_panel(app, ui);
        },
        app,
    );
    h.run_steps(3);
    assert!(h.query_by_label("Select an object.").is_none());
    h.get_by_label("Stroke Behind Fill");
}

/// As InDesign's: Edit Page takes the Page tool to the page in view, and Properties sets that
/// page alone; Back returns to the document.
#[test]
fn edit_page_shows_the_page_and_sets_its_size_alone() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 3, "facingPages": false})).unwrap();
    app.ui.dock_tab = "properties".into();
    let mut h = crate::test_window::open(app, vec2(1400.0, 900.0));
    crate::canvas::go_to_page(&mut h.state_mut().app, 1);
    h.run_steps(3);
    h.get_by_label("Edit Page").click();
    h.run_steps(3);
    let st = h.state().app.session.active().unwrap();
    assert_eq!((h.state().app.session.tool_id(), st.selection.page), ("page", Some(1)));
    assert!(h.query_by_label("Document Setup").is_none(), "no dialog");
    let _ = h.get_by_label("Page Size");
    let _ = h.get_by_label("Margins and Columns");
    h.get_by_label("Landscape").click();
    h.run_steps(3);
    let doc = &h.state().app.session.active().unwrap().doc;
    let size = |abs: usize| doc.page(abs).map(|p| (p.width, p.height)).unwrap();
    assert_eq!(size(1), (792.0, 612.0), "the page turns");
    assert_eq!((size(0), size(2)), ((612.0, 792.0), (612.0, 792.0)), "the others don't");
    h.get_by_label("Back").click();
    h.run_steps(3);
    let st = h.state().app.session.active().unwrap();
    assert_eq!((h.state().app.session.tool_id(), st.selection.page), ("selection", None));
    let _ = h.get_by_label("Edit Page");
}

#[test]
fn links_panel_distinguishes_same_named_sources_and_keeps_link_actions() {
    let first = "/project/assets/studio-a/photo.svg";
    let second = "/project/assets/studio-b/photo.svg";
    let (app, items, assets) = linked_graphics(&[Some(first), Some(second), None]);
    let mut h = links_harness(app, 320.0);
    assert_eq!(h.get_all_by_label("photo.svg").count(), 3);
    assert!(h.query_by_label(first).is_some(), "the first source path is visible in the row");
    h.get_by_label(second).click();
    h.run_steps(3);
    h.get_by_label("Go To").click();
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().selection.items, vec![items[1]]);
    h.get_by_label("Embed").click();
    h.run_steps(3);
    assert!(h.state().session.active().unwrap().doc.assets[&assets[1]].link.is_none());
    assert_eq!(h.state().session.active().unwrap().doc.assets[&assets[0]].link.as_deref(), Some(first));
    assert!(h.query_by_label(second).is_none(), "an embedded graphic no longer shows a source path");
    assert_eq!(h.get_all_by_label("Embedded").count(), 2);
    assert_eq!(h.get_all_by_label("photo.svg").count(), 3);
    h.get_by_label(first).click();
    h.run_steps(3);
    h.get_by_label("Go To").click();
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().selection.items, vec![items[0]]);
}

#[test]
fn links_panel_keeps_long_source_paths_inside_a_narrow_panel() {
    let path = "/project/assets/studio-a/very-long-folder-name/another-long-folder-name/photo.svg";
    let (app, _, _) = linked_graphics(&[Some(path)]);
    let mut h = links_harness(app, 240.0);
    let source = h.get_by_label(path).rect();
    assert!(source.width() < 150.0, "the source is truncated to its column: {source:?}");
    assert!(source.right() <= 240.0, "the source stays inside the narrow panel: {source:?}");
    assert!(h.get_by_label("Page · PPI").rect().right() <= 240.0);
    h.get_by_label(path).click();
    h.run_steps(3);
    assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(text) if text.galley.job.text == path && !text.galley.elided && text.visual_bounding_rect().right() <= 240.0)), "the selected path is painted in full and wraps inside the panel");
}

#[test]
fn properties_character_section_sets_the_language() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "bunun", "caret": true})).unwrap();
    app.run("edit.selectAll", json!({})).unwrap();
    assert_ne!(language(&mut app), "French");
    let mut h = Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            // The panel's fonts are installed before its first frame.
            let id = egui::Id::unique("test_fonts");
            if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(id, true));
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(3);
    assert!(h.query_by_label("Language").is_none(), "under •••, as in InDesign");
    h.get_all_by_label("More Options").next().unwrap().click();
    h.run_steps(3);
    h.get_by_label("Language").click();
    h.run_steps(2);
    h.get_by_label("French").click();
    h.run_steps(2);
    assert_eq!(language(h.state_mut()), "French");
}

/// Properties › Character and Paragraph •••, as in InDesign: the sections expand in place,
/// with the scales, baseline shift, skew, language, OpenType and the case buttons, and the
/// indents, spacing and drop caps.
/// Properties › Paragraph ••• on a First Line Only paragraph: Align to Baseline Grid shows
/// pressed, and a click turns alignment off instead of making it All Lines.
#[test]
fn more_options_grid_button_keeps_first_line_only() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "If fine plants", "caret": true})).unwrap();
    app.run("edit.selectAll", json!({})).unwrap();
    app.run("type.para", json!({"attrs": {"gridAlign": "firstLineOnly"}})).unwrap();
    let mut h = panel_harness(app, show);
    h.run_steps(3);
    h.get_all_by_label("More Options").nth(1).unwrap().click();
    h.run_steps(3);
    assert_eq!(h.get_by_label("Align to Baseline Grid").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
    assert_eq!(text_attrs(h.state_mut()).unwrap()["para"]["gridAlign"], "firstLineOnly");
    h.get_by_label("Align to Baseline Grid").click();
    h.run_steps(3);
    assert_eq!(text_attrs(h.state_mut()).unwrap()["para"]["gridAlign"], "none");
}

#[test]
fn more_options_expand_character_and_paragraph_in_place() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "If fine plants", "caret": true})).unwrap();
    app.run("edit.selectAll", json!({})).unwrap();
    let mut h = panel_harness(app, show);
    h.run_steps(3);
    for l in ["Vertical Scale", "Small Caps", "Drop Cap Number of Lines", "Space Before"] {
        assert!(h.query_by_label(l).is_none(), "{l} hidden until •••");
    }
    h.get_all_by_label("More Options").next().unwrap().click();
    h.run_steps(3);
    h.get_by_label("Small Caps").click();
    h.run_steps(3);
    let _ = h.get_by_label("OpenType");
    let _ = h.get_by_label("Language");
    h.get_all_by_label("More Options").nth(1).unwrap().click();
    h.run_steps(3);
    let _ = h.get_by_label("Hyphenate");
    h.get_by_label("Align to Baseline Grid").click();
    h.run_steps(3);
    let a = text_attrs(h.state_mut()).unwrap();
    assert_eq!(a["chars"]["capitalization"], "smallCaps");
    assert_eq!(a["para"]["gridAlign"], "allLines");
    // ••• again folds the section.
    h.get_all_by_label("More Options").next().unwrap().click();
    h.run_steps(3);
    assert!(h.query_by_label("Small Caps").is_none());
}

/// The Paragraph panel on a First Line Only paragraph: Align to Baseline Grid shows pressed
/// and keeps First Line Only, and the panel menu checks Only Align First Line to Grid.
#[test]
fn paragraph_panel_keeps_first_line_only() {
    let mut app = text_app();
    app.run("type.para", json!({"attrs": {"gridAlign": "firstLineOnly"}})).unwrap();
    let mut h = panel_harness(app, paragraph_panel);
    h.run_steps(3);
    assert_eq!(h.get_by_label("Align to Baseline Grid").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
    h.get_by_label("Align to Baseline Grid").click();
    h.run_steps(3);
    assert_eq!(text_attrs(h.state_mut()).unwrap()["para"]["gridAlign"], "firstLineOnly");
    h.get_by_label("☰").click();
    h.run_steps(3);
    let _ = h.get_by_label("✓ Only Align First Line to Grid");
}

#[test]
fn paragraph_panel_menu_toggles_ignore_optical_margin() {
    let mut h = panel_harness(text_app(), paragraph_panel);
    h.run_steps(3);
    h.get_by_label("☰").click();
    h.run_steps(3);
    h.get_by_label("   Ignore Optical Margin").click();
    h.run_steps(3);
    assert_eq!(text_attrs(h.state_mut()).unwrap()["para"]["ignoreOpticalMargin"], true);
    h.get_by_label("☰").click();
    h.run_steps(3);
    h.get_by_label("✓ Ignore Optical Margin").click();
    h.run_steps(3);
    assert_eq!(text_attrs(h.state_mut()).unwrap()["para"]["ignoreOpticalMargin"], false);
}

#[test]
fn paragraph_panel_menu_define_lists_opens_its_dialog() {
    let mut h = panel_harness(text_app(), paragraph_panel);
    h.run_steps(3);
    h.get_by_label("☰").click();
    h.run_steps(3);
    h.get_by_label("Define Lists…").click();
    h.run_steps(3);
    assert!(h.state_mut().ui.dialog.is_some(), "Define Lists");
}

#[test]
fn paragraph_panel_is_laid_out_as_indesigns_with_its_panel_menu() {
    let mut h = panel_harness(text_app(), paragraph_panel);
    h.run_steps(3);
    for l in [
        "Left Indent",
        "Right Indent",
        "First Line Left Indent",
        "Last Line Right Indent",
        "Space Before",
        "Space After",
        "Space Between Paragraphs Using Same Style",
        "Drop Cap Number of Lines",
        "Drop Cap One or More Characters",
        "Paragraph Shading",
        "Paragraph Border",
        "Hyphenate",
        "Do Not Align to Baseline Grid",
    ] {
        let _ = h.get_by_label(l);
    }
    let para = |h: &mut Harness<'static, DesignApp>| text_attrs(h.state_mut()).unwrap()["para"].clone();
    // Space Between Paragraphs Using Same Style starts at Ignore; a value is set, and emptying
    // the field ignores it again.
    assert!(para(&mut h)["sameParaStyleSpacing"].is_null());
    let field = h.get_all_by_role(egui::accesskit::Role::TextInput).nth(6).unwrap();
    field.focus();
    field.type_text("6 pt");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(para(&mut h)["sameParaStyleSpacing"], 6.0);
    h.get_all_by_role(egui::accesskit::Role::TextInput).nth(6).unwrap().focus();
    h.run_steps(1);
    for _ in 0..8 {
        h.key_press(egui::Key::Backspace);
    }
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(para(&mut h)["sameParaStyleSpacing"].is_null(), "Ignore");
    // Shading on, then the grid buttons and Only Align First Line to Grid from the menu.
    h.get_by_label("Shading").click();
    h.run_steps(3);
    assert_eq!(para(&mut h)["shadingOn"], true);
    h.get_by_label("Align to Baseline Grid").click();
    h.run_steps(3);
    assert_eq!(para(&mut h)["gridAlign"], "allLines");
    h.get_by_label("☰").click();
    h.run_steps(3);
    for l in
        ["   Balance Ragged Lines", "Keep Options…", "Drop Caps and Nested Styles…", "GREP Styles…", "Define Lists…", "Restart/Continue Numbering"]
    {
        let _ = h.get_by_label(l);
    }
    h.get_by_label("   Only Align First Line to Grid").click();
    h.run_steps(3);
    assert_eq!(para(&mut h)["gridAlign"], "firstLineOnly");
    h.get_by_label("Do Not Align to Baseline Grid").click();
    h.run_steps(3);
    assert_eq!(para(&mut h)["gridAlign"], "none");
    // Keep Options… opens the selected paragraphs' Keep Options, and OK applies them.
    h.get_by_label("☰").click();
    h.run_steps(3);
    h.get_by_label("Keep Options…").click();
    h.run_steps(3);
    let app = h.state_mut();
    let d = app.ui.dialog.as_mut().expect("Keep Options");
    assert_eq!((d.id.as_str(), d.s("section").as_str()), ("paragraphLocal", "keep"));
    d.fields.insert("p.keepWithNext".into(), json!(2));
    crate::dialogs::confirm(app).unwrap();
    assert_eq!(para(&mut h)["keepWithNext"], 2);
}

fn kerning(app: &mut DesignApp) -> Value {
    text_attrs(app).unwrap()["chars"]["kerning"].clone()
}

/// A harness drawing `panel` for `app` (fonts installed before the first frame).
fn panel_harness(app: DesignApp, panel: fn(&mut DesignApp, &mut egui::Ui)) -> Harness<'static, DesignApp> {
    Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
        move |ui, app: &mut DesignApp| {
            let id = egui::Id::unique("test_fonts");
            if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(id, true));
                return;
            }
            panel(app, ui);
        },
        app,
    )
}

fn text_app() -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "AVATAR", "caret": true})).unwrap();
    app.run("edit.selectAll", json!({})).unwrap();
    app
}

#[test]
fn properties_kerning_menu_sets_zero_kerning() {
    let mut h = panel_harness(text_app(), show);
    h.run_steps(3);
    assert_eq!(kerning(h.state_mut())["kind"], "metrics");
    h.get_by_label("Kerning").click();
    h.run_steps(2);
    h.get_by_label("0").click();
    h.run_steps(2);
    assert_eq!(kerning(h.state_mut())["kind"], "none");
}

fn undo_steps(app: &DesignApp) -> usize {
    app.session.active().unwrap().history.undo.len()
}

fn font_style(app: &mut DesignApp) -> String {
    text_attrs(app).unwrap()["chars"]["fontStyle"].as_str().unwrap().to_string()
}

/// The Digits menu uses the reference names (documented as Arabic, Hindi and Farsi), which
/// are translated, with a sample of each script's figures.
#[test]
fn digits_choices_use_the_reference_names() {
    assert_eq!(DIGIT_OPTIONS.map(|o| o.1), ["Default", "Arabic", "Hindi", "Farsi", "Native"]);
    for (lang, _) in crate::i18n::LANGUAGES {
        let tr = |s| crate::i18n::tr(lang, s);
        let shown: Vec<String> = DIGIT_OPTIONS.iter().map(|o| digit_label(lang, *o)).collect();
        let expected = [
            tr("Default").to_string(),
            format!("{}  0123", tr("Arabic")),
            format!("{}  ٠١٢٣", tr("Hindi")),
            format!("{}  ۰۱۲۳", tr("Farsi")),
            tr("Native").to_string(),
        ];
        assert_eq!(shown, expected, "{lang}");
    }
}

#[test]
fn axes_with_an_inverted_or_missing_range_get_no_slider() {
    let axis = |tag: &str, min: f32, max: f32| (tag.to_string(), tag.to_string(), min, min, max);
    let kept = usable_axes(vec![axis("wght", 100.0, 900.0), axis("wdth", 125.0, 75.0), axis("opsz", f32::NAN, 72.0), axis("slnt", 0.0, 0.0)]);
    let tags: Vec<&str> = kept.iter().map(|a| a.0.as_str()).collect();
    assert_eq!(tags, ["wght", "slnt"]);
}

#[test]
fn variable_font_style_keeps_two_decimals_at_most() {
    let values = [("wght".to_string(), 512.0_f32), ("wdth".to_string(), 1.25), ("opsz".to_string(), 0.4801)];
    assert_eq!(variable_font_style("Regular", &values), "Regular {wght:512,wdth:1.25,opsz:0.48}");
    assert_eq!(axis_step(100.0, 900.0), (1.0, 0));
    assert_eq!(axis_step(0.48, 3.2), (0.01, 2));
}

#[test]
fn a_live_preview_held_over_many_frames_is_one_undo_step_once_let_go() {
    let mut app = text_app();
    let steps = undo_steps(&app);
    let size = |app: &mut DesignApp| text_attrs(app).unwrap()["chars"]["size"].as_f64().unwrap();
    // A drag: a preview on most frames, held without moving on others.
    for v in [20.0, 24.0, 30.0] {
        app.settle_live_preview();
        app.live_preview("Size", "type.char", json!({"attrs": {"size": v}}));
        app.settle_live_preview();
        app.hold_live_preview();
    }
    assert_eq!(size(&mut app), 30.0, "the page shows the last preview");
    assert_eq!(undo_steps(&app), steps, "nothing is undoable while held");
    // Let go: a frame without a hold, then the frame after it commits the preview.
    app.settle_live_preview();
    assert_eq!(undo_steps(&app), steps, "the last held frame keeps it open");
    app.settle_live_preview();
    assert_eq!(undo_steps(&app), steps + 1);
    assert!(app.session.active().unwrap().interaction.is_none());
    app.settle_live_preview();
    assert_eq!(undo_steps(&app), steps + 1, "committed once");
    // Later edits get their own undo steps.
    app.run("type.char", json!({"attrs": {"tracking": 50}})).unwrap();
    assert_eq!(undo_steps(&app), steps + 2);
}

/// A system variable font with two or more axes (macOS ships some), loaded into the shared fonts.
fn system_variable_font() -> Option<&'static str> {
    let candidates = [("/System/Library/Fonts/Supplemental/Skia.ttf", "Skia"), ("/System/Library/Fonts/NewYork.ttf", "New York")];
    candidates.into_iter().find_map(|(path, family)| {
        let bytes = std::fs::read(path).ok()?;
        let db = designcraft_fonts::FontDb::global();
        db.add_font(bytes);
        let style = db.styles(family).first().cloned()?;
        (db.axes(family, &style).len() >= 2).then_some(family)
    })
}

#[test]
fn dragging_one_of_several_axis_sliders_is_one_undo_step() {
    let Some(family) = system_variable_font() else {
        eprintln!("skipped: no variable font with two axes on this system");
        return;
    };
    let mut app = text_app();
    super::super::apply_font(&mut app, &super::super::FontPick { family: family.into(), style: None });
    let before = font_style(&mut app);
    let steps = undo_steps(&app);
    // The app commits a preview no panel held at the start of each frame (DesignApp::ui).
    let mut h = Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let id = egui::Id::unique("test_fonts");
            if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(id, true));
                return;
            }
            app.settle_live_preview();
            character_panel(app, ui);
        },
        app,
    );
    h.run_steps(3);
    let sliders = h.get_all_by_role(egui::accesskit::Role::Slider).map(|n| n.rect()).collect::<Vec<_>>();
    assert!(sliders.len() >= 2, "one slider per axis: {sliders:?}");
    let track = sliders[0];
    let y = track.center().y;
    h.hover_at(egui::pos2(track.left() + 2.0, y));
    h.run_steps(1);
    h.drag_at(egui::pos2(track.left() + 2.0, y));
    h.run_steps(1);
    for k in 1..=10 {
        h.hover_at(egui::pos2(track.left() + track.width() * k as f32 / 10.0, y));
        h.run_steps(1);
        assert_eq!(undo_steps(h.state()), steps, "no undo step while dragging (frame {k})");
    }
    let previewed = font_style(h.state_mut());
    assert_ne!(previewed, before, "the drag previews on the page");
    h.drop_at(egui::pos2(track.right(), y));
    h.run_steps(3);
    assert_eq!(undo_steps(h.state()), steps + 1, "the whole drag is one undo step");
    assert_eq!(font_style(h.state_mut()), previewed);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(font_style(h.state_mut()), before);
}

#[test]
fn character_panel_has_kerning_and_takes_a_typed_manual_value() {
    let mut h = panel_harness(text_app(), character_panel);
    h.run_steps(3);
    // The caption and the control share the name: click the control.
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Kerning").click();
    h.run_steps(2);
    // Address the manual-value editor by semantics, not tree traversal order.
    let field = h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Manual Kerning");
    field.focus();
    field.type_text("-40");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(kerning(h.state_mut()), json!({"kind": "manual", "value": -40.0}));
}

#[test]
fn a_manual_kerning_value_at_a_caret_kerns_the_pair() {
    let mut app = text_app();
    let story = app.session.active().unwrap().selection.text.unwrap().story;
    app.run("text.select", json!({"story": story.0, "anchor": 1, "focus": 1})).unwrap();
    let mut h = panel_harness(app, character_panel);
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Kerning").click();
    h.run_steps(2);
    let field = h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Manual Kerning");
    field.focus();
    field.type_text("-80");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let st = h.state_mut().session.active().unwrap().doc.story(story).unwrap().format_after(0).over.kerning;
    assert_eq!(st, Some(designcraft_doc::Kerning::Manual(-80.0)), "A of the A|V pair");
    assert_eq!(kerning(h.state_mut()), json!({"kind": "manual", "value": -80.0}));
}

#[test]
fn clicking_a_swatch_chip_in_the_appearance_fill_menu_applies_it() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    // A graphic frame: the Appearance section shows a single Fill chip.
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let before = app.session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.swatch.clone();
    assert_ne!(before, "[Paper]");
    let mut h = Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let id = egui::Id::unique("test_fonts");
            if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(id, true));
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(3);
    // Open the Fill menu from the Appearance chip.
    h.get_by_label("Fill swatch").click();
    h.run_steps(3);
    // Click the *chip* (the row's left edge), not the name.
    let name = "[Paper]".to_string();
    let row = h.get_by_label(&name).rect();
    // The row spans the menu (chip + name), not just the name text.
    assert!(row.width() > 150.0, "the row is the full menu width, chip included: {row:?}");
    let p = row.min + vec2(8.0, row.height() / 2.0);
    h.hover_at(p);
    h.drag_at(p);
    h.drop_at(p);
    h.run_steps(4);
    let after = h.state().session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.swatch.clone();
    assert_eq!(after, name, "clicking the swatch chip applies it");
}

fn panel(app: DesignApp) -> Harness<'static, DesignApp> {
    let mut h = Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let id = egui::Id::unique("test_fonts");
            if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(id, true));
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// The course's chapter 21: Properties' Bulleted List button gives the selected paragraphs
/// bullets with InDesign's hanging indent; pressed again, both go.
#[test]
fn the_bulleted_list_button_applies_a_hanging_indent() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let f = app
        .run(
            "frame.create",
            json!({"rect": [72, 72, 300, 200], "content": "text", "text": "4 eggs
1 cup milk"}),
        )
        .unwrap();
    app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 15})).unwrap();
    let mut h = panel(app);
    h.get_by_label("Bulleted List").click();
    h.run_steps(3);
    let a = h.state_mut().run("type.selectionAttrs", json!({})).unwrap();
    assert_eq!(
        (a["para"]["listType"].as_str(), a["para"]["leftIndent"].as_f64(), a["para"]["firstLineIndent"].as_f64()),
        (Some("bullets"), Some(18.0), Some(-18.0))
    );
    assert_eq!(crate::menus::checked(h.state(), "list.bullets", &json!({})), Some(true), "Type › Apply Bullets shows it");
    h.get_by_label("Bulleted List").click();
    h.run_steps(3);
    let a = h.state_mut().run("type.selectionAttrs", json!({})).unwrap();
    assert_eq!((a["para"]["listType"].as_str(), a["para"]["leftIndent"].as_f64()), (Some("none"), Some(0.0)));
}

#[test]
fn several_shapes_show_pathfinder_and_intersect_combines_them() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [100, 100, 300, 300], "shape": "ellipse"})).unwrap()["id"].clone();
    let b = app.run("frame.create", json!({"rect": [200, 100, 300, 200]})).unwrap()["id"].clone();
    // One shape: no Pathfinder section.
    app.run("selection.set", json!({"ids": [a]})).unwrap();
    let mut h = panel(app);
    assert!(h.query_by_label("Pathfinder").is_none());
    // Two: the section and its five buttons; Intersect leaves one shape, the ellipse's quarter.
    h.state_mut().run("selection.set", json!({"ids": [a, b]})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Pathfinder").is_some());
    for op in ["Add", "Subtract", "Intersect", "Exclude Overlap", "Minus Back"] {
        assert!(h.query_by_label(op).is_some(), "{op}");
    }
    h.get_by_label("Intersect").click();
    h.run_steps(3);
    let doc = &h.state().session.active().unwrap().doc;
    let items = &doc.spreads[0].items;
    assert_eq!(items.len(), 1, "one shape left");
    let r = items[0].bounds();
    assert!((r.x0 - 200.0).abs() < 0.5 && (r.y1 - 200.0).abs() < 0.5 && (r.x1 - 300.0).abs() < 0.5, "{r:?}");
}

const CYAN: &str = "C=100 M=0 Y=0 K=0";

#[test]
fn the_fill_menu_sets_a_tint() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("object.fill", json!({"swatch": CYAN, "ids": [id]})).unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let mut h = panel(app);
    h.get_by_label("Fill swatch").click();
    h.run_steps(3);
    // A frame that isn't a text frame: no Apply To.
    assert!(h.query_by_label("Apply To:").is_none());
    // The field on the Tint row (Opacity also reads 100%).
    let row = h.get_by_label("Tint:").rect();
    h.query_all_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("100%"))
        .find(|n| (n.rect().center().y - row.center().y).abs() < 8.0)
        .expect("the Tint field")
        .click();
    h.run_steps(2);
    h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
    h.event(egui::Event::Text("25".into()));
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let fill = h.state().session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.clone();
    assert_eq!((fill.swatch.as_str(), fill.tint), (CYAN, 0.25), "the tint, on the same swatch");
}

#[test]
fn a_tint_on_several_objects_keeps_each_ones_swatch() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let b = app.run("frame.create", json!({"rect": [220, 72, 300, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("object.fill", json!({"swatch": CYAN, "ids": [a]})).unwrap();
    app.run("object.fill", json!({"swatch": "[Black]", "ids": [b]})).unwrap();
    app.run("selection.set", json!({"ids": [a, b]})).unwrap();
    let mut h = panel(app);
    h.get_by_label("Fill swatch").click();
    h.run_steps(3);
    let row = h.get_by_label("Tint:").rect();
    h.query_all_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("100%"))
        .find(|n| (n.rect().center().y - row.center().y).abs() < 8.0)
        .expect("the Tint field")
        .click();
    h.run_steps(2);
    h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
    h.event(egui::Event::Text("25".into()));
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let doc = &h.state().session.active().unwrap().doc;
    let fill = |id: u64| doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.clone();
    assert_eq!((fill(a).swatch.as_str(), fill(a).tint), (CYAN, 0.25));
    assert_eq!((fill(b).swatch.as_str(), fill(b).tint), ("[Black]", 0.25), "the second keeps its own swatch");
}

#[test]
fn apply_to_text_colours_a_text_frames_text_not_the_frame() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Heading"})).unwrap();
    let (id, story) = (r["id"].as_u64().unwrap(), designcraft_doc::StoryId(r["story"].as_u64().unwrap()));
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let mut h = panel(app);
    h.get_by_label("Fill swatch").click();
    h.run_steps(3);
    h.get_by_label("Text").click();
    h.run_steps(3);
    h.get_by_label(CYAN).click();
    h.run_steps(3);
    let doc = &h.state().session.active().unwrap().doc;
    assert_eq!(doc.story(story).unwrap().char_format_at(0).over.fill.as_deref(), Some(CYAN), "the text is cyan");
    assert_eq!(doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.swatch, "[None]", "the frame isn't");
}

#[test]
fn transform_flips_and_rotates_from_properties() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let mut h = panel(app);
    let xf = |h: &Harness<'_, DesignApp>| h.state().session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().xf;
    // Flip Horizontal mirrors the frame (a negative determinant); Flip Vertical as well brings it
    // back to a half turn, as in the tutorial's petal.
    h.get_by_label("Flip Horizontal").click();
    h.run_steps(3);
    assert!(xf(&h).determinant() < 0.0, "{:?}", xf(&h));
    h.get_by_label("Flip Vertical").click();
    h.run_steps(3);
    let c = xf(&h).as_coeffs();
    assert!((c[0] + 1.0).abs() < 1e-9 && (c[3] + 1.0).abs() < 1e-9, "a half turn: {c:?}");
    let info = h.state_mut().session.execute("transform.info", &json!({})).unwrap();
    assert!((info["rotation"].as_f64().unwrap().abs() - 180.0).abs() < 1e-6, "{info}");
    // A quarter turn clockwise from there.
    h.get_by_label("Rotate 90° Clockwise").click();
    h.run_steps(3);
    let info = h.state_mut().session.execute("transform.info", &json!({})).unwrap();
    assert!((info["rotation"].as_f64().unwrap() - 90.0).abs() < 1e-6, "{info}");
    // The angle and scale fields are there.
    assert!(h.query_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("90°")).is_some());
}
