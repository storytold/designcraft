use egui_kittest::kittest::Queryable;

use super::object::{preview_row, transform_anchors};
use super::*;

// The native-body tests that failed on the framework once the native bodies were deleted moved to their successors of
// the same names in `successor_tests.rs`: `a_manual_kerning_value_at_a_caret_kerns_the_pair`,
// `a_tint_on_several_objects_keeps_each_ones_swatch`, `apply_to_text_colours_the_text_of_several_text_frames`,
// `character_panel_has_kerning_and_takes_a_typed_manual_value`, `character_size_field_reads_the_unit_typed`,
// `clicking_a_swatch_chip_in_the_appearance_fill_menu_applies_it`, `more_options_grid_button_keeps_first_line_only`,
// `one_selected_point_shows_its_frames_size_greyed`, `paragraph_panel_is_laid_out_as_indesigns_with_its_panel_menu`,
// `paragraph_panel_keeps_first_line_only`, `paragraph_panel_menu_define_lists_opens_its_dialog`,
// `paragraph_panel_menu_toggles_ignore_optical_margin`, `properties_kerning_menu_sets_zero_kerning`,
// `properties_text_frame_inset_link_of_one_frame_does_not_carry_to_a_text_selection`,
// `properties_text_frame_inset_link_starts_off_when_another_frame_has_uneven_sides`,
// `properties_text_frame_one_inset_keeps_the_sides_of_a_rectangle_in_the_selection`,
// `properties_text_frame_one_inset_lowers_a_uniform_inset_on_every_side`,
// `properties_text_frame_sets_inset_spacing_and_vertical_justification`,
// `properties_text_frame_shows_no_insets_for_type_on_a_path`, `properties_text_frame_shows_one_inset_for_an_oval`,
// `properties_text_frame_shows_while_typing_and_changes_the_caret_frame`,
// `properties_text_wrap_shows_and_sets_the_offsets`, `the_bulleted_list_button_applies_a_hanging_indent`,
// `the_corner_shape_list_has_glyph_rows_and_picks_a_shape`, `the_fill_menu_sets_a_tint`,
// `transform_flips_and_rotates_from_properties`.

/// A link to a panel whose flyout is open leaves the flyout open.
#[test]
fn a_panel_link_keeps_its_open_flyout() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    show_panel(&mut app, "stroke");
    assert_eq!(app.ui.docking_flyout.as_deref(), Some("stroke"));
    show_panel(&mut app, "stroke");
    assert!(crate::panel_docking::showing(&app, "stroke"));
}

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
    // The Page Size section's heading and its preset select share the name.
    assert!(h.query_all_by_label("Page Size").next().is_some(), "Page Size");
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

fn text_app() -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "AVATAR", "caret": true})).unwrap();
    app.run("edit.selectAll", json!({})).unwrap();
    app
}

fn undo_steps(app: &DesignApp) -> usize {
    app.session.active().unwrap().history.undo.len()
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

/// The Align To button shows the target picked in its menu.
#[test]
fn the_align_to_button_shows_its_target() {
    assert_eq!(align_to_icon("selection"), "align-to");
    assert_eq!(align_to_icon("keyObject"), "align-to-key");
    assert_eq!(align_to_icon("margins"), "align-to-margins");
    assert_eq!(align_to_icon("page"), "align-to-page");
    assert_eq!(align_to_icon("spread"), "align-to-spread");
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

/// With the Type tool, the Text Frame section shows the frame Text Frame Options change: the frame
/// the caret is drawn in, not the caret's stored frame.
#[test]
fn text_frame_section_shows_the_frame_that_shows_the_caret() {
    use designcraft_doc::{ItemId, Selection, StoryId, TextSel};
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let text = (0..40).map(|i| format!("Line {i} of the threaded story.")).collect::<Vec<_>>().join(" ");
    let a = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": text})).unwrap();
    let b = app.run("frame.create", json!({"rect": [36, 200, 300, 700], "content": "text", "text": "", "caret": false})).unwrap();
    let (a, b, sid) = (a["id"].as_u64().unwrap(), b["id"].as_u64().unwrap(), a["story"].as_u64().unwrap());
    app.run("text.thread", json!({"from": a, "to": b})).unwrap();
    app.run("object.textFrameOptions", json!({"ids": [b], "columns": 3})).unwrap();
    // The end of the story is drawn in the second frame; the caret's stored frame is the first.
    app.session.doc_mut().unwrap().selection =
        Selection::text(TextSel { story: StoryId(sid), anchor: text.len(), focus: text.len(), frame: Some(ItemId(a)), cell: None });
    let shown = super::text::text_frame_target(&app).map(|(id, o, _)| (id, o.columns));
    assert_eq!(shown, Some((ItemId(b), 3)));
}
