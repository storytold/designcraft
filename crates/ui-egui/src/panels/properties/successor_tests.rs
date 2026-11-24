//! Successors, on the installed framework, of the native Properties panel tests of `tests.rs`. Each keeps its old steps and
//! checks (the document, the commands, the undo steps and what the panel shows) and runs them in a window whose context has
//! the craft UI runtime installed and the app's framework flag on, so the panel draws its framework body. Where the old
//! test named a native control (the paragraph panel's `☰` menu button and its padded or ticked row labels) the successor
//! names the framework control (the `Paragraph` menu button; a row is named without a tick, and its state is its toggle).
//! The old tests stay until each one is deleted with its native body.

use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

use super::framework_test_kit as ln;
use super::framework_test_kit::Window;
use super::object::transform_anchors;
use super::*;

fn language(app: &mut DesignApp) -> Value {
    text_attrs(app).unwrap()["chars"]["language"].clone()
}

/// The graphic [`linked_graphics`] places.
const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#4488cc"/></svg>"##;

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
                data: designcraft_doc::AssetData::embedded(SVG.to_vec()),
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
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(width, 600.0)).build_ui_state(
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
    settle_links(&mut h);
    h
}

/// The link statuses come from a background check: step until it has finished and its rows are
/// drawn.
fn settle_links(h: &mut Harness<'static, DesignApp>) {
    for _ in 0..500 {
        h.run_steps(1);
        if !h.state().links_worker.checking() {
            h.run_steps(1);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the background link check did not finish");
}

/// Successor of `stroke_panel_shows_the_text_stroke_options_while_text_is_selected` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn stroke_panel_shows_the_text_stroke_options_while_text_is_selected() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
    let mut ready = false;
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(320.0, 400.0)).build_ui_state(
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

/// The text Stroke panel's Weight keeps a typed value inside the character stroke weight's
/// range of 0 to 800 pt, as the character style's Weight field does.
/// Successor of `text_stroke_panel_keeps_the_weight_in_range` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn text_stroke_panel_keeps_the_weight_in_range() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Hello", "caret": true})).unwrap();
    app.run("edit.selectAll", json!({})).unwrap();
    let mut ready = false;
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(320.0, 400.0)).build_ui_state(
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
    let weight = |h: &mut Harness<'_, DesignApp>| crate::panels::text_attrs(h.state_mut()).unwrap()["chars"]["strokeWeight"].as_f64();
    for (typed, expected) in [("2000", 800.0), ("12", 12.0), ("-5", 0.0)] {
        let field = h.get_all_by_role(egui::accesskit::Role::TextInput).next().unwrap().rect();
        type_in_field(&mut h, field, typed);
        assert_eq!(weight(&mut h), Some(expected), "{typed}");
    }
}

/// The Properties panel's text stroke Weight keeps a typed value inside the same 0 to 800 pt
/// range as the Stroke panel and the character style's Weight field.
/// Successor of `properties_text_stroke_weight_keeps_the_character_range` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_text_stroke_weight_keeps_the_character_range() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Hello", "caret": true})).unwrap();
    app.run("edit.selectAll", json!({})).unwrap();
    let mut ready = false;
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(320.0, 400.0)).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), "");
                crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                ready = true;
                return;
            }
            super::text::text_appearance_section(app, ui);
        },
        app,
    );
    h.run_steps(3);
    let weight = |h: &mut Harness<'_, DesignApp>| crate::panels::text_attrs(h.state_mut()).unwrap()["chars"]["strokeWeight"].as_f64();
    for (typed, expected) in [("500", 500.0), ("2000", 800.0), ("-5", 0.0)] {
        let field = h.get_all_by_role(egui::accesskit::Role::TextInput).next().unwrap().rect();
        type_in_field(&mut h, field, typed);
        assert_eq!(weight(&mut h), Some(expected), "{typed}");
    }
}

/// Successor of `links_panel_distinguishes_same_named_sources_and_keeps_link_actions` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn links_panel_distinguishes_same_named_sources_and_keeps_link_actions() {
    // Files holding the graphics' bytes, so their links are OK and can be embedded.
    let dir = std::env::temp_dir().join(format!("dc-links-panel-same-name-{}", std::process::id()));
    let (first, second) = (dir.join("studio-a/photo.svg"), dir.join("studio-b/photo.svg"));
    for file in [&first, &second] {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, SVG).unwrap();
    }
    let (first, second) = (first.to_str().unwrap(), second.to_str().unwrap());
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
    settle_links(&mut h);
    // Embedding keeps the link for Unembed.
    let doc = &h.state().session.active().unwrap().doc;
    assert_eq!(doc.assets[&assets[1]].link.as_deref(), Some(second));
    assert_eq!(doc.assets[&assets[0]].link.as_deref(), Some(first));
    assert_eq!(h.get_all_by_label("Embedded").count(), 2);
    assert_eq!(h.get_all_by_label("photo.svg").count(), 3);
    h.get_by_label(first).click();
    h.run_steps(3);
    h.get_by_label("Go To").click();
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().selection.items, vec![items[0]]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Successor of `links_panel_keeps_long_source_paths_inside_a_narrow_panel` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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

/// Successor of `properties_character_section_sets_the_language` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_character_section_sets_the_language() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "bunun", "caret": true})).unwrap();
    app.run("edit.selectAll", json!({})).unwrap();
    assert_ne!(language(&mut app), "French");
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(320.0, 1400.0)).build_ui_state(
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

/// Drop shadow Distance and Size in the Effects panel read and show the document's unit, as
/// the Effects dialog does (#1523): in picas a typed `7` is 7 picas in both.
/// Successor of `effects_panel_shadow_fields_use_the_document_unit` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn effects_panel_shadow_fields_use_the_document_unit() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 144, 144]})).unwrap()["id"].clone();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    app.run("object.dropShadow", json!({"on": true, "distance": 7.0, "size": 5.0})).unwrap();
    let shadow = |app: &DesignApp| {
        let d = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id.as_u64().unwrap())).unwrap().effects.drop_shadow.clone();
        (d.distance, d.size)
    };
    let mut h = panel_harness(app, effects_panel);
    h.run_steps(3);
    let values =
        |h: &Harness<'static, DesignApp>| -> Vec<String> { h.get_all_by_role(egui::accesskit::Role::TextInput).filter_map(|n| n.value()).collect() };
    let shown = values(&h);
    assert!(shown.contains(&"0p7".to_string()) && shown.contains(&"0p5".to_string()), "{shown:?}");
    for (old, typed, want) in [("0p7", "7", (84.0, 5.0)), ("0p5", "3mm", (84.0, 3.0 * designcraft_geom::units::PT_PER_MM))] {
        let field = h.get_all_by_role(egui::accesskit::Role::TextInput).find(|n| n.value().as_deref() == Some(old)).unwrap();
        field.focus();
        h.run_steps(1);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.get_all_by_role(egui::accesskit::Role::TextInput).find(|n| n.is_focused()).unwrap().type_text(typed);
        h.run_steps(1);
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        let got = shadow(h.state_mut());
        assert!((got.0 - want.0).abs() < 1e-3 && (got.1 - want.1).abs() < 1e-3, "{typed}: {got:?}, want {want:?}");
    }
}

/// Properties › Character and Paragraph •••, as in InDesign: the sections expand in place,
/// with the scales, baseline shift, skew, language, OpenType and the case buttons, and the
/// indents, spacing and drop caps.
/// Properties › Paragraph ••• on a First Line Only paragraph: Align to Baseline Grid shows
/// pressed, and a click turns alignment off instead of making it All Lines.
/// Successor of `more_options_grid_button_keeps_first_line_only` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
    // The framework's icon button shows its on look as the selected state.
    assert_eq!(h.get_by_role_and_label(egui::accesskit::Role::Button, "Align to Baseline Grid").accesskit_node().is_selected(), Some(true));
    assert_eq!(text_attrs(h.state_mut()).unwrap()["para"]["gridAlign"], "firstLineOnly");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Align to Baseline Grid").click();
    h.run_steps(3);
    assert_eq!(text_attrs(h.state_mut()).unwrap()["para"]["gridAlign"], "none");
}

/// Successor of `more_options_expand_character_and_paragraph_in_place` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
/// Successor of `paragraph_panel_keeps_first_line_only` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn paragraph_panel_keeps_first_line_only() {
    let mut app = text_app();
    app.run("type.para", json!({"attrs": {"gridAlign": "firstLineOnly"}})).unwrap();
    let mut h = panel_harness(app, paragraph_panel);
    h.run_steps(3);
    // The framework's icon button shows its on look as the selected state.
    assert_eq!(h.get_by_role_and_label(egui::accesskit::Role::Button, "Align to Baseline Grid").accesskit_node().is_selected(), Some(true));
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Align to Baseline Grid").click();
    h.run_steps(3);
    assert_eq!(text_attrs(h.state_mut()).unwrap()["para"]["gridAlign"], "firstLineOnly");
    h.get_all_by_label("Paragraph").find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button).expect("the panel menu button").click();
    h.run_steps(3);
    // The row is named without its tick; its check is its toggle.
    let row = h.get_by_label("Only Align First Line to Grid");
    assert_eq!(row.accesskit_node().toggled(), Some(egui::accesskit::Toggled::True), "the panel menu checks Only Align First Line to Grid");
}

/// Successor of `paragraph_panel_menu_toggles_ignore_optical_margin` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn paragraph_panel_menu_toggles_ignore_optical_margin() {
    let mut h = panel_harness(text_app(), paragraph_panel);
    h.run_steps(3);
    h.get_all_by_label("Paragraph").find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button).expect("the panel menu button").click();
    h.run_steps(3);
    h.get_by_label("Ignore Optical Margin").click();
    h.run_steps(3);
    assert_eq!(text_attrs(h.state_mut()).unwrap()["para"]["ignoreOpticalMargin"], true);
    h.get_all_by_label("Paragraph").find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button).expect("the panel menu button").click();
    h.run_steps(3);
    h.get_by_label("Ignore Optical Margin").click();
    h.run_steps(3);
    assert_eq!(text_attrs(h.state_mut()).unwrap()["para"]["ignoreOpticalMargin"], false);
}

/// Successor of `paragraph_panel_menu_define_lists_opens_its_dialog` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn paragraph_panel_menu_define_lists_opens_its_dialog() {
    let mut h = panel_harness(text_app(), paragraph_panel);
    h.run_steps(3);
    h.get_all_by_label("Paragraph").find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button).expect("the panel menu button").click();
    h.run_steps(3);
    h.get_by_label("Define Lists…").click();
    h.run_steps(3);
    assert!(h.state_mut().ui.dialog.is_some(), "Define Lists");
}

/// Successor of `paragraph_panel_is_laid_out_as_indesigns_with_its_panel_menu` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
<<<<<<< HEAD
        // An indent's or spacing's icon carries the field's name too: at least one node is named so.
        let _ = h.get_all_by_label(l);
=======
        // A framework text field's text part carries its field's name too: one or more nodes.
        assert!(h.query_all_by_label(l).next().is_some(), "{l}");
>>>>>>> design-port/craft-ui
    }
    let para = |h: &mut Harness<'static, DesignApp>| text_attrs(h.state_mut()).unwrap()["para"].clone();
    // Space Between Paragraphs Using Same Style starts at Ignore; a value is set, and emptying
    // the field ignores it again.
    assert!(para(&mut h)["sameParaStyleSpacing"].is_null());
<<<<<<< HEAD
    let field = same_style_spacing(&h);
=======
    let field = h.get_all_by_role(egui::accesskit::Role::TextInput).filter(field_node).nth(6).unwrap();
>>>>>>> design-port/craft-ui
    field.focus();
    field.type_text("6 pt");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(para(&mut h)["sameParaStyleSpacing"], 6.0);
<<<<<<< HEAD
    same_style_spacing(&h).focus();
=======
    h.get_all_by_role(egui::accesskit::Role::TextInput).filter(field_node).nth(6).unwrap().focus();
>>>>>>> design-port/craft-ui
    h.run_steps(1);
    for _ in 0..8 {
        h.key_press(egui::Key::Backspace);
    }
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(para(&mut h)["sameParaStyleSpacing"].is_null(), "Ignore");
    // Shading on, then the grid buttons and Only Align First Line to Grid from the menu.
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Shading").click();
    h.run_steps(3);
    assert_eq!(para(&mut h)["shadingOn"], true);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Align to Baseline Grid").click();
    h.run_steps(3);
    assert_eq!(para(&mut h)["gridAlign"], "allLines");
    h.get_all_by_label("Paragraph").find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button).expect("the panel menu button").click();
    h.run_steps(3);
    for l in ["Balance Ragged Lines", "Keep Options…", "Drop Caps and Nested Styles…", "GREP Styles…", "Define Lists…", "Restart/Continue Numbering"]
    {
        let _ = h.get_by_label(l);
    }
    h.get_by_label("Only Align First Line to Grid").click();
    h.run_steps(3);
    assert_eq!(para(&mut h)["gridAlign"], "firstLineOnly");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Do Not Align to Baseline Grid").click();
    h.run_steps(3);
    assert_eq!(para(&mut h)["gridAlign"], "none");
    // Keep Options… opens the selected paragraphs' Keep Options, and OK applies them.
    h.get_all_by_label("Paragraph").find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button).expect("the panel menu button").click();
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

/// Whether `n` is a field a person sees: a number field (spin button) or a text field. The editor inside each one folds
/// into the field's own node as a hidden text input, which is not a field of its own.
fn is_field(n: &egui_kittest::kittest::AccessKitNode<'_>) -> bool {
    matches!(n.role(), egui::accesskit::Role::SpinButton | egui::accesskit::Role::TextInput) && !n.is_hidden()
}

/// The rect of the field named `label`: not its caption or icon, which carry the same name.
fn field_rect(h: &Harness<'_, DesignApp>, label: &str) -> egui::Rect {
    h.query_all_by(|n| is_field(n) && n.label().as_deref() == Some(label)).next().unwrap_or_else(|| panic!("no field named {label}")).rect()
}

/// The Paragraph panel's Space Between Paragraphs Using Same Style field (its icon carries the name too).
fn same_style_spacing<'h>(h: &'h Harness<'_, DesignApp>) -> egui_kittest::Node<'h> {
    h.query_all_by(|n| is_field(n) && n.label().as_deref() == Some("Space Between Paragraphs Using Same Style"))
        .next()
        .expect("the Space Between Paragraphs Using Same Style field")
}

/// A harness drawing `panel` for `app` (fonts installed before the first frame).
fn panel_harness(app: DesignApp, panel: fn(&mut DesignApp, &mut egui::Ui)) -> Harness<'static, DesignApp> {
    crate::test_window::builder::<DesignApp>().with_size(vec2(320.0, 1400.0)).build_ui_state(
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

/// Successor of `properties_kerning_menu_sets_zero_kerning` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_kerning_menu_sets_zero_kerning() {
    let mut h = panel_harness(text_app(), show);
    h.run_steps(3);
    assert_eq!(kerning(h.state_mut())["kind"], "metrics");
<<<<<<< HEAD
    // The Kerning field's presets list holds Metrics, Optical and 0 (its icon carries the name too).
    h.get_by_role_and_label(egui::accesskit::Role::SpinButton, "Kerning").get_by_role_and_label(egui::accesskit::Role::Button, "Presets").click();
    h.run_steps(2);
    h.get_by_role_and_label(egui::accesskit::Role::ListBoxOption, "0").click();
=======
    // The kerning field's presets list (Metrics, Optical, 0) opens with Alt+Down from the focused field.
    named_field(&h, "Kerning").focus();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowDown);
    h.run_steps(2);
    h.query_all_by_label("0").last().expect("the presets list's 0 row").click();
>>>>>>> design-port/craft-ui
    h.run_steps(2);
    assert_eq!(kerning(h.state_mut())["kind"], "none");
}

fn undo_steps(app: &DesignApp) -> usize {
    app.session.active().unwrap().history.undo.len()
}

fn font_style(app: &mut DesignApp) -> String {
    text_attrs(app).unwrap()["chars"]["fontStyle"].as_str().unwrap().to_string()
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

/// Successor of `dragging_one_of_several_axis_sliders_is_one_undo_step` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(320.0, 1400.0)).build_ui_state(
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

/// Successor of `character_panel_has_kerning_and_takes_a_typed_manual_value` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn character_panel_has_kerning_and_takes_a_typed_manual_value() {
    let mut h = panel_harness(text_app(), character_panel);
    h.run_steps(3);
<<<<<<< HEAD
    // The Kerning field itself takes a manual value (its caption carries the name too).
    let field = field_rect(&h, "Kerning");
    type_in_field(&mut h, field, "-40");
=======
    // The kerning field takes a manual value typed into it (its presets list holds Metrics, Optical and 0).
    let field = named_field(&h, "Kerning");
    field.focus();
    h.run_steps(1);
    // It shows "Metrics": select that before typing over it.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    named_field(&h, "Kerning").type_text("-40");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
>>>>>>> design-port/craft-ui
    assert_eq!(kerning(h.state_mut()), json!({"kind": "manual", "value": -40.0}));
}

/// The size field reads the unit typed, as InDesign's does: 13Q is 9.213 pt (it used to keep
/// the digits only and set 13 pt). A bare number is still points.
/// Successor of `character_size_field_reads_the_unit_typed` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn character_size_field_reads_the_unit_typed() {
    let mut h = panel_harness(text_app(), character_panel);
    h.run_steps(3);
    let size = |app: &mut DesignApp| text_attrs(app).unwrap()["chars"]["size"].as_f64().unwrap();
    for (typed, pt) in [("13Q", 13.0 * designcraft_geom::units::PT_PER_Q), ("5mm", 5.0 * designcraft_geom::units::PT_PER_MM), ("14", 14.0)] {
        // The Size field by its name (its caption carries the name too); the framework paints the unit beside the
        // number instead of writing it into the value.
        let field = field_rect(&h, "Size");
        type_in_field(&mut h, field, typed);
        let got = size(h.state_mut());
        assert!((got - pt).abs() < 1e-6, "{typed}: {got} pt, want {pt}");
    }
}

/// Successor of `a_manual_kerning_value_at_a_caret_kerns_the_pair` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn a_manual_kerning_value_at_a_caret_kerns_the_pair() {
    let mut app = text_app();
    let story = app.session.active().unwrap().selection.text.unwrap().story;
    app.run("text.select", json!({"story": story.0, "anchor": 1, "focus": 1})).unwrap();
    let mut h = panel_harness(app, character_panel);
    h.run_steps(3);
<<<<<<< HEAD
    // The Kerning field itself takes a manual value (its caption carries the name too).
    let field = field_rect(&h, "Kerning");
    type_in_field(&mut h, field, "-80");
=======
    let field = named_field(&h, "Kerning");
    field.focus();
    h.run_steps(1);
    // It shows "Metrics": select that before typing over it.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    named_field(&h, "Kerning").type_text("-80");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
>>>>>>> design-port/craft-ui
    let st = h.state_mut().session.active().unwrap().doc.story(story).unwrap().format_after(0).over.kerning;
    assert_eq!(st, Some(designcraft_doc::Kerning::Manual(-80.0)), "A of the A|V pair");
    assert_eq!(kerning(h.state_mut()), json!({"kind": "manual", "value": -80.0}));
}

/// Successor of `clicking_a_swatch_chip_in_the_appearance_fill_menu_applies_it` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn clicking_a_swatch_chip_in_the_appearance_fill_menu_applies_it() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    // A graphic frame: the Appearance section shows a single Fill chip.
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let before = app.session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.swatch.clone();
    assert_ne!(before, "[Paper]");
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(320.0, 1400.0)).build_ui_state(
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
    // The row spans the menu (chip + name), not just the name text: as wide as the popover's swatch list.
    let list = h
        .query_all_by(|n| {
            matches!(n.role(), egui::accesskit::Role::List | egui::accesskit::Role::ListBox) && n.label().as_deref() == Some("Swatches")
        })
        .next()
        .expect("the Fill popover's swatch list")
        .rect();
    assert!(
        row.left() <= list.left() + 8.0 && row.width() >= list.width() - 16.0,
        "the row is the full menu width, chip included: {row:?} in {list:?}"
    );
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
/// Successor of `double_clicking_a_fill_chip_opens_the_color_picker` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
        let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(320.0, 1400.0)).with_step_dt(0.001).build_ui_state(
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
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(320.0, 1400.0)).build_ui_state(
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
/// Successor of `bullets_options_open_the_bullets_and_numbering_dialog` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn bullets_options_open_the_bullets_and_numbering_dialog() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let f = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "4 eggs"})).unwrap();
    app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 6})).unwrap();
    let mut h = panel(app);
    // Bullets and Numbering's Options, above Text Frame's.
    let bullets = h.get_all_by_label("Options").min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap();
    bullets.click();
    h.run_steps(3);
    let d = h.state().ui.dialog.clone().expect("a dialog");
    assert_eq!((d.id.as_str(), d.s("section").as_str()), ("paragraphLocal", "bullets"));
    assert_ne!(h.state().ui.open_panel.as_deref(), Some("paragraph"));
}

/// The course's chapter 21: Properties' Bulleted List button gives the selected paragraphs
/// bullets with InDesign's hanging indent; pressed again, both go.
/// Successor of `the_bulleted_list_button_applies_a_hanging_indent` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
<<<<<<< HEAD
    // The button, not its tooltip, which carries the name too once the pointer rests on it.
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Bulleted List").click();
=======
    // The button, not the tooltip that names it while the pointer rests on it after a click.
    let button = |h: &Harness<'_, DesignApp>| {
        h.get_all_by_label("Bulleted List").find(|n| n.role() != egui::accesskit::Role::Tooltip).expect("the Bulleted List button").rect().center()
    };
    let at = button(&h);
    h.hover_at(at);
    h.drag_at(at);
    h.drop_at(at);
>>>>>>> design-port/craft-ui
    h.run_steps(3);
    let a = h.state_mut().run("type.selectionAttrs", json!({})).unwrap();
    assert_eq!(
        (a["para"]["listType"].as_str(), a["para"]["leftIndent"].as_f64(), a["para"]["firstLineIndent"].as_f64()),
        (Some("bullets"), Some(18.0), Some(-18.0))
    );
    assert_eq!(crate::menus::checked(h.state(), "list.bullets", &json!({})), Some(true), "Type › Apply Bullets shows it");
<<<<<<< HEAD
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Bulleted List").click();
=======
    let at = button(&h);
    h.hover_at(at);
    h.drag_at(at);
    h.drop_at(at);
>>>>>>> design-port/craft-ui
    h.run_steps(3);
    let a = h.state_mut().run("type.selectionAttrs", json!({})).unwrap();
    assert_eq!((a["para"]["listType"].as_str(), a["para"]["leftIndent"].as_f64()), (Some("none"), Some(0.0)));
}

/// Successor of `several_shapes_show_pathfinder_and_intersect_combines_them` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
/// Successor of `the_opacity_arrow_opens_a_slider_and_a_drag_is_one_undo_step` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn the_opacity_arrow_opens_a_slider_and_a_drag_is_one_undo_step() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "shape": "rectangle"})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let opacity = |app: &DesignApp| app.session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().opacity;
    let steps = undo_steps(&app);
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(320.0, 1400.0)).build_ui_state(
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
/// Successor of `properties_frame_fitting_sets_auto_fit_and_opens_its_options` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
/// Successor of `properties_text_wrap_shows_and_sets_the_offsets` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
<<<<<<< HEAD
        let mut v: Vec<egui::Rect> = h.query_all_by(is_field).map(|n| n.rect()).filter(|r| r.center().y > top && r.center().y < bottom).collect();
=======
        let mut v: Vec<egui::Rect> = h
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .filter(field_node)
            .map(|n| n.rect())
            .filter(|r| r.center().y > top && r.center().y < bottom)
            .collect();
>>>>>>> design-port/craft-ui
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

/// The corner shape list shows each shape's glyph beside its name, as InDesign's; a row picks
/// the shape and the current one is marked.
/// Successor of `the_corner_shape_list_has_glyph_rows_and_picks_a_shape` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn the_corner_shape_list_has_glyph_rows_and_picks_a_shape() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "shape": "rectangle"})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let mut h = panel(app);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Corner Shape").click();
    h.run_steps(3);
<<<<<<< HEAD
    // The select's list rows are options.
=======
    // The select's list holds its rows as options.
>>>>>>> design-port/craft-ui
    let rows = ["None", "Fancy", "Bevel", "Inset", "Inverse Rounded", "Rounded"];
    for r in rows {
        let _ = h.get_by_role_and_label(egui::accesskit::Role::ListBoxOption, r);
    }
    h.get_by_role_and_label(egui::accesskit::Role::ListBoxOption, "Bevel").click();
    h.run_steps(3);
    let it = h.state().session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().clone();
    assert!(it.corners.corners.iter().all(|c| c.shape == designcraft_geom::corners::CornerShape::Bevel), "{:?}", it.corners);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Corner Shape").click();
    h.run_steps(3);
    use egui_kittest::kittest::NodeT;
<<<<<<< HEAD
    // An option marks the current value as its selected state.
=======
>>>>>>> design-port/craft-ui
    let n = h.get_by_role_and_label(egui::accesskit::Role::ListBoxOption, "Bevel");
    assert_eq!(n.accesskit_node().is_selected(), Some(true), "the current shape is marked");
    // The stroke type list has the same rows, by name.
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Stroke Type").click();
    h.run_steps(3);
    let _ = h.get_by_role_and_label(egui::accesskit::Role::ListBoxOption, "Thick - Thin");
}

const CYAN: &str = "C=100 M=0 Y=0 K=0";

/// Successor of `the_fill_menu_sets_a_tint` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
<<<<<<< HEAD
    // The Tint field by its name (its caption carries the name too; Opacity is a field of its own name).
    let field = field_rect(&h, "Tint:");
    type_in_field(&mut h, field, "25");
=======
    // The field on the Tint row (Opacity also reads 100%).
    // The row's caption (the field carries the same name).
    let row = h.get_by_role_and_label(egui::accesskit::Role::Label, "Tint:").rect();
    h.query_all_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("100%"))
        .find(|n| (n.rect().center().y - row.center().y).abs() < 8.0)
        .expect("the Tint field")
        .click();
    h.run_steps(2);
    h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
    h.event(egui::Event::Text("25".into()));
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
>>>>>>> design-port/craft-ui
    let fill = h.state().session.active().unwrap().doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.clone();
    assert_eq!((fill.swatch.as_str(), fill.tint), (CYAN, 0.25), "the tint, on the same swatch");
}

/// Successor of `a_tint_on_several_objects_keeps_each_ones_swatch` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
<<<<<<< HEAD
    // The Tint field by its name (its caption carries the name too; Opacity is a field of its own name).
    let field = field_rect(&h, "Tint:");
    type_in_field(&mut h, field, "25");
=======
    // The row's caption (the field carries the same name).
    let row = h.get_by_role_and_label(egui::accesskit::Role::Label, "Tint:").rect();
    h.query_all_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("100%"))
        .find(|n| (n.rect().center().y - row.center().y).abs() < 8.0)
        .expect("the Tint field")
        .click();
    h.run_steps(2);
    h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
    h.event(egui::Event::Text("25".into()));
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
>>>>>>> design-port/craft-ui
    let doc = &h.state().session.active().unwrap().doc;
    let fill = |id: u64| doc.item(designcraft_doc::ids::ItemId(id)).unwrap().fill.clone();
    assert_eq!((fill(a).swatch.as_str(), fill(a).tint), (CYAN, 0.25));
    assert_eq!((fill(b).swatch.as_str(), fill(b).tint), ("[Black]", 0.25), "the second keeps its own swatch");
}

/// Successor of `apply_to_text_colours_a_text_frames_text_not_the_frame` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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

/// Successor of `apply_to_text_colours_the_text_of_several_text_frames` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
<<<<<<< HEAD
    // Three text frames, as one: Apply To is offered (its caption carries the name too).
    assert!(h.query_by_role_and_label(egui::accesskit::Role::RadioGroup, "Apply To:").is_some());
    h.get_by_role_and_label(egui::accesskit::Role::RadioButton, "Text").click();
=======
    // Three text frames, as one: Apply To is offered.
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Label, "Apply To:").is_some());
    h.get_by_label("Text").click();
>>>>>>> design-port/craft-ui
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
/// Successor of `selected_points_show_convert_points_and_delete_points` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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

/// One point has no size: W and H show its frame's, greyed out, as InDesign; two points
/// measure (and set) their own.
/// Successor of `one_selected_point_shows_its_frames_size_greyed` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn one_selected_point_shows_its_frames_size_greyed() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [72, 72, 172, 222]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"anchors": [[a, 0, 0]]})).unwrap();
    assert_eq!(super::super::sel_info(&app).unwrap().object_size, (100.0, 150.0));
    let mut h = panel(app);
    h.run_steps(2);
<<<<<<< HEAD
    let greyed = |h: &Harness<'_, DesignApp>| h.query_all_by(|n| is_field(n) && n.is_disabled()).filter_map(|n| n.value()).collect::<Vec<_>>();
=======
    let greyed = |h: &Harness<'_, DesignApp>| {
        h.query_all_by(|n| n.role() == egui::accesskit::Role::TextInput && n.is_disabled())
            .filter(field_node)
            .filter_map(|n| n.value())
            .collect::<Vec<_>>()
    };
>>>>>>> design-port/craft-ui
    let u = units(h.state());
    let f = |v| designcraft_geom::units::format_measure(v, u);
    assert_eq!(greyed(&h), [f(100.0), f(150.0)], "the frame's W and H, greyed");
    h.state_mut().run("selection.set", json!({"anchors": [[a, 0, 0], [a, 0, 2]]})).unwrap();
    h.run_steps(2);
    assert!(greyed(&h).is_empty(), "two points have a size of their own");
}

/// Successor of `transform_flips_and_rotates_from_properties` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
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
<<<<<<< HEAD
    // The angle and scale fields are there: the angle field reads 90 (the framework paints the ° beside the number
    // instead of writing it into the value).
    let angle = h.get_by_role_and_label(egui::accesskit::Role::SpinButton, "Rotation Angle").accesskit_node().numeric_value();
    assert!(angle.is_some_and(|v| (v - 90.0).abs() < 1e-6), "the angle field shows the quarter turn: {angle:?}");
    let _ = h.get_by_role_and_label(egui::accesskit::Role::SpinButton, "Scale X Percentage");
=======
    // The angle and scale fields are there: the rotation field shows the quarter turn (its framework number field may write a decimal: "90.0°").
    let ninety = |v: &str| v.trim_end_matches('°').trim().parse::<f64>().is_ok_and(|n| (n - 90.0).abs() < 1e-9) && v.ends_with('°');
    assert!(
        h.query_all_by(
            |n| matches!(n.role(), egui::accesskit::Role::TextInput | egui::accesskit::Role::SpinButton) && n.value().as_deref().is_some_and(ninety)
        )
        .next()
        .is_some()
    );
>>>>>>> design-port/craft-ui
}

/// The vertical extent of Properties › Text Frame: from its heading (the last "Text Frame" label;
/// the selection band above may say "Text Frame" too) to the Quick Actions heading.
fn text_frame_band(h: &Harness<'_, DesignApp>) -> (f32, f32) {
    let top = h.get_all_by_label("Text Frame").map(|n| n.rect().bottom()).fold(f32::MIN, f32::max);
    (top, h.get_by_label("Quick Actions").rect().top())
}

/// The fields of Properties › Text Frame (Columns is a number field, the lengths text fields), in reading order.
fn text_frame_fields(h: &Harness<'_, DesignApp>) -> Vec<egui::Rect> {
    let (top, bottom) = text_frame_band(h);
<<<<<<< HEAD
    let mut v: Vec<egui::Rect> = h.query_all_by(is_field).map(|n| n.rect()).filter(|r| r.center().y > top && r.center().y < bottom).collect();
=======
    let mut v: Vec<egui::Rect> = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .filter(field_node)
        .map(|n| n.rect())
        .filter(|r| r.center().y > top && r.center().y < bottom)
        .collect();
>>>>>>> design-port/craft-ui
    v.sort_by(|a, b| (a.top(), a.left()).partial_cmp(&(b.top(), b.left())).unwrap());
    v
}

/// Clicks the control labeled `label` inside Properties › Text Frame.
fn click_in_text_frame(h: &mut Harness<'_, DesignApp>, label: &str) {
    let (top, bottom) = text_frame_band(h);
    h.get_all_by_label(label).find(|n| n.rect().center().y > top && n.rect().center().y < bottom).unwrap().click();
    h.run_steps(3);
}

fn type_in_field(h: &mut Harness<'_, DesignApp>, at: egui::Rect, text: &str) {
    h.hover_at(at.center());
    h.drag_at(at.center());
    h.drop_at(at.center());
    h.run_steps(2);
    h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
    h.event(egui::Event::Text(text.into()));
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
}

fn frame_options(app: &DesignApp, id: u64) -> designcraft_doc::TextFrameOptions {
    app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().text_frame().unwrap().options.clone()
}

/// Properties › Text Frame shows the frame's inset spacing under the columns, linked while the
/// four insets are the same, and the four vertical justification buttons. Each change is one
/// undo step; the button that is already on adds none.
/// Successor of `properties_text_frame_sets_inset_spacing_and_vertical_justification` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_text_frame_sets_inset_spacing_and_vertical_justification() {
    use designcraft_doc::VerticalJustification;
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 300], "content": "text", "text": "Hello"})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let mut h = panel(app);
    // Columns and gutter, then Top and Left, then Bottom and Right.
    let f = text_frame_fields(&h);
    assert_eq!(f.len(), 6, "columns, gutter and four insets: {f:?}");
    let steps = undo_steps(h.state());
    // Linked: the top inset sets all four.
    type_in_field(&mut h, f[2], "12pt");
    assert_eq!(frame_options(h.state(), id).inset, [12.0; 4]);
    assert_eq!(undo_steps(h.state()), steps + 1);
    // Unlinked: the bottom one alone.
    click_in_text_frame(&mut h, "Make all settings the same");
    let f = text_frame_fields(&h);
    type_in_field(&mut h, f[4], "6pt");
    assert_eq!(frame_options(h.state(), id).inset, [12.0, 12.0, 6.0, 12.0], "[top, left, bottom, right]");
    // Vertical justification.
    assert_eq!(frame_options(h.state(), id).vertical_justification, VerticalJustification::Top);
    for (label, vj) in
        [("Bottom", VerticalJustification::Bottom), ("Center", VerticalJustification::Center), ("Justify", VerticalJustification::Justify)]
    {
        click_in_text_frame(&mut h, label);
        assert_eq!(frame_options(h.state(), id).vertical_justification, vj, "{label}");
    }
    let steps = undo_steps(h.state());
    click_in_text_frame(&mut h, "Justify");
    assert_eq!(undo_steps(h.state()), steps, "the button already on adds no undo step");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(frame_options(h.state(), id).vertical_justification, VerticalJustification::Center);
}

/// Insets unlinked before Revert start linked again in the reverted document, whose frame has the
/// same id and four equal insets.
#[cfg(not(target_arch = "wasm32"))]
/// Successor of `properties_text_frame_inset_link_starts_linked_after_revert` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_text_frame_inset_link_starts_linked_after_revert() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 300], "content": "text", "text": "Hello"})).unwrap()["id"].as_u64().unwrap();
    let dir = tempfile::tempdir().unwrap();
    app.run("file.saveAs", json!({"path": dir.path().join("revert.designcraft")})).unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let mut h = panel(app);
    click_in_text_frame(&mut h, "Make all settings the same");
    h.state_mut().run("frame.create", json!({"rect": [320, 72, 400, 150], "content": "text", "text": "unsaved"})).unwrap();
    h.state_mut().run("file.revert", json!({})).unwrap();
    h.state_mut().run("selection.set", json!({"ids": [id]})).unwrap();
    h.run_steps(3);
    let f = text_frame_fields(&h);
    type_in_field(&mut h, f[4], "6pt");
    assert_eq!(frame_options(h.state(), id).inset, [6.0; 4], "linked: the bottom inset sets all four");
}

/// With the Type tool in the second frame of a thread, Properties shows the Text Frame section,
/// and its insets and vertical justification change that frame and not the first.
/// Successor of `properties_text_frame_shows_while_typing_and_changes_the_caret_frame` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_text_frame_shows_while_typing_and_changes_the_caret_frame() {
    use designcraft_doc::VerticalJustification;
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let text = (0..40).map(|i| format!("Line {i} of the threaded story.")).collect::<Vec<_>>().join(" ");
    let a = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": text})).unwrap();
    let b = app.run("frame.create", json!({"rect": [36, 200, 300, 700], "content": "text", "text": "", "caret": false})).unwrap();
    let (a, b, sid) = (a["id"].as_u64().unwrap(), b["id"].as_u64().unwrap(), a["story"].as_u64().unwrap());
    app.run("text.thread", json!({"from": a, "to": b})).unwrap();
    // The end of the story is in the second frame.
    app.run("text.select", json!({"story": sid, "anchor": text.len()})).unwrap();
    let mut h = panel(app);
    let f = text_frame_fields(&h);
    assert_eq!(f.len(), 6, "columns, gutter and four insets while typing: {f:?}");
    type_in_field(&mut h, f[2], "9pt");
    click_in_text_frame(&mut h, "Bottom");
    let (oa, ob) = (frame_options(h.state(), a), frame_options(h.state(), b));
    assert_eq!((oa.inset, ob.inset), ([0.0; 4], [9.0; 4]));
    assert_eq!((oa.vertical_justification, ob.vertical_justification), (VerticalJustification::Top, VerticalJustification::Bottom));
    assert!(h.state().session.active().unwrap().selection.text.is_some(), "the caret stays in the text");
}

/// A non-rectangular text frame shows one Inset, which sets the distance its text keeps from
/// the outline. The composer also bounds its text by all four insets, so the field sets all four.
/// Successor of `properties_text_frame_shows_one_inset_for_an_oval` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_text_frame_shows_one_inset_for_an_oval() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 300], "shape": "ellipse", "content": "text", "text": "Hello"})).unwrap()["id"]
        .as_u64()
        .unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let mut h = panel(app);
    let f = text_frame_fields(&h);
    assert_eq!(f.len(), 3, "columns, gutter and one inset: {f:?}");
    h.state_mut().run("object.textFrameOptions", json!({"ids": [id], "inset": [1, 2, 3, 4]})).unwrap();
    h.run_steps(2);
    type_in_field(&mut h, f[2], "8pt");
    assert_eq!(frame_options(h.state(), id).inset, [8.0; 4]);
}

/// Lowering an oval's uniform inset with the one Inset field lowers every side, so no hidden
/// side keeps the old inset and the text area that goes with it.
/// Successor of `properties_text_frame_one_inset_lowers_a_uniform_inset_on_every_side` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_text_frame_one_inset_lowers_a_uniform_inset_on_every_side() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 300], "shape": "ellipse", "content": "text", "text": "Hello"})).unwrap()["id"]
        .as_u64()
        .unwrap();
    app.run("object.textFrameOptions", json!({"ids": [id], "inset": 96})).unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let mut h = panel(app);
    let f = text_frame_fields(&h);
    assert_eq!(f.len(), 3, "columns, gutter and one inset: {f:?}");
    type_in_field(&mut h, f[2], "0pt");
    assert_eq!(frame_options(h.state(), id).inset, [0.0; 4]);
}

/// With selected text that runs from an oval into a rectangular frame, the one Inset field
/// changes the top inset of both, and each frame keeps its other sides.
/// Successor of `properties_text_frame_one_inset_keeps_the_sides_of_a_rectangle_in_the_selection` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_text_frame_one_inset_keeps_the_sides_of_a_rectangle_in_the_selection() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let text = (0..40).map(|i| format!("Line {i} of the threaded story.")).collect::<Vec<_>>().join(" ");
    let a = app.run("frame.create", json!({"rect": [72, 72, 300, 300], "shape": "ellipse", "content": "text", "text": text})).unwrap();
    let b = app.run("frame.create", json!({"rect": [72, 400, 300, 760], "content": "text", "text": "", "caret": false})).unwrap();
    let (oval, rect, sid) = (a["id"].as_u64().unwrap(), b["id"].as_u64().unwrap(), a["story"].as_u64().unwrap());
    app.run("text.thread", json!({"from": oval, "to": rect})).unwrap();
    app.run("object.textFrameOptions", json!({"ids": [oval, rect], "inset": [1, 2, 3, 4]})).unwrap();
    app.run("text.select", json!({"story": sid, "anchor": 0, "focus": text.len()})).unwrap();
    assert_eq!(
        designcraft_engine::cmd::text_frame_options_frames(&app.session),
        [designcraft_doc::ItemId(oval), designcraft_doc::ItemId(rect)],
        "the selected text runs through both frames"
    );
    let mut h = panel(app);
    let f = text_frame_fields(&h);
    assert_eq!(f.len(), 3, "columns, gutter and one inset for the first frame, an oval: {f:?}");
    type_in_field(&mut h, f[2], "8pt");
    assert_eq!(frame_options(h.state(), oval).inset, [8.0, 2.0, 3.0, 4.0]);
    assert_eq!(frame_options(h.state(), rect).inset, [8.0, 2.0, 3.0, 4.0]);
}

/// With selected text that runs through two rectangular frames, the inset link starts off when
/// one of them has uneven sides, even while the frame the section shows has four equal insets, so
/// a changed Top leaves the other sides of each frame as they were.
/// Successor of `properties_text_frame_inset_link_starts_off_when_another_frame_has_uneven_sides` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_text_frame_inset_link_starts_off_when_another_frame_has_uneven_sides() {
    for even_first in [true, false] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let text = (0..40).map(|i| format!("Line {i} of the threaded story.")).collect::<Vec<_>>().join(" ");
        let a = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": text})).unwrap();
        let b = app.run("frame.create", json!({"rect": [36, 200, 300, 700], "content": "text", "text": "", "caret": false})).unwrap();
        let (a, b, sid) = (a["id"].as_u64().unwrap(), b["id"].as_u64().unwrap(), a["story"].as_u64().unwrap());
        app.run("text.thread", json!({"from": a, "to": b})).unwrap();
        let (even, uneven) = if even_first { (a, b) } else { (b, a) };
        app.run("object.textFrameOptions", json!({"ids": [even], "inset": 5})).unwrap();
        app.run("object.textFrameOptions", json!({"ids": [uneven], "inset": [1, 2, 3, 4]})).unwrap();
        app.run("text.select", json!({"story": sid, "anchor": 0, "focus": text.len()})).unwrap();
        assert_eq!(
            designcraft_engine::cmd::text_frame_options_frames(&app.session),
            [designcraft_doc::ItemId(a), designcraft_doc::ItemId(b)],
            "the selected text runs through both frames"
        );
        let mut h = panel(app);
        let f = text_frame_fields(&h);
        assert_eq!(f.len(), 6, "columns, gutter and four insets: {f:?}");
        type_in_field(&mut h, f[2], "8pt");
        assert_eq!(frame_options(h.state(), even).inset, [8.0, 5.0, 5.0, 5.0], "even frame first: {even_first}");
        assert_eq!(frame_options(h.state(), uneven).inset, [8.0, 2.0, 3.0, 4.0], "even frame first: {even_first}");
    }
}

/// A link turned on for one frame does not carry over to selected text that runs from that frame
/// into a frame with uneven sides: the link starts off there, and a changed Top leaves the other
/// sides as they were.
/// Successor of `properties_text_frame_inset_link_of_one_frame_does_not_carry_to_a_text_selection` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_text_frame_inset_link_of_one_frame_does_not_carry_to_a_text_selection() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let text = (0..40).map(|i| format!("Line {i} of the threaded story.")).collect::<Vec<_>>().join(" ");
    let a = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": text})).unwrap();
    let b = app.run("frame.create", json!({"rect": [36, 200, 300, 700], "content": "text", "text": "", "caret": false})).unwrap();
    let (a, b, sid) = (a["id"].as_u64().unwrap(), b["id"].as_u64().unwrap(), a["story"].as_u64().unwrap());
    app.run("text.thread", json!({"from": a, "to": b})).unwrap();
    app.run("object.textFrameOptions", json!({"ids": [a], "inset": 5})).unwrap();
    app.run("object.textFrameOptions", json!({"ids": [b], "inset": [1, 2, 3, 4]})).unwrap();
    app.run("selection.set", json!({"ids": [a]})).unwrap();
    let mut h = panel(app);
    // Off, then on again: the link of the first frame alone is now turned on by hand.
    click_in_text_frame(&mut h, "Make all settings the same");
    click_in_text_frame(&mut h, "Make all settings the same");
    // The caret at the start of the story is in the first frame; the selection runs into the second.
    h.state_mut().run("text.select", json!({"story": sid, "anchor": text.len(), "focus": 0})).unwrap();
    h.run_steps(3);
    let f = text_frame_fields(&h);
    assert_eq!(f.len(), 6, "columns, gutter and four insets: {f:?}");
    type_in_field(&mut h, f[2], "8pt");
    assert_eq!(frame_options(h.state(), a).inset, [8.0, 5.0, 5.0, 5.0]);
    assert_eq!(frame_options(h.state(), b).inset, [8.0, 2.0, 3.0, 4.0]);
}

/// Type on a path sets one line along the path: Properties › Text Frame shows no insets and no
/// vertical justification buttons for it.
/// Successor of `properties_text_frame_shows_no_insets_for_type_on_a_path` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn properties_text_frame_shows_no_insets_for_type_on_a_path() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "shape": "ellipse"})).unwrap()["id"].as_u64().unwrap();
    app.run("type.onPath", json!({"id": id, "text": "Around"})).unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    assert!(frame_options(&app, id).path.is_some());
    let h = panel(app);
    assert_eq!(text_frame_fields(&h).len(), 2, "columns and gutter only");
    let (top, bottom) = text_frame_band(&h);
    assert!(h.query_all_by_label("Justify").all(|n| n.rect().center().y < top || n.rect().center().y > bottom));
}

// ---------------------------------------------------------------------------------------------------------------------
// Effects and Text Wrap, by accessible name

/// Whether any node of the open panel is named like `part`.
fn named_like(h: &Window, part: &str) -> bool {
    h.query_all_by_label_contains(part).next().is_some()
}

/// Successor of `the_effects_panel_shows_the_level_and_isolate_blending_and_knockout_group_for_groups`: for a group the Effects
/// panel shows its level (Group:), the blend mode and opacity, and Isolate Blending and Knockout Group, which set the group's
/// flags; for another object the level is Object: and Knockout Group does nothing.
#[test]
fn the_framework_effects_panel_shows_the_level_and_group_blending_options() {
    let mut app = ln::document();
    let a = app.run("frame.create", json!({"rect": [72, 72, 144, 144]})).unwrap()["id"].as_u64().unwrap();
    let b = app.run("frame.create", json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let g = app.run("object.group", json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [g]})).unwrap();
    app.run("object.opacity", json!({"opacity": 1.0, "blend": "colorDodge"})).unwrap();
    let mut h = ln::open(app, crate::panels::properties::effects_panel);
    for expected in ["Group", "Isolate Blending", "Knockout Group"] {
        assert!(named_like(&h, expected), "{expected} is shown");
    }
    assert!(h.query_all_by(|n| n.value().is_some_and(|v| v.contains("Color Dodge"))).next().is_some(), "the blend mode shows");
    ln::click(&mut h, egui::accesskit::Role::CheckBox, "Isolate Blending");
    assert!(ln::item(&h, g).isolate);
    ln::click(&mut h, egui::accesskit::Role::CheckBox, "Knockout Group");
    assert!(ln::item(&h, g).knockout);
    // Another object: the level is Object, and the group options do nothing.
    h.state_mut().run("selection.set", json!({"ids": [a]})).unwrap();
    ln::steps(&mut h, 3);
    assert!(named_like(&h, "Object"), "the level is Object");
    if ln::has(&h, egui::accesskit::Role::CheckBox, "Knockout Group") {
        ln::click(&mut h, egui::accesskit::Role::CheckBox, "Knockout Group");
    }
    assert!(!ln::item(&h, a).knockout);
}

/// Successor of `pasted_into_frames_keep_object_level_and_disable_group_blending`: a frame that holds pasted content stays an
/// Object in the Effects panel, and its group-only options are disabled: clicking them changes nothing and makes no undo step.
#[test]
fn the_framework_effects_panel_keeps_a_frame_with_pasted_content_an_object() {
    let mut app = ln::document();
    let child = app.run("frame.create", json!({"rect": [80, 80, 120, 120]})).unwrap()["id"].clone();
    app.run("selection.set", json!({"ids": [child]})).unwrap();
    app.run("edit.copy", json!({})).unwrap();
    let container = app.run("frame.create", json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [container]})).unwrap();
    app.run("edit.pasteInto", json!({})).unwrap();
    app.run("selection.set", json!({"ids": [container]})).unwrap();
    let mut h = ln::open(app, crate::panels::properties::effects_panel);
    assert!(named_like(&h, "Object"), "a frame remains an object after Paste Into");
    let undo = ln::undo_steps(&h);
    for label in ["Isolate Blending", "Knockout Group"] {
        if ln::has(&h, egui::accesskit::Role::CheckBox, label) {
            ln::click(&mut h, egui::accesskit::Role::CheckBox, label);
        }
    }
    let it = ln::item(&h, container);
    assert!(it.has_nested_items());
    assert!(!it.isolate && !it.knockout, "group-only options stay disabled for a frame holding content");
    assert_eq!(ln::undo_steps(&h), undo, "disabled controls do not create undo steps");
}

/// Successor of `the_text_wrap_panel_shows_each_offset_and_invert`: the Text Wrap panel shows Top, Left, Bottom and Right with
/// each offset in the ruler unit (picas here: 0p1 = 1 pt) and Wrap To, and Invert sets the wrap's invert flag.
#[test]
fn the_framework_text_wrap_panel_shows_each_offset_and_invert() {
    let mut app = ln::document();
    let id = app.run("frame.create", json!({"rect": [72, 72, 144, 144]})).unwrap()["id"].as_u64().unwrap();
    app.run("object.textWrap", json!({"mode": "boundingBox", "offset": [1, 2, 3, 4]})).unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let mut h = ln::open(app, crate::panels::properties::wrap_panel);
    for side in ["Top", "Left", "Bottom", "Right"] {
        assert!(named_like(&h, side), "{side} is shown");
    }
    let shown: Vec<String> = h
        .query_all_by(|n| n.role() == egui::accesskit::Role::TextInput || n.role() == egui::accesskit::Role::SpinButton)
        .filter_map(|n| n.value())
        .collect();
    for offset in ["0p1", "0p2", "0p3", "0p4"] {
        assert!(shown.iter().any(|v| v == offset), "{offset} in {shown:?}");
    }
    assert!(named_like(&h, "Wrap To"), "Wrap To is shown");
    let invert = if ln::has(&h, egui::accesskit::Role::CheckBox, "Invert") { egui::accesskit::Role::CheckBox } else { egui::accesskit::Role::Button };
    ln::click(&mut h, invert, "Invert");
    assert!(ln::item(&h, id).wrap.invert);
}

/// Whether `n` is a field's own node, not the text part a framework text field holds inside itself: both read as text
/// inputs, the part as a child of its field.
fn field_node(n: &egui_kittest::Node<'_>) -> bool {
    !n.parent().is_some_and(|p| p.role() == egui::accesskit::Role::TextInput)
}

/// The field named `name` (a number field or a text field), at its text part when it has one.
fn named_field<'h>(h: &'h Harness<'_, DesignApp>, name: &str) -> egui_kittest::Node<'h> {
    h.query_all_by(|n| matches!(n.role(), egui::accesskit::Role::TextInput | egui::accesskit::Role::SpinButton) && n.label().as_deref() == Some(name))
        .last()
        .unwrap_or_else(|| panic!("no field named {name}"))
}
