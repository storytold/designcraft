use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

use super::object::transform_anchors;
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

/// The course's chapter 23: a double-click on Properties › Appearance › Fill opens the Color
/// Picker with the fill's colour, as InDesign's (a click opens the swatches). For selected
/// text, the text's Fill chip opens it for the text.
#[test]
fn double_clicking_a_fill_chip_opens_the_color_picker() {
    let double_click = |h: &mut Harness<'_, DesignApp>, label: &str| {
        let at = h.get_by_label(label).rect().center();
        h.hover_at(at);
        h.step();
        for pressed in [true, false, true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            h.step();
        }
        h.run_steps(3);
    };
    // Frames 1 ms apart, so two clicks make a double-click.
    let panel = |app: DesignApp| {
        let mut h = Harness::builder().with_size(vec2(320.0, 1400.0)).with_step_dt(0.001).build_ui_state(
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
    };
    let frame = || {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        app.run("object.fill", json!({"swatch": "C=0 M=0 Y=100 K=0"})).unwrap();
        app
    };
    let mut h = panel(frame());
    double_click(&mut h, "Fill swatch");
    assert!(h.query_by_label("Tint:").is_none(), "the swatches popup isn't left open too");
    let d = h.state().ui.dialog.clone().expect("the Color Picker");
    assert_eq!((d.id.as_str(), d.s("target"), d.s("model"), d.b("text")), ("colorPicker", "fill".into(), "cmyk".into(), false));
    assert_eq!(d.fields["cmyk"], json!([0.0, 0.0, 100.0, 0.0]));
    let mut h = panel(frame());
    double_click(&mut h, "Stroke swatch");
    assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.s("target")), Some("stroke".into()));
    // Selected text: its own Fill chip.
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let f = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Lemon"})).unwrap();
    app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 5})).unwrap();
    let mut h = panel(app);
    double_click(&mut h, "Text fill swatch");
    let d = h.state().ui.dialog.clone().expect("the Color Picker");
    assert_eq!((d.id.as_str(), d.s("target"), d.b("text")), ("colorPicker", "fill".into(), true));
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

/// The course's chapter 21: Properties › Bullets and Numbering › Options opens the Bullets
/// and Numbering dialog, as InDesign's, not the Paragraph panel.
#[test]
fn bullets_options_open_the_bullets_and_numbering_dialog() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let f = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "4 eggs"})).unwrap();
    app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 6})).unwrap();
    let mut h = panel(app);
    h.get_by_label("Options").click();
    h.run_steps(3);
    let d = h.state().ui.dialog.clone().expect("a dialog");
    assert_eq!((d.id.as_str(), d.s("section").as_str()), ("paragraphLocal", "bullets"));
    assert_ne!(h.state().ui.open_panel.as_deref(), Some("paragraph"));
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

/// The arrow beside Opacity opens a slider, as in InDesign: a drag shows the opacity live and
/// is one undo step; the Opacity link still opens the Effects panel.
#[test]
fn the_opacity_arrow_opens_a_slider_and_a_drag_is_one_undo_step() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "shape": "rectangle"})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let opacity = |app: &DesignApp| app.session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().opacity;
    let steps = undo_steps(&app);
    let mut h = Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let id = egui::Id::unique("test_fonts");
            if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(id, true));
                return;
            }
            app.settle_live_preview();
            show(app, ui);
        },
        app,
    );
    h.run_steps(3);
    assert!(h.query_by_role(egui::accesskit::Role::Slider).is_none());
    h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some("Opacity")).click();
    h.run_steps(3);
    assert!(
        h.query_by(|n| n.role() == egui::accesskit::Role::Slider && n.label().as_deref() == Some("Opacity")).is_some(),
        "assistive technology names the slider"
    );
    let track = h.get_by_role(egui::accesskit::Role::Slider).rect();
    let y = track.center().y;
    h.hover_at(egui::pos2(track.right() - 1.0, y));
    h.run_steps(1);
    h.drag_at(egui::pos2(track.right() - 1.0, y));
    h.run_steps(1);
    for k in 1..=5 {
        h.hover_at(egui::pos2(track.right() - track.width() * 0.05 * k as f32, y));
        h.run_steps(1);
        assert_eq!(undo_steps(h.state()), steps, "no undo step while dragging");
    }
    let previewed = opacity(h.state());
    assert!(previewed < 0.9 && previewed > 0.5, "the drag shows the opacity live: {previewed}");
    // Let go further along than the last move: the release frame changes the value too.
    h.drop_at(egui::pos2(track.right() - track.width() * 0.3, y));
    h.run_steps(3);
    assert_eq!(undo_steps(h.state()), steps + 1, "the whole drag is one undo step");
    assert!(opacity(h.state()) < previewed, "the release position is applied: {previewed} → {}", opacity(h.state()));
    assert_ne!(h.state().ui.open_panel.as_deref(), Some("effects"), "the arrow doesn't open Effects");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(opacity(h.state()), 1.0);
}

/// Properties › Frame Fitting has Auto-Fit and an Options button, as InDesign's: ticking
/// Auto-Fit makes the picture refit when its frame is resized.
#[test]
fn properties_frame_fitting_sets_auto_fit_and_opens_its_options() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 272, 172]})).unwrap()["id"].as_u64().unwrap();
    let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
    app.run("file.place", json!({"base64": designcraft_engine::cmd::base64_encode(&png), "name": "p.png", "frame": id})).unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let graphic = |app: &DesignApp| app.session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().graphic().unwrap().clone();
    assert_eq!(graphic(&app).auto_fit, designcraft_doc::Fitting::None);
    let mut h = panel(app);
    // A checkbox its section disables says so: Invert, with no text wrap.
    use egui_kittest::kittest::NodeT;
    assert!(h.get_by_label("Invert").accesskit_node().is_disabled());
    assert!(!h.get_by_label("Auto-Fit").accesskit_node().is_disabled());
    h.get_by_label("Auto-Fit").click();
    h.run_steps(3);
    assert_ne!(graphic(h.state()).auto_fit, designcraft_doc::Fitting::None, "Auto-Fit is on");
    // The frame resized: the picture follows it.
    h.state_mut().run("transform.resize", json!({"from": [72, 72, 272, 172], "to": [72, 72, 372, 272]})).unwrap();
    let g = graphic(h.state());
    let r = g.xf.transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1));
    assert!(r.width() > 250.0, "refitted to the larger frame: {r:?}");
    h.get_by_label("Options").click();
    h.run_steps(3);
    assert!(h.state().ui.dialog.is_some(), "Frame Fitting Options opens");
}

/// Properties › Text Wrap shows the four offsets under the wrap buttons, as InDesign's
/// Properties panel does: linked at first, so one value sets all four; unlinked, one side.
#[test]
fn properties_text_wrap_shows_and_sets_the_offsets() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "shape": "rectangle"})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    app.run("object.textWrap", json!({"mode": "boundingBox", "offset": 0})).unwrap();
    let offsets = |app: &DesignApp| app.session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().wrap.offsets;
    let mut h = panel(app);
    // The fields between the Text Wrap heading and Quick Actions: Top, Left, Bottom, Right.
    let fields = |h: &Harness<'_, DesignApp>| {
        let (top, bottom) = (h.get_by_label("Text Wrap").rect().bottom(), h.get_by_label("Quick Actions").rect().top());
        let mut v: Vec<egui::Rect> =
            h.get_all_by_role(egui::accesskit::Role::TextInput).map(|n| n.rect()).filter(|r| r.center().y > top && r.center().y < bottom).collect();
        v.sort_by(|a, b| (a.top(), a.left()).partial_cmp(&(b.top(), b.left())).unwrap());
        v
    };
    let f = fields(&h);
    assert_eq!(f.len(), 4, "four offsets: {f:?}");
    let type_in = |h: &mut Harness<'_, DesignApp>, at: egui::Rect, text: &str| {
        h.hover_at(at.center());
        h.drag_at(at.center());
        h.drop_at(at.center());
        h.run_steps(2);
        h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.event(egui::Event::Text(text.into()));
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
    };
    // Linked: the top offset sets all four.
    type_in(&mut h, f[0], "12pt");
    assert_eq!(offsets(h.state()), [12.0; 4]);
    // Unlinked: the bottom one alone.
    let (top, bottom) = (h.get_by_label("Text Wrap").rect().bottom(), h.get_by_label("Quick Actions").rect().top());
    h.get_all_by_label("Make all settings the same").find(|n| n.rect().center().y > top && n.rect().center().y < bottom).unwrap().click();
    h.run_steps(2);
    let f = fields(&h);
    type_in(&mut h, f[2], "6pt");
    assert_eq!(offsets(h.state()), [12.0, 12.0, 6.0, 12.0], "[top, left, bottom, right]");
    // Two objects with different offsets: a side typed in changes that side on each, and
    // each keeps its other three.
    let other = h.state_mut().run("frame.create", json!({"rect": [72, 300, 300, 400], "shape": "rectangle"})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().run("object.textWrap", json!({"mode": "contour", "offset": [3, 4, 5, 6], "ids": [other]})).unwrap();
    let of =
        |h: &Harness<'_, DesignApp>, id: u64| h.state().session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().wrap.offsets;
    h.state_mut().run("selection.set", json!({"ids": [id, other]})).unwrap();
    h.run_steps(3);
    let f = fields(&h);
    type_in(&mut h, f[0], "9pt");
    assert_eq!((of(&h, id), of(&h, other)), ([9.0, 12.0, 6.0, 12.0], [9.0, 4.0, 5.0, 6.0]), "the top alone, on each");
    let mode =
        |h: &Harness<'_, DesignApp>, id: u64| h.state().session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().wrap.mode;
    assert_eq!((mode(&h, id), mode(&h, other)), (designcraft_doc::WrapMode::BoundingBox, designcraft_doc::WrapMode::Contour), "each keeps its mode");
    // An object whose offsets are the same starts linked, though the link was off before.
    let third = h.state_mut().run("frame.create", json!({"rect": [320, 72, 500, 200], "shape": "rectangle"})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().run("object.textWrap", json!({"mode": "boundingBox", "offset": 4, "ids": [third]})).unwrap();
    h.state_mut().run("selection.set", json!({"ids": [third]})).unwrap();
    h.run_steps(3);
    let f = fields(&h);
    type_in(&mut h, f[2], "8pt");
    assert_eq!(of(&h, third), [8.0; 4], "linked again for this selection");
}

/// A row is as wide as its translated name needs: a long one isn't cut at a fixed width.
#[test]
fn preview_rows_fit_long_translated_names() {
    let ctx = egui::Context::default();
    let long = "Скругленный прямоугольник с обратным углом";
    let mut widths = (0.0, 0.0);
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        egui::Area::new(egui::Id::unique("rows")).show(ui.ctx(), |ui| {
            ui.set_max_width(10.0);
            let r = preview_row(ui, false, long, 40.0, |_, _, _| {});
            let text = ui.fonts_mut(|f| f.layout_no_wrap(long.to_string(), egui::FontId::proportional(12.0), egui::Color32::WHITE).size().x);
            widths = (r.rect.width(), 40.0 + 12.0 + text);
        });
    });
    out.textures_delta.clear();
    assert!(widths.0 >= widths.1, "{widths:?}");
}

/// The corner shape list shows each shape's glyph beside its name, as InDesign's; a row picks
/// the shape and the current one is marked.
#[test]
fn the_corner_shape_list_has_glyph_rows_and_picks_a_shape() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "shape": "rectangle"})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let mut h = panel(app);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Corner Shape").click();
    h.run_steps(3);
    let rows = ["None", "Fancy", "Bevel", "Inset", "Inverse Rounded", "Rounded"];
    for r in rows {
        let _ = h.get_by_role_and_label(egui::accesskit::Role::Button, r);
    }
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Bevel").click();
    h.run_steps(3);
    let it = h.state().session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().clone();
    assert!(it.corners.corners.iter().all(|c| c.shape == designcraft_geom::corners::CornerShape::Bevel), "{:?}", it.corners);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Corner Shape").click();
    h.run_steps(3);
    use egui_kittest::kittest::NodeT;
    let n = h.get_by_role_and_label(egui::accesskit::Role::Button, "Bevel");
    assert_eq!(n.accesskit_node().toggled(), Some(egui::accesskit::Toggled::True), "the current shape is marked");
    // The stroke type list has the same rows, by name.
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Stroke Type").click();
    h.run_steps(3);
    let _ = h.get_by_role_and_label(egui::accesskit::Role::Button, "Thick - Thin");
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

/// The Align To button shows the target picked in its menu.
#[test]
fn the_align_to_button_shows_its_target() {
    assert_eq!(align_to_icon("selection"), "align-to");
    assert_eq!(align_to_icon("keyObject"), "align-to-key");
    assert_eq!(align_to_icon("margins"), "align-to-margins");
    assert_eq!(align_to_icon("page"), "align-to-page");
    assert_eq!(align_to_icon("spread"), "align-to-spread");
}

#[test]
fn apply_to_text_colours_the_text_of_several_text_frames() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let mut ids = vec![];
    let mut stories = vec![];
    for x in [72, 250, 430] {
        let r = app.run("frame.create", json!({"rect": [x, 72, x + 150, 140], "content": "text", "text": "75%"})).unwrap();
        ids.push(r["id"].as_u64().unwrap());
        stories.push(designcraft_doc::StoryId(r["story"].as_u64().unwrap()));
    }
    app.run("selection.set", json!({"ids": ids})).unwrap();
    let mut h = panel(app);
    h.get_by_label("Fill swatch").click();
    h.run_steps(3);
    // Three text frames, as one: Apply To is offered.
    assert!(h.query_by_label("Apply To:").is_some());
    h.get_by_label("Text").click();
    h.run_steps(3);
    h.get_by_label(CYAN).click();
    h.run_steps(3);
    let doc = &h.state().session.active().unwrap().doc;
    for (id, story) in ids.iter().zip(&stories) {
        assert_eq!(doc.story(*story).unwrap().char_format_at(0).over.fill.as_deref(), Some(CYAN), "every frame's text is cyan");
        assert_eq!(doc.item(designcraft_doc::ids::ItemId(*id)).unwrap().fill.swatch, "[None]", "the frames aren't");
    }
}

/// Points selected with Direct Selection: InDesign's "Anchor Point(s)" panel, with Convert
/// Points and the Delete Point(s) quick action, as in the Zero2Graphic tutorial (1:25).
#[test]
fn selected_points_show_convert_points_and_delete_points() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [72, 72, 172, 172]})).unwrap()["id"].as_u64().unwrap();
    let b = app.run("frame.create", json!({"rect": [200, 72, 300, 172]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"anchors": [[a, 0, 0], [b, 0, 0]]})).unwrap();
    assert_eq!(selection_title(&app, &super::super::sel_info(&app)), "Anchor Point(s)");
    let mut h = panel(app);
    // The four point types (Appearance has a Corner row too).
    for tip in ["Plain", "Corner", "Smooth", "Symmetrical"] {
        assert!(h.get_all_by_label(tip).next().is_some(), "{tip}");
    }
    assert!(h.query_by_label("Align").is_none(), "only the sections InDesign shows for points");
    h.get_by_label("Delete Point(s)").click();
    h.run_steps(3);
    let d = &h.state().session.active().unwrap().doc;
    for id in [a, b] {
        assert_eq!(d.item(designcraft_doc::ids::ItemId(id)).unwrap().path.subpaths[0].anchors.len(), 3, "one point gone from each");
    }
}

/// With points selected, Transform measures and moves the points: one corner of a frame shows
/// that corner, not the frame, and X moves the corner alone; W spreads two points apart.
#[test]
fn transform_measures_and_moves_the_selected_points() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [72, 72, 172, 172]})).unwrap()["id"].as_u64().unwrap();
    let corner = |app: &DesignApp, i: usize| {
        let d = &app.session.active().unwrap().doc;
        let it = d.item(designcraft_doc::ids::ItemId(a)).unwrap();
        it.xf * it.path.subpaths[0].anchors[i].p
    };
    let first = corner(&app, 0);
    app.run("selection.set", json!({"anchors": [[a, 0, 0]]})).unwrap();
    let i = super::super::sel_info(&app).unwrap();
    assert_eq!((i.page_rect.width(), i.page_rect.height()), (0.0, 0.0), "a point, not the frame");
    app.ui.ref_point = 0;
    transform_anchors(&mut app, &json!({"x": i.page_rect.x0 + 10.0})).unwrap();
    assert_eq!(corner(&app, 0), first + designcraft_geom::Vec2::new(10.0, 0.0), "the corner moved");
    assert_eq!(corner(&app, 2), designcraft_geom::Point::new(172.0, 172.0), "the opposite corner stayed");
    // Two points 100 pt apart, W 200 from the left: the right one goes 100 further.
    app.run("selection.set", json!({"anchors": [[a, 0, 2], [a, 0, 3]]})).unwrap();
    let (p2, p3) = (corner(&app, 2), corner(&app, 3));
    let w = super::super::sel_info(&app).unwrap().page_rect.width();
    transform_anchors(&mut app, &json!({"width": w * 2.0})).unwrap();
    let (left, right) = if p2.x < p3.x { (2, 3) } else { (3, 2) };
    let (pl, pr) = if left == 2 { (p2, p3) } else { (p3, p2) };
    assert_eq!(corner(&app, left), pl, "the reference side stays");
    assert!((corner(&app, right).x - (pr.x + w)).abs() < 1e-9, "spread to twice the width");
}

/// One point has no size: W and H show its frame's, greyed out, as InDesign; two points
/// measure (and set) their own.
#[test]
fn one_selected_point_shows_its_frames_size_greyed() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [72, 72, 172, 222]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"anchors": [[a, 0, 0]]})).unwrap();
    assert_eq!(super::super::sel_info(&app).unwrap().object_size, (100.0, 150.0));
    let mut h = panel(app);
    h.run_steps(2);
    let greyed = |h: &Harness<'_, DesignApp>| {
        h.query_all_by(|n| n.role() == egui::accesskit::Role::TextInput && n.is_disabled()).filter_map(|n| n.value()).collect::<Vec<_>>()
    };
    let u = units(h.state());
    let f = |v| designcraft_geom::units::format_measure(v, u);
    assert_eq!(greyed(&h), [f(100.0), f(150.0)], "the frame's W and H, greyed");
    h.state_mut().run("selection.set", json!({"anchors": [[a, 0, 0], [a, 0, 2]]})).unwrap();
    h.run_steps(2);
    assert!(greyed(&h).is_empty(), "two points have a size of their own");
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
