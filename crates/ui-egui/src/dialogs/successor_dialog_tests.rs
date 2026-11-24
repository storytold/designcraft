//! More successors of the native dialog tests of `dialogs/tests.rs`, on the framework path (see `successor_tests`). They
//! cover the Numbering & Section Options window, the Color Picker, Hyphenation, Paragraph and Character Style Options and
//! their preview, Find/Change's formats, Preferences, New Document and the dialog window itself (its title, its size and
//! its move). The name of each test says which old test it replaces; the list is in
//! `work/ui-port-design/TEST-SUCCESSORS-wave4.md`.

use super::table_layout_kit as kit;
use super::*;
use egui::accesskit::Role;
use egui::{Key, Modifiers, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use serde_json::{Value, json};

/// What a failed fit check needs to be traced: the window, the dialog titled `title`, and the named nodes that lie
/// partly or wholly outside the window (the widest first).
fn fit_report(h: &egui_kittest::Harness<'_, DesignApp>, title: &str) -> String {
    let screen = kit::screen(h);
    let dialog = h.query_all_by_role(Role::Dialog).find(|node| node.accesskit_node().label().as_deref() == Some(title)).map(|node| node.rect());
    let mut outside: Vec<(f32, String)> = h
        .query_all_by(|node| node.label().is_some())
        .filter(|node| !screen.contains_rect(node.rect()) && dialog.is_some_and(|d| d.intersects(node.rect())))
        .map(|node| (node.rect().width(), format!("{:?} {:?} {:?}", node.accesskit_node().role(), node.accesskit_node().label(), node.rect())))
        .collect();
    outside.sort_by(|a, b| b.0.total_cmp(&a.0));
    let outside: Vec<String> = outside.into_iter().take(12).map(|(_, node)| node).collect();
    let actions: Vec<String> = ["Create", "Close", "OK", "Cancel"]
        .into_iter()
        .flat_map(|name| h.query_all_by_label(name).map(move |node| format!("{name}: {:?} {:?}", node.accesskit_node().role(), node.rect())))
        .collect();
    format!("window {screen:?}, dialog {dialog:?}, the actions: {actions:?}, outside the window: {outside:#?}")
}

// ---------------------------------------------------------------------------------------------------------------------
// Numbering & Section Options

/// Successor of `enter_and_escape_close_the_style_menu_not_numbering_and_section_options`: with the page numbering Style
/// menu open, Enter and Escape belong to the menu and leave the dialog open without starting a section at 0; with no menu
/// open, Escape closes the dialog.
#[test]
fn section_form_enter_and_escape_belong_to_the_open_style_menu() {
    let mut app = kit::app(2, false);
    kit::open(&mut app, "layout.section");
    let mut h = kit::harness(app);
    for pressed in [Key::Enter, Key::Escape] {
        // The Style menu is named by its group, as the framework form names it: Page Numbering.
        kit::click(&mut h, &[Role::ComboBox], "Page Numbering", 0);
        kit::key(&mut h, pressed, Modifiers::NONE);
        assert!(h.state().ui.dialog.is_some(), "{pressed:?} with the menu open leaves the dialog open");
        assert!(h.state().session.active().unwrap().doc.sections.iter().all(|s| s.start_number != Some(0)));
        // A menu that is still open is closed before the next round.
        if h.query_all_by_role(Role::ListBox).next().is_some() {
            kit::key(&mut h, Key::Escape, Modifiers::NONE);
        }
    }
    kit::key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().ui.dialog.is_none(), "with no menu open, Escape closes the dialog");
}

/// Successor of `numbering_and_section_options_keep_their_actions_in_view`: in English and Arabic, at a large and a very
/// small window, the title, OK, Cancel and Start Section are in the window; at the large one every option shows without
/// scrolling, and choosing one then OK applies it.
#[test]
fn section_form_keeps_its_actions_in_view() {
    for language in ["en", "ar"] {
        for size in [vec2(1178.0, 814.0), vec2(392.0, 271.0)] {
            let mut app = kit::app(2, false);
            app.ui.language = language.into();
            kit::open(&mut app, "layout.section");
            let mut h = kit::harness_sized(app, size);
            let tr = |s| crate::i18n::tr(language, s);
            // The framework window is titled with the "Numbering & Section Options" key, which Arabic words differently
            // from the menu item's; either wording is the title.
            let titles = [tr("Numbering & Section Options"), tr("Numbering & Section Options…").trim_end_matches('…')];
            assert!(titles.iter().any(|title| kit::in_view(&h, title)), "{language} {size:?}: the title {titles:?} is in the window");
            for expected in [tr("OK"), tr("Cancel"), tr("Start Section")] {
                assert!(kit::in_view(&h, expected), "{language} {size:?}: {expected} is in the window");
            }
            if size.x > 1000.0 {
                assert!(
                    kit::in_view(&h, &format!("{} {}", tr("Book Name:"), tr("N/A"))) || kit::in_view(&h, tr("Book Name:")),
                    "{language}: Book Name shows"
                );
                kit::click_first(&mut h, tr("Automatic Page Numbering"));
                kit::click(&mut h, &[Role::Button], tr("OK"), 0);
                assert!(h.state().ui.dialog.is_none(), "{language}: OK closes the dialog");
                let doc = &h.state().session.active().unwrap().doc;
                assert!(doc.sections.iter().all(|s| s.start != 0 || s.start_number.is_none()), "{language}: page 1 numbers automatically");
            }
        }
    }
}

/// Successor of `on_a_page_without_a_section_the_window_is_new_section`: on a page with no section the window is New Section
/// with Start Section on and 1 offered; typing a number chooses Start Page Numbering at, and OK makes page 3 page 5; reopened
/// on that page it is Numbering & Section Options and shows the section.
#[test]
fn section_form_on_a_page_without_a_section_is_new_section() {
    // Chris Nielsen's "Numbering Page Sections" (4:43): page 3 selected, the Pages panel menu's Numbering & Section Options
    // opens New Section with Start Section on and 1 offered.
    let mut app = kit::app(6, true);
    crate::canvas::go_to_page(&mut app, 2);
    kit::open(&mut app, "layout.section");
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!(
        (d.n("page"), d.b("newSection"), d.b("startSection"), d.b("automatic"), d.s("startNumber")),
        (Some(3.0), true, true, true, "1".into())
    );
    let mut h = kit::harness_sized(app, vec2(1178.0, 814.0));
    assert!(kit::shows(&h, "New Section"));
    kit::type_and_commit(&mut h, "Start Page Numbering at:", 0, "5");
    assert!(!kit::draft(&h).b("automatic"), "typing chose Start Page Numbering at");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!(h.state().session.active().unwrap().doc.page_name(2), "5");
    // Opened again on that page, it is Numbering & Section Options, showing the section.
    kit::reopen(&mut h, "layout.section");
    let d = kit::draft(&h);
    assert_eq!((d.b("newSection"), d.b("startSection"), d.b("automatic"), d.s("startNumber")), (false, true, false, "5".into()));
    assert!(kit::shows(&h, "Numbering & Section Options"));
    assert!(!kit::shows(&h, "New Section"));
}

// ---------------------------------------------------------------------------------------------------------------------
// Color Picker

/// A document with a selected frame, and the Color Picker open on its fill with `fields`.
fn picker(fields: Value) -> (egui_kittest::Harness<'static, DesignApp>, u64) {
    let mut app = kit::app(1, false);
    let id = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    app.ui.dialog = Some(Dialog::new("colorPicker", fields));
    (kit::harness_sized(app, vec2(1100.0, 900.0)), id)
}

fn fill(h: &egui_kittest::Harness<'_, DesignApp>, id: u64) -> designcraft_color::Color {
    let doc = &h.state().session.active().unwrap().doc;
    let swatch = doc.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch.clone();
    doc.resolve_color(&swatch, 1.0).unwrap()
}

fn fill_hex(h: &egui_kittest::Harness<'_, DesignApp>, id: u64) -> Option<String> {
    let doc = &h.state().session.active().unwrap().doc;
    let swatch = doc.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch.clone();
    doc.resolve_color(&swatch, 1.0).map(|color| color.to_hex())
}

/// Successor of `the_color_picker_enter_confirms_and_escape_cancels_even_in_a_field`: with a field of the picker holding a
/// typed value, Enter confirms the dialog and the fill takes it, and Escape cancels and leaves the fill.
#[test]
fn color_picker_form_enter_confirms_and_escape_cancels_even_in_a_field() {
    for (pressed, applied) in [(Key::Enter, true), (Key::Escape, false)] {
        let (mut h, id) = picker(json!({"target": "fill", "hex": "#ffffff"}));
        let before = fill_hex(&h, id);
        kit::click(&mut h, &[Role::SpinButton], "Red", 0);
        kit::key(&mut h, Key::A, Modifiers::COMMAND);
        kit::type_text(&mut h, "245");
        kit::key(&mut h, pressed, Modifiers::NONE);
        assert!(h.state().ui.dialog.is_none(), "{pressed:?} closes the picker");
        let hex = fill_hex(&h, id);
        assert_eq!(hex.as_deref() == Some("#f5ffff"), applied, "{pressed:?}: {hex:?} (was {before:?})");
    }
}

/// Successor of `the_hex_field_keeps_typed_text_and_commits_a_valid_code`: a hex code typed in the picker's hex field takes
/// effect when it is a whole #rrggbb, and text that is no colour is replaced by the colour when focus leaves.
#[test]
fn color_picker_form_hex_field_commits_a_valid_code_and_replaces_an_invalid_one() {
    let (mut h, _) = picker(json!({"target": "fill", "hex": "#ffffff"}));
    kit::retype_showing(&mut h, "ffffff", "#f59019");
    assert_eq!(kit::draft(&h).s("hex"), "#f59019", "a 7-character #rrggbb commits");
    // Invalid text is replaced by the colour when focus leaves.
    kit::retype_showing(&mut h, "f59019", "zz");
    assert_eq!(kit::draft(&h).s("hex"), "#f59019", "the colour is kept");
    let _ = kit::field_showing(&h, "f59019");
}

/// Successor of `typed_channel_values_are_kept_while_typing`: a typed 12.5 in the R field makes the colour #0d0203, and OK gives the
/// fill that red. The framework picker's RGB fields take whole numbers, as InDesign's do, so 12.5 is kept as 13 (the native field
/// kept the half).
#[test]
fn color_picker_form_keeps_a_typed_channel_value() {
    let (mut h, id) = picker(json!({"target": "fill", "hex": "#010203", "model": "rgb", "rgb": [1, 2, 3]}));
    kit::type_and_commit(&mut h, "Red", 0, "12.5");
    assert_eq!(kit::draft(&h).fields["rgb"][1], json!(2.0));
    assert_eq!(kit::draft(&h).s("hex"), "#0d0203");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let designcraft_color::Color::Rgb { r, .. } = fill(&h, id) else { panic!("not RGB") };
    // Whole numbers: 12.5 is taken as 13.
    assert!((r - 13.0 / 255.0).abs() < 1e-6, "{r}");
}

/// Successor of `a_cmyk_colour_stays_cmyk_unless_rgb_values_are_typed`: a colour picked by hex is converted to CMYK, a typed
/// ink is a percent (M 1 is 1 %), and typed R, G or B values make it RGB.
#[test]
fn color_picker_form_cmyk_stays_cmyk_unless_rgb_values_are_typed() {
    // The fields the Fill chip opens the picker with for C0 M50 Y100 K0 (`picker_fields`): the hex is the inks' display
    // colour, so the picker keeps the inks as typed.
    let cmyk = crate::panels::swatches::picker_fields(Some(designcraft_color::Color::cmyk(0.0, 0.5, 1.0, 0.0)), false);
    let hex = cmyk["hex"].as_str().unwrap().to_owned();
    // Picked by hex: converted to CMYK.
    let (mut h, id) = picker(cmyk.clone());
    kit::retype_showing(&mut h, &hex, "3366cc");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(matches!(fill(&h, id), designcraft_color::Color::Cmyk { .. }), "{:?}", fill(&h, id));
    // A typed C/M/Y/K percent is a percent: M 1 is 1 %.
    let (mut h, id) = picker(cmyk.clone());
    kit::type_and_commit(&mut h, "Magenta", 0, "1");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    let designcraft_color::Color::Cmyk { c, m, y, k } = fill(&h, id) else { panic!("not CMYK") };
    assert!(c == 0.0 && (m - 0.01).abs() < 1e-6 && (y - 1.0).abs() < 1e-6 && k == 0.0, "{c} {m} {y} {k}");
    // Typed R/G/B values: RGB.
    let (mut h, id) = picker(cmyk);
    kit::type_and_commit(&mut h, "Red", 0, "10");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(matches!(fill(&h, id), designcraft_color::Color::Rgb { .. }), "{:?}", fill(&h, id));
}

/// Successor of `ok_without_edits_leaves_the_fill_alone`: OK on a picker nothing was typed in leaves the fill as it was.
#[test]
fn color_picker_form_ok_without_edits_leaves_the_fill_alone() {
    let (mut h, id) = picker(json!({"target": "fill", "hex": "#ff8000", "model": "cmyk", "cmyk": [0, 50, 100, 0]}));
    let before = h.state().session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().fill.clone();
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!(h.state().session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().fill, before);
}

/// Successor of `dialogs_do_not_dim_the_page`: a framework dialog paints no shape over the whole window, so the page behind
/// it is what its colours are judged against.
#[test]
fn framework_dialogs_do_not_dim_the_page() {
    let (mut h, _) = picker(json!({"target": "fill", "hex": "#ff8000"}));
    let screen = kit::screen(&h);
    let ctx = h.ctx.clone();
    let mut input = egui::RawInput { screen_rect: Some(screen), max_texture_side: Some(8192), time: Some(1.0e4), ..Default::default() };
    h.state_mut().raw_input_hook(&ctx, &mut input);
    let app = h.state_mut();
    let output = ctx.run_ui(input, |ui| show(app, ui.ctx()));
    fn covers(shape: &egui::Shape, screen: egui::Rect) -> bool {
        match shape {
            egui::Shape::Rect(r) => r.rect.contains_rect(screen) && r.fill.a() > 0,
            egui::Shape::Vec(v) => v.iter().any(|s| covers(s, screen)),
            _ => false,
        }
    }
    assert!(h.state().ui.dialog.is_some());
    assert!(!output.shapes.iter().any(|s| covers(&s.shape, screen)), "nothing paints over the whole window");
}

// ---------------------------------------------------------------------------------------------------------------------
// Paragraph and Character Style Options, Hyphenation

/// Successor of `paragraph_style_options_ignore_optical_margin_is_set_on_ok`.
#[test]
fn paragraph_style_options_form_sets_ignore_optical_margin_on_ok() {
    let mut app = kit::app(1, false);
    app.run("style.paragraph.create", json!({"name": "Flush"})).unwrap();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Flush", "section": "indents"})));
    let mut h = kit::harness_sized(app, vec2(1000.0, 800.0));
    kit::click(&mut h, &[Role::CheckBox], "Ignore Optical Margin", 0);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!(h.state().session.doc().unwrap().doc.styles.para("Flush").unwrap().para.ignore_optical_margin, Some(true));
}

/// Successor of `hyphenation_settings_mixed_slider_click_sets_the_step`: with the selection's paragraphs differing, a click on
/// the Better Spacing to Fewer Hyphens slider sets the middle step.
#[test]
fn hyphenation_form_mixed_slider_click_sets_the_step() {
    let (mut app, story) = kit::two_paragraphs();
    app.run("type.para", json!({"attrs": {"hyphWeight": 0.2}})).unwrap();
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 7})).unwrap();
    app.run("app.hyphenationDialog", json!({})).unwrap();
    let mut h = kit::harness_sized(app, vec2(1400.0, 900.0));
    kit::click(&mut h, &[Role::Slider], "Hyphenation Weight", 0);
    assert_eq!(kit::draft(&h).fields.get("hyphWeight"), Some(&json!(0.5)));
}

/// Successor of `hyphenation_settings_mixed_checkbox_and_refused_settings`: a mixed Hyphenate turns on at the first click; a
/// setting the dialog refuses shows no preview and gives the reason, OK with it keeps the dialog open, and corrected it
/// applies.
#[test]
fn hyphenation_form_mixed_checkbox_and_refused_settings() {
    let (mut app, story) = kit::two_paragraphs();
    app.run("type.para", json!({"attrs": {"hyphenate": false}})).unwrap();
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 7})).unwrap();
    app.run("app.hyphenationDialog", json!({})).unwrap();
    assert!(app.ui.dialog.as_ref().unwrap().fields["current"]["hyphenate"].is_null(), "mixed");
    let mut h = kit::harness_sized(app, vec2(1400.0, 900.0));
    kit::click(&mut h, &[Role::CheckBox], "Hyphenate", 0);
    assert_eq!(kit::draft(&h).fields.get("hyphenate"), Some(&json!(true)), "the first click turns it on");
    // A refused setting (as a control-channel caller could send it): no preview, the reason.
    let before = kit::undo_steps(&h);
    let d = h.state_mut().ui.dialog.as_mut().unwrap();
    d.fields.insert("hyphWeight".into(), json!("heavy"));
    d.fields.insert("preview".into(), json!(true));
    kit::steps(&mut h, 2);
    let d = kit::draft(&h);
    assert!(!d.fields.contains_key(PREVIEWED) && d.fields.contains_key("status"), "{:?}", d.fields);
    assert!(h.state().session.doc().unwrap().interaction.is_none(), "the preview went back");
    kit::steps(&mut h, 2);
    assert!(h.state().session.doc().unwrap().interaction.is_none(), "not tried again");
    // OK with it: an error, and the dialog stays open with the reason.
    kit::click(&mut h, &[Role::Button], "OK", 0);
    let d = h.state().ui.dialog.as_ref().expect("the dialog stays open");
    assert!(d.fields.contains_key("status"));
    assert_eq!(kit::undo_steps(&h), before);
    // Corrected, it previews and applies.
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("hyphWeight".into(), json!(0.9));
    kit::steps(&mut h, 2);
    assert!(!kit::draft(&h).fields.contains_key("status"));
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!(kit::para_attr(&mut h, story, 0, "hyphenate"), true);
    assert_eq!(kit::para_attr(&mut h, story, 4, "hyphWeight"), 0.9);
}

/// Successor of `paragraph_style_options_justification_fits_without_horizontal_scrolling`: the Justification section's
/// Minimum and Desired headers sit close together (the spacing table is its own grid) and Maximum and every control label are
/// in the window, so nothing needs a horizontal scroll to be reached.
#[test]
fn paragraph_style_options_form_justification_fits_without_horizontal_scrolling() {
    let mut app = kit::app(1, false);
    app.run("style.paragraph.create", json!({"name": "Headline"})).unwrap();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Headline", "section": "justify"})));
    let h = kit::harness_sized(app, vec2(1200.0, 800.0));
    // Each spacing row's fields are named by their captions, with the colon a field caption has.
    let (minimum, desired) = (kit::rect_of(&h, "Minimum:"), kit::rect_of(&h, "Desired:"));
    let (minimum, desired) = (minimum.expect("Minimum is shown"), desired.expect("Desired is shown"));
    // The spacing table has its own grid: the two headers are one column apart, not a whole stretched column.
    let gap = desired.min.x - minimum.min.x;
    assert!(gap < 200.0, "Minimum/Desired headers {gap:.1} pt apart: {minimum:?} {desired:?}");
    for expected in ["Maximum:", "Single Word Justification:", "Composer:", "Insert Kashidas:", "Korean Line Breaks:"] {
        assert!(kit::in_view(&h, expected), "{expected} is in the window");
    }
}

/// Successor of `style_options_preview_choice_is_remembered_on_reopen`: Preview stays checked when the dialog is reopened.
#[test]
fn style_options_form_preview_choice_is_remembered_on_reopen() {
    let mut app = kit::app(1, false);
    app.run("style.paragraph.create", json!({"name":"Tutorial","chars":{"size":10}})).unwrap();
    open_style_options(&mut app, true, "Tutorial");
    let mut h = kit::harness_sized(app, vec2(1000.0, 800.0));
    kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
    cancel(h.state_mut());
    open_style_options(h.state_mut(), true, "Tutorial");
    assert!(h.state().ui.dialog.as_ref().unwrap().b("preview"), "Preview stays checked on reopen");
}

/// Successor of `style_options_preview_updates_all_uses_cancels_and_commits_once`, for paragraph and character styles: Preview
/// shows an edited definition on every use without an undo step or a journal entry, repeated previews replace each other,
/// Preview off and Cancel put the document back, and OK commits once.
#[test]
fn style_options_form_preview_updates_all_uses_cancels_and_commits_once() {
    for paragraph in [true, false] {
        let mut app = kit::app(1, false);
        let f = app.run("frame.create", json!({"rect":[36,36,300,300],"content":"text","text":"First\nSecond"})).unwrap();
        app.run("text.select", json!({"story":f["story"],"anchor":0,"focus":12})).unwrap();
        let kind = if paragraph { "paragraph" } else { "character" };
        app.run(&format!("style.{kind}.create"), json!({"name":"Tutorial","chars":{"size":10}})).unwrap();
        app.run(&format!("style.{kind}.apply"), json!({"name":"Tutorial"})).unwrap();
        let before = app.session.doc().unwrap().doc.clone();
        let undo = app.session.doc().unwrap().history.undo.len();
        let journal = app.session.journal.len();
        open_style_options(&mut app, paragraph, "Tutorial");
        let mut h = kit::harness_sized(app, vec2(1000.0, 800.0));
        kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
        h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("c.size".into(), json!(18));
        kit::steps(&mut h, 3);
        let app = h.state_mut();
        assert_eq!(app.run("type.selectionAttrs", json!({})).unwrap()["chars"]["size"], 18.0);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), undo);
        assert_eq!(app.session.journal.len(), journal);
        // Repeated previews replace each other, never accumulate undo entries.
        app.ui.dialog.as_mut().unwrap().fields.insert("c.size".into(), json!(20));
        kit::steps(&mut h, 3);
        assert_eq!(h.state_mut().run("type.selectionAttrs", json!({})).unwrap()["chars"]["size"], 20.0);
        kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
        assert_eq!(h.state().session.doc().unwrap().doc, before, "Preview off puts the document back");
        kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
        cancel(h.state_mut());
        kit::steps(&mut h, 2);
        assert_eq!(h.state().session.doc().unwrap().doc, before);
        assert_eq!(h.state().session.doc().unwrap().history.undo.len(), undo);
        open_style_options(h.state_mut(), paragraph, "Tutorial");
        kit::steps(&mut h, 2);
        let d = h.state_mut().ui.dialog.as_mut().unwrap();
        d.fields.insert("preview".into(), json!(true));
        d.fields.insert("c.size".into(), json!(24));
        kit::steps(&mut h, 3);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().session.doc().unwrap().history.undo.len(), undo + 1);
        // Both paragraphs/character runs still use the globally edited definition.
        for pos in [0, 6] {
            h.state_mut().run("text.select", json!({"story":f["story"],"anchor":pos,"focus":pos+1})).unwrap();
            assert_eq!(h.state_mut().run("type.selectionAttrs", json!({})).unwrap()["chars"]["size"], 24.0);
        }
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.doc().unwrap().doc, before);
    }
}

/// Successor of `a_new_character_style_starts_from_the_selected_text`: New Character Style starts from the selected text's
/// character attributes (not the paragraph style's), applies to the selection, and OK with a name makes the style.
#[test]
fn new_character_style_form_starts_from_the_selected_text() {
    let mut app = kit::app(1, false);
    let f = app.run("frame.create", json!({"rect": [36, 36, 400, 300], "content": "text", "text": "Drop eggs into boiling water."})).unwrap();
    app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 29})).unwrap();
    app.run("style.paragraph.create", json!({"name": "Recipe", "chars": {"size": 14}})).unwrap();
    app.run("style.paragraph.apply", json!({"name": "Recipe"})).unwrap();
    app.run("text.select", json!({"story": f["story"], "anchor": 5, "focus": 9})).unwrap();
    app.run("type.char", json!({"attrs": {"fontStyle": "Bold", "fill": "C=15 M=100 Y=100 K=0"}})).unwrap();
    open_new_style(&mut app, false);
    let d = app.ui.dialog.clone().unwrap();
    assert_eq!((d.fields["c.fontStyle"].clone(), d.fields["c.fill"].clone()), (json!("Bold"), json!("C=15 M=100 Y=100 K=0")));
    assert!(d.b("apply"));
    let mut h = kit::harness_sized(app, vec2(1000.0, 700.0));
    assert!(kit::shows(&h, "New Character Style"));
    assert!(kit::shows(&h, "Apply Style to Selection"));
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("rename".into(), json!("eggs"));
    kit::steps(&mut h, 2);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let app = h.state_mut();
    let style = app.session.doc().unwrap().doc.styles.char_style("eggs").cloned().unwrap();
    assert_eq!((style.chars.font_style.as_deref(), style.chars.fill.as_deref()), (Some("Bold"), Some("C=15 M=100 Y=100 K=0")));
    assert_eq!(style.chars.size, None, "the size is the paragraph style's, not the text's own");
    let a = app.run("type.selectionAttrs", json!({})).unwrap();
    assert_eq!((a["characterStyle"].clone(), a["charOverrides"].clone()), (json!("eggs"), json!(0)), "applied to the selection");
}

/// Successor of `the_rule_colour_menu_offers_text_color`: the Color menu of a paragraph rule offers the text's own colour,
/// and picking it stores it.
#[test]
fn paragraph_rules_form_color_menu_offers_text_color() {
    let (mut app, story) = kit::two_paragraphs();
    let rule = json!({"on": true, "weight": 3.0, "color": "[Black]"});
    app.run("type.para", json!({"attrs": {"ruleBelow": rule}})).unwrap();
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
    app.run("app.paragraphRulesDialog", json!({"rule": "ruleBelow"})).unwrap();
    let mut h = kit::harness(app);
    kit::pick(&mut h, "Color:", "Text Color");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let below = kit::para_attr(&mut h, story, 0, "ruleBelow");
    assert_eq!(below["color"].as_str(), Some(designcraft_doc::TEXT_COLOR));
}

// ---------------------------------------------------------------------------------------------------------------------
// Find/Change formats

/// A text frame holding `text`, Find/Change open with "eggs" to find and a character style "eggs" (Bold).
fn format_app(text: &str) -> (DesignApp, u64) {
    let mut app = kit::app(1, false);
    let f = app.run("frame.create", json!({"rect": [36, 36, 400, 300], "content": "text", "text": text})).unwrap();
    app.run("style.character.create", json!({"name": "eggs", "chars": {"fontStyle": "Bold"}})).unwrap();
    (app, f["story"].as_u64().unwrap())
}

fn field(h: &egui_kittest::Harness<'_, DesignApp>, key: &str) -> Option<Value> {
    h.state().ui.dialog.as_ref().unwrap().fields.get(key).cloned()
}

/// Clicks the button named `label` of the dialog titled `title`, not the button of that name of a dialog under it: one
/// in the dialog's own nodes, else one inside its bounds.
fn click_in_dialog(h: &mut egui_kittest::Harness<'_, DesignApp>, title: &str, label: &str) {
    let at = {
        let dialog = h.get_by_role_and_label(Role::Dialog, title);
        let bounds = dialog.rect();
        let own: Vec<egui::Pos2> = dialog.query_all_by_role_and_label(Role::Button, label).map(|node| node.rect().center()).collect();
        own.last().copied().unwrap_or_else(|| {
            h.query_all_by_role_and_label(Role::Button, label)
                .map(|node| node.rect())
                .filter(|rect| bounds.contains_rect(*rect))
                .last()
                .unwrap_or_else(|| panic!("no button {label:?} in the dialog {title:?}"))
                .center()
        })
    };
    kit::click_at(h, at);
}

/// Successor of `find_change_gives_the_found_text_a_change_format`: Specify attributes to change opens the Change Format
/// Settings, picking the eggs character style and OK makes it the Change Format (the box and Change to's help show it),
/// Escape closes the settings only, and Change All formats every lowercase "eggs" and leaves the words.
#[test]
fn find_change_form_gives_the_found_text_a_change_format() {
    let text = "Poached Eggs\nBreak eggs; cook until eggs are set.";
    let (mut app, story) = format_app(text);
    app.run("app.findChange", json!({})).unwrap();
    let d = app.ui.dialog.as_mut().unwrap();
    d.fields.insert("find".into(), json!("eggs"));
    d.fields.insert("caseSensitive".into(), json!(true));
    let mut h = kit::harness_sized(app, vec2(1200.0, 900.0));
    const TIP: &str = "With a Change Format and nothing in Change to, the found text stays and is only formatted";
    assert!(kit::shows(&h, "Change Format:"));
    assert!(!kit::shows(&h, TIP), "no help without a Change Format");
    kit::click(&mut h, &[Role::Button], "Specify attributes to change", 0);
    for label in ["Change Format Settings", "Style Options", "Basic Character Formats", "Character Color"] {
        assert!(kit::shows(&h, label), "{label}");
    }
    // Style Options > Character Style: eggs.
    kit::pick(&mut h, "Character Style:", "eggs");
    // The settings' own OK: Find/Change under it has an OK of its own.
    click_in_dialog(&mut h, "Change Format Settings", "OK");
    assert_eq!((field(&h, CHANGE_FORMAT), field(&h, FORMAT_EDIT)), (Some(json!({"charStyle": "eggs"})), None));
    assert!(kit::shows(&h, "Character Style: eggs"), "the box shows it");
    assert!(kit::shows(&h, TIP), "Change to's help");
    // Escape closes Change Format Settings only, and keeps the Change Format.
    kit::click(&mut h, &[Role::Button], "Specify attributes to change", 0);
    kit::key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some("findChange"));
    assert_eq!((field(&h, CHANGE_FORMAT), field(&h, FORMAT_EDIT)), (Some(json!({"charStyle": "eggs"})), None));
    // Change All: the words stay, every lowercase "eggs" has the style.
    kit::click(&mut h, &[Role::Button], "Change All", 0);
    assert_eq!(field(&h, "status"), Some(json!("2 replacement(s) made")));
    let sid = designcraft_doc::StoryId(story);
    let story = h.state().session.doc().unwrap().doc.story(sid).cloned().unwrap();
    assert_eq!(story.text, text);
    assert_eq!(
        [8, 19, 36].map(|at| story.format_after(at).style.clone()),
        [designcraft_doc::NO_CHAR_STYLE.to_string(), "eggs".into(), "eggs".into()]
    );
    // The clear button removes it (the second: the first is the Find Format's).
    kit::click(&mut h, &[Role::Button], "Clear specified attributes", 1);
    assert_eq!(field(&h, CHANGE_FORMAT), None);
    assert!(!kit::shows(&h, TIP));
}

/// Successor of `find_format_finds_only_text_with_it`: with a Find Format of the eggs character style, Count finds only the
/// "eggs" in it, Find Next with nothing in Find what finds the text in the style, and the Find Format's clear button removes
/// it.
#[test]
fn find_change_form_find_format_finds_only_text_with_it() {
    let (mut app, story) = format_app("Break eggs; cook eggs.");
    app.run("text.select", json!({"story": story, "anchor": 6, "focus": 10})).unwrap();
    app.run("style.character.apply", json!({"name": "eggs"})).unwrap();
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
    app.run("app.findChange", json!({})).unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("find".into(), json!("eggs"));
    let mut h = kit::harness_sized(app, vec2(1200.0, 900.0));
    const TIP: &str = "With a Find Format, only text that has it is found; with nothing in Find what, all text that has it";
    assert!(kit::shows(&h, "Find Format:") && !kit::shows(&h, TIP));
    kit::click(&mut h, &[Role::Button], "Specify attributes to find", 0);
    assert!(kit::shows(&h, "Find Format Settings"));
    kit::pick(&mut h, "Character Style:", "eggs");
    // The settings' own OK: Find/Change under it has an OK of its own.
    click_in_dialog(&mut h, "Find Format Settings", "OK");
    assert_eq!((field(&h, FIND_FORMAT), field(&h, CHANGE_FORMAT)), (Some(json!({"charStyle": "eggs"})), None));
    assert!(kit::shows(&h, "Character Style: eggs") && kit::shows(&h, TIP));
    kit::click(&mut h, &[Role::Button], "Count", 0);
    assert_eq!(field(&h, "status"), Some(json!("1 match(es)")), "only the \"eggs\" in the style");
    // Nothing in Find what: the text in the style.
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("find".into(), json!(""));
    kit::steps(&mut h, 2);
    kit::click(&mut h, &[Role::Button], "Find Next", 0);
    let sel = h.state().session.doc().unwrap().selection.text.unwrap();
    assert_eq!(sel.range(), 6..10);
    // The first clear button is the Find Format's.
    kit::click(&mut h, &[Role::Button], "Clear specified attributes", 0);
    assert_eq!(field(&h, FIND_FORMAT), None);
}

/// Successor of `change_and_change_find_change_the_found_text`: Change is off until something is found; Change/Find replaces
/// the found text and finds the next, and Change replaces the found text and reports it.
#[test]
fn find_change_form_change_and_change_find_change_the_found_text() {
    let mut app = kit::app(1, false);
    let f = app.run("frame.create", json!({"rect": [36, 36, 400, 300], "content": "text", "text": "red red | red"})).unwrap();
    let sid = designcraft_doc::StoryId(f["story"].as_u64().unwrap());
    app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
    app.run("app.findChange", json!({})).unwrap();
    let d = app.ui.dialog.as_mut().unwrap();
    d.fields.insert("find".into(), json!("red"));
    d.fields.insert("change".into(), json!("blue"));
    let mut h = kit::harness_sized(app, vec2(1200.0, 900.0));
    kit::click(&mut h, &[Role::ComboBox], "Search:", 0);
    for scope in ["Document", "Story", "Selection"] {
        assert!(kit::shows(&h, scope), "the Search choices are InDesign's: {scope}");
    }
    kit::key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().ui.dialog.is_some(), "Escape closed the list only");
    assert!(kit::disabled(&h, &[Role::Button], "Change", 0), "nothing found yet");
    let text = |h: &egui_kittest::Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.story(sid).unwrap().text.clone();
    let selected = |h: &egui_kittest::Harness<'_, DesignApp>| h.state().session.doc().unwrap().selection.text.unwrap().range();
    kit::click(&mut h, &[Role::Button], "Find Next", 0);
    assert_eq!(selected(&h), 0..3);
    kit::click(&mut h, &[Role::Button], "Change/Find", 0);
    assert_eq!((text(&h), selected(&h)), ("blue red | red".to_string(), 5..8));
    kit::click(&mut h, &[Role::Button], "Change", 0);
    assert_eq!((text(&h), selected(&h)), ("blue blue | red".to_string(), 5..9));
    assert_eq!(kit::draft(&h).fields["status"], json!("1 replacement(s) made"));
}

// ---------------------------------------------------------------------------------------------------------------------
// Preferences and New Document

/// Successor of `preferences_with_no_document_set_the_units_of_new_documents`: with no document Preferences lists Units &
/// Increments for new documents; OK saves them, a Cursor Key of 0 or text that is no measure is refused and keeps the dialog
/// and the saved values, Kerning/Tracking of 0.5 is kept, and New Document opens with the new units.
#[test]
fn preferences_form_with_no_document_sets_the_units_of_new_documents() {
    use designcraft_geom::Unit;
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".into();
    crate::menus::activate(&mut app, "app.preferences", &Value::Null);
    assert!(app.ui.dialog.as_ref().unwrap().b("defaults"), "for new documents");
    let mut h = kit::harness_sized(app, vec2(1178.0, 814.0));
    assert!(kit::shows(&h, "Units & Increments"), "listed with no document");
    let d = h.state_mut().ui.dialog.as_mut().unwrap();
    d.fields.insert("horizontalUnits".into(), json!("inches"));
    d.fields.insert("verticalUnits".into(), json!("inches"));
    d.fields.insert("keyboardIncrement".into(), json!("0.5 pt"));
    kit::steps(&mut h, 2);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let du = h.state().session.prefs.default_units;
    assert_eq!((du.horizontal_units, du.vertical_units, du.keyboard_increment), (Unit::Inches, Unit::Inches, 0.5));
    assert!(h.state().session.active().is_none(), "no document made");
    // A Cursor Key of 0 or text that isn't a measure is refused, keeping the dialog open and the saved values, the other
    // preferences' too; a Kerning/Tracking of 0.5 is kept as it is.
    let scale = h.state().session.prefs.scale_strokes;
    for bad in [json!({"keyboardIncrement": "0 pt"}), json!({"keyboardIncrement": "wide"}), json!({"kerningIncrement": "lots"})] {
        let mut d = Dialog::new("preferences", json!({"defaults": true, "horizontalUnits": "inches", "scaleStrokes": !scale}));
        for (k, v) in bad.as_object().unwrap() {
            d.fields.insert(k.clone(), v.clone());
        }
        h.state_mut().ui.dialog = Some(d);
        kit::steps(&mut h, 3);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.as_ref().is_some_and(|d| d.id == "preferences"), "still open: {bad}");
        assert_eq!(h.state().session.prefs.default_units.keyboard_increment, 0.5, "{bad}");
        assert_eq!(h.state().session.prefs.scale_strokes, scale, "no other preference saved: {bad}");
    }
    h.state_mut().ui.dialog = Some(Dialog::new("preferences", json!({"defaults": true, "horizontalUnits": "inches", "kerningIncrement": "0.5"})));
    kit::steps(&mut h, 3);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert_eq!(h.state().session.prefs.default_units.kerning_increment, 0.5, "not rounded up to 1");
    // New Document opens in inches, and the document it makes has inch rulers.
    kit::reopen(&mut h, "app.newDocumentDialog");
    let nd = kit::draft(&h).clone();
    assert_eq!(nd.fields["units"], json!("inches"));
    assert_eq!(nd.s("width"), designcraft_geom::format_measure(612.0, Unit::Inches));
    kit::click(&mut h, &[Role::Button], "Create", 0);
    let st = &h.state().session.doc().unwrap().doc.settings;
    assert_eq!((st.horizontal_units, st.vertical_units, st.keyboard_increment), (Unit::Inches, Unit::Inches, 0.5));
    // With the document open, Preferences edits that document's units, not the defaults.
    kit::reopen(&mut h, "app.preferences");
    assert!(!kit::draft(&h).b("defaults"));
}

/// Successor of `file_handling_shows_the_recent_items_count_as_a_whole_number`: File Handling shows the number of recent items
/// as 20, without a decimal.
#[test]
fn preferences_form_file_handling_shows_the_recent_items_count_as_a_whole_number() {
    let mut app = kit::app(1, false);
    app.run("app.preferences", json!({})).unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("section".into(), json!("files"));
    let h = kit::harness_sized(app, vec2(1000.0, 700.0));
    assert!(kit::shows(&h, "Number of Recent Items to Display:"));
    let count = kit::draft(&h).n("recentFilesMax").unwrap();
    assert_eq!(count, 20.0, "InDesign's default");
    let shown = h
        .query_all_by_label("Number of Recent Items to Display:")
        .filter_map(|node| node.value())
        .chain(h.query_all_by_role(Role::SpinButton).filter_map(|node| node.value()))
        .collect::<Vec<_>>();
    assert!(shown.iter().any(|value| value.trim() == "20"), "20 is shown: {shown:?}");
    assert!(!shown.iter().any(|value| value.trim() == "20.0"), "no decimal shown: {shown:?}");
}

/// Successor of `dialog_title_and_actions_fit_scaled_viewports`: Preferences and New Document keep their title and their two
/// actions inside the window at every size and in both languages.
#[test]
fn framework_dialogs_keep_their_title_and_actions_in_scaled_viewports() {
    for language in ["en", "ar"] {
        for size in [vec2(1178.0, 814.0), vec2(589.0, 407.0), vec2(392.0, 271.0), vec2(360.0, 280.0)] {
            for (command, title, ok, cancel) in
                [("app.preferences", "Preferences", "OK", "Cancel"), ("app.newDocumentDialog", "New Document", "Create", "Close")]
            {
                let mut app = kit::app(1, false);
                app.ui.language = language.into();
                app.run(command, json!({})).unwrap();
                let h = kit::harness_sized(app, size);
                for expected in [title, ok, cancel] {
                    let shown = crate::i18n::tr(language, expected);
                    assert!(
                        kit::in_view(&h, shown),
                        "{language} {size:?} {command}: {shown} is in the window; {}",
                        fit_report(&h, &crate::i18n::tr(language, title))
                    );
                }
            }
        }
    }
}

/// Successor of `preferences_sidebar_scroll_keeps_selected_options_visible`: in a small window the last section of the
/// Preferences list is reached by scrolling the list, choosing it shows its options, and OK and Cancel stay in view.
#[test]
fn preferences_form_sidebar_reaches_the_last_section_in_a_small_window() {
    let mut app = kit::app(1, false);
    app.run("app.preferences", json!({})).unwrap();
    let mut h = kit::harness_sized(app, vec2(589.0, 407.0));
    // The section list is one Tab stop and its arrow keys pick: arrowing down to the last section scrolls the list to it, as
    // the wheel would (the list's own rows take no AccessKit scroll request).
    kit::click(&mut h, &[Role::ListItem], "General", 0);
    for _ in 0..24 {
        kit::key(&mut h, egui::Key::ArrowDown, egui::Modifiers::NONE);
    }
    kit::steps(&mut h, 3);
    let files = kit::reach(&mut h, "File Handling");
    kit::click_at(&mut h, files.center());
    assert_eq!(kit::draft(&h).s("section"), "files");
    for expected in ["Document Recovery Data", "Save recovery data every:", "OK", "Cancel"] {
        assert!(
            kit::in_view(&h, expected) || {
                kit::reach(&mut h, expected);
                true
            },
            "{expected} is reachable"
        );
    }
    assert!(kit::in_view(&h, "OK") && kit::in_view(&h, "Cancel"), "the actions stay in view");
}

/// Successor of `preferences_horizontal_scroll_reaches_clipped_options`: in a very small window the widest General option is
/// reached by scrolling, and clicking it sets it.
#[test]
fn preferences_form_reaches_the_widest_option_in_a_very_small_window() {
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".into();
    app.run("app.preferences", json!({})).unwrap();
    let mut h = kit::harness_sized(app, vec2(360.0, 280.0));
    let option = "Absolute Numbering (instead of Section Numbering)";
    let at = kit::reach(&mut h, option).center();
    for expected in ["Preferences", "OK", "Cancel"] {
        assert!(kit::in_view(&h, expected), "{expected} is in the window");
    }
    kit::click_at(&mut h, at);
    assert!(kit::draft(&h).b("absolutePageNumbers"));
}

/// Scrolls New Document's one column with the wheel, over the column, until `find`'s node lies wholly in the window,
/// as a person reaches a field in a small window; returns its bounds, or panics with where it stayed.
fn wheel_into_view(
    h: &mut egui_kittest::Harness<'_, DesignApp>,
    what: &str,
    find: impl Fn(&egui_kittest::Harness<'_, DesignApp>) -> Option<egui::Rect>,
) -> egui::Rect {
    // The last move: a node scrolled out of the column may leave the tree, and the wheel then goes back the other way.
    let mut last = 0.0_f32;
    for _ in 0..24 {
        let window = kit::screen(h);
        // What the column shows: the dialog under its title and over its footer, inside the window.
        let dialog = h.query_all_by_role(Role::Dialog).next().map_or(window, |node| node.rect()).intersect(window);
        let shown = egui::Rect::from_min_max(egui::pos2(dialog.left(), dialog.top() + 50.0), egui::pos2(dialog.right(), dialog.bottom() - 60.0));
        let delta = match find(h) {
            Some(target) if shown.contains_rect(target) || (window.contains_rect(target) && shown.height() < target.height()) => return target,
            Some(target) => (target.center().y - shown.center().y).clamp(-40.0, 40.0),
            None if last != 0.0 => -last / 2.0,
            None => panic!("{what} is not shown"),
        };
        last = delta;
        // The pointer rests just inside the column's left edge.
        let over = egui::pos2(shown.left() + 12.0, shown.center().y);
        kit::event(h, egui::Event::PointerMoved(over));
        kit::event(
            h,
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -delta),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE,
                source: egui::MouseWheelSource::Unknown,
            },
        );
        kit::steps(h, 2);
    }
    panic!("{what} cannot be reached with the wheel: {:?} in {:?}", find(h), kit::screen(h))
}

/// Successor of `new_document_vertical_scroll_reaches_every_field_and_preset`: in a very small window New Document's
/// Primary Text Frame caption is reached by scrolling and toggles its check box, and the preset cards below the details are
/// reached too.
#[test]
fn new_document_form_vertical_scroll_reaches_every_field_and_preset() {
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".into();
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let mut h = kit::harness_sized(app, vec2(392.0, 271.0));
    for expected in ["New Document", "Create", "Close"] {
        assert!(kit::in_view(&h, expected), "{expected} is in the window; {}", fit_report(&h, "New Document"));
    }
    // The one column scrolls under the title and over the footer: the wheel over it reaches what lies below.
    let at = wheel_into_view(&mut h, "Primary Text Frame", |h| kit::rect_of(h, "Primary Text Frame")).center();
    kit::click_at(&mut h, at);
    assert!(kit::draft(&h).b("primaryTextFrame"), "the caption toggles its checkbox");
    wheel_into_view(&mut h, "[Default]", |h| kit::rect_of(h, "[Default]"));
}

/// Successor of `arabic_new_document_fields_remain_reachable_and_editable`: with the Arabic interface, at three window sizes,
/// the New Document fields are reached by scrolling, numeric fields accept edits and checkboxes stay clickable.
#[test]
fn new_document_form_arabic_fields_remain_reachable_and_editable() {
    for size in [vec2(392.0, 271.0), vec2(360.0, 280.0), vec2(1178.0, 814.0)] {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "ar".into();
        app.run("app.newDocumentDialog", json!({})).unwrap();
        let fields = [("height", "66p1"), ("gutter", "1p2"), ("marginBottom", "3p3"), ("marginOutside", "3p4")];
        for (field, value) in fields {
            app.ui.dialog.as_mut().unwrap().fields.insert(field.into(), json!(value));
        }
        let mut h = kit::harness_sized(app, size);
        // The Height field may start partly below the column's clip: scroll to it with the wheel like the other fields.
        let shows = |value: &'static str| {
            move |h: &egui_kittest::Harness<'_, DesignApp>| {
                h.query_all_by_role(Role::TextInput).find(|node| node.value().as_deref() == Some(value)).map(|node| node.rect())
            }
        };
        wheel_into_view(&mut h, "66p1", shows("66p1"));
        kit::retype_showing(&mut h, "66p1", "88p8");
        assert_eq!(kit::draft(&h).s("height"), "88p8", "numeric fields accept edits");
        let caption = crate::i18n::tr("ar", "Primary Text Frame");
        let at = wheel_into_view(&mut h, caption, |h| kit::rect_of(h, caption)).center();
        kit::click_at(&mut h, at);
        assert!(kit::draft(&h).b("primaryTextFrame"), "checkboxes remain clickable");
        for (_, value) in &fields[1..] {
            let node = h.query_all_by_role(Role::TextInput).find(|node| node.value().as_deref() == Some(*value));
            assert!(node.is_some(), "{value} is shown at {size:?}");
        }
    }
}

/// Successor of `resized_dialogs_cancel_and_reopen_without_applying_edits`: after Preferences and New Document are resized
/// through three sizes, Cancel closes them without creating a document, the reopened dialog shows the original value of an
/// edited field, and Escape closes it.
#[test]
fn framework_dialogs_cancel_and_reopen_without_applying_edits_after_resizing() {
    for (command, title, field, ok, cancel) in
        [("app.preferences", "Preferences", "recoveryMinutes", "OK", "Cancel"), ("app.newDocumentDialog", "New Document", "pages", "Create", "Close")]
    {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".into();
        app.run(command, json!({})).unwrap();
        let original = app.ui.dialog.as_ref().unwrap().fields[field].clone();
        app.ui.dialog.as_mut().unwrap().fields.insert(field.into(), json!(42));
        let mut h = kit::harness_sized(app, vec2(1178.0, 814.0));
        for size in [vec2(1178.0, 814.0), vec2(392.0, 271.0), vec2(589.0, 407.0)] {
            kit::resize(&mut h, size);
            for expected in [title, ok, cancel] {
                assert!(kit::in_view(&h, expected), "{command} {size:?}: {expected} is in the window; {}", fit_report(&h, title));
            }
        }
        kit::click(&mut h, &[Role::Button], cancel, 0);
        assert!(h.state().ui.dialog.is_none());
        assert!(h.state().session.active().is_none(), "cancelling must not create a document");
        kit::reopen(&mut h, command);
        assert_eq!(kit::draft(&h).fields[field], original, "cancelled edits must not survive reopening");
        assert!(kit::in_view(&h, title));
        kit::key(&mut h, Key::Escape, Modifiers::NONE);
        assert!(h.state().ui.dialog.is_none(), "Escape must also close the reopened dialog");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The dialog window

/// The title text of the dialog named `title`: the topmost node of that name that is not the dialog frame (New
/// Document's preset tab strip is named "New Document" too).
fn title_at(h: &egui_kittest::Harness<'_, DesignApp>, title: &str) -> egui::Pos2 {
    h.query_all_by_label(title)
        .filter(|node| node.accesskit_node().role() != Role::Dialog)
        .map(|node| node.rect().center())
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap_or_else(|| panic!("no title {title:?}"))
}

/// Successor of `dragging_the_title_moves_the_dialog_and_dragging_the_body_does_not`: Insert Table starts centred; a drag on
/// its title moves it by the drag, a drag in its body leaves it, the title row stays inside the top edge, and the dialog
/// opens centred again.
#[test]
fn framework_dialog_title_drag_moves_the_dialog_and_body_drag_does_not() {
    let mut app = kit::app(1, false);
    app.ui.dialog = Some(Dialog::new("insertTable", Value::Null));
    let mut h = kit::harness_sized(app, vec2(1200.0, 800.0));
    let centred = kit::dialog_rect(&h, "Insert Table");
    assert!((centred.center() - egui::pos2(600.0, 400.0)).length() < 2.0, "{centred:?}");
    assert!(centred.width() < 1000.0 && centred.height() < 700.0, "the window is the dialog, not a backdrop: {centred:?}");

    let title = title_at(&h, "Insert Table");
    kit::drag(&mut h, title, title + vec2(100.0, 60.0));
    let moved = kit::dialog_rect(&h, "Insert Table");
    assert!((moved.min - centred.min - vec2(100.0, 60.0)).length() < 3.0, "{centred:?} -> {moved:?}");

    let body = egui::pos2(moved.right() - 40.0, moved.center().y);
    kit::drag(&mut h, body, body + vec2(-150.0, 80.0));
    assert_eq!(kit::dialog_rect(&h, "Insert Table"), moved, "a drag in the body leaves the dialog in place");

    // The title row stays inside the top edge.
    let title = title_at(&h, "Insert Table");
    kit::drag(&mut h, title, title - vec2(0.0, 1000.0));
    assert!(kit::dialog_rect(&h, "Insert Table").top().abs() < 2.0, "{:?}", kit::dialog_rect(&h, "Insert Table"));

    // Opened again, the dialog starts centred.
    h.state_mut().ui.dialog = None;
    kit::steps(&mut h, 2);
    h.state_mut().ui.dialog = Some(Dialog::new("insertTable", Value::Null));
    kit::steps(&mut h, 4);
    assert!((kit::dialog_rect(&h, "Insert Table").center() - centred.center()).length() < 2.0, "{:?}", kit::dialog_rect(&h, "Insert Table"));
}

/// Successor of `the_new_document_window_drags_mostly_aside_and_keeps_its_preview`: the New Document window with Preview on can
/// be dragged mostly out of the way, over the preview, and keeps the preview, the dialog and the open documents.
#[test]
fn new_document_form_window_drags_mostly_aside_and_keeps_its_preview() {
    let mut app = kit::app(1, false);
    app.ui.new_document_preview = true;
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let mut h = kit::harness_sized(app, vec2(1400.0, 900.0));
    kit::steps(&mut h, 3);
    let preview = h.state().session.new_document_preview();
    assert!(preview.is_some(), "Preview is on");
    let centred = kit::dialog_rect(&h, "New Document");
    assert!(centred.width() > 900.0, "{centred:?}");
    let from = title_at(&h, "New Document");
    let to = from + vec2(1300.0, 0.0);
    kit::drag(&mut h, from, to);
    // All but a strip of its title is past the right edge, over the preview: at least as far aside as the old window went
    // (100 px left in view), with the strip the framework keeps (`DragBounds::TitleVisible`) still in the window to take
    // it back by.
    let moved = kit::dialog_rect(&h, "New Document");
    let window = kit::screen(&h);
    assert!(moved.left() >= 1300.0 - 3.0 && moved.left() < window.right() && moved.top() >= window.top(), "{moved:?} in {window:?}");
    let app = h.state();
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("newDocument"));
    assert_eq!(app.session.new_document_preview(), preview, "moving the window keeps the preview");
    assert_eq!(app.session.documents().len(), 2);
}

// ---------------------------------------------------------------------------------------------------------------------
// Text Frame Options

/// The text of the field named like `part` (a text field, a number field or a picker).
fn value_of(h: &egui_kittest::Harness<'_, DesignApp>, part: &str) -> String {
    h.query_all_by_label_contains(part)
        .find(|node| kit::FIELD.contains(&node.accesskit_node().role()))
        .unwrap_or_else(|| panic!("no field named like {part:?}"))
        .value()
        .unwrap_or_default()
}

/// Successor of `text_frame_options_dims_the_sides_for_a_non_rectangular_frame`: the four sides are dimmed and one Inset
/// field takes their place for a non-rectangular frame (and the reverse for a rectangle), the General page fits above the
/// buttons, which stay put on the other pages, and the link toggles only for a rectangle.
#[test]
fn text_frame_options_form_dims_the_sides_for_a_non_rectangular_frame() {
    for shaped in [true, false] {
        let mut app = kit::app(1, false);
        let shape = if shaped { "ellipse" } else { "rectangle" };
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "shape": shape, "content": "text", "text": "Hello"})).unwrap();
        app.run("selection.set", json!({"ids": [r["id"]]})).unwrap();
        app.ui.dialog = Some(Dialog::new("textFrameOptions", json!({})));
        let mut h = kit::harness_sized(app, vec2(1000.0, 800.0));
        for name in ["Columns:", "Align:"] {
            let _ = h.get_by_role_and_label(Role::ComboBox, name);
        }
        for side in ["Top", "Left", "Bottom", "Right"] {
            assert_eq!(kit::disabled(&h, kit::FIELD, &format!("{side}:"), 0), shaped, "{shape}: {side}");
        }
        assert_eq!(kit::shows(&h, "Inset:"), shaped, "{shape}: the Inset caption");
        // The General page fits in the dialog's fixed height: the buttons stay put on another page.
        let (wrap, cancel) = (kit::rect_of(&h, "Ignore Text Wrap").unwrap().bottom(), kit::rect_of(&h, "Cancel").unwrap().top());
        assert!(wrap < cancel, "{shape}: Ignore Text Wrap ends at {wrap}, below the top of Cancel at {cancel}");
        for tab in ["Column Rules", "Baseline Options", "Auto-Size"] {
            kit::click(&mut h, &[Role::ListItem], tab, 0);
        }
        assert_eq!(kit::rect_of(&h, "Cancel").unwrap().top(), cancel, "{shape}: the buttons stay put on another page");
        kit::click(&mut h, &[Role::ListItem], "General", 0);
        // The link toggles only for a rectangle.
        let before = kit::draft(&h).b("insetLinked");
        kit::click(&mut h, kit::TOGGLE, "Make all settings the same", 0);
        assert_eq!(kit::draft(&h).b("insetLinked") != before, !shaped, "{shape}: the link");
    }
}

/// Successor of `text_frame_options_follow_the_ruler_units`: with millimetre rulers the inset, gutter and paragraph spacing
/// limit show in millimetres, the column width has the unit, and bare numbers typed in the fields are millimetres.
#[test]
fn text_frame_options_form_follow_the_ruler_units() {
    use designcraft_geom::{Unit, format_measure};
    let mm = |pt: f64| format_measure(pt, Unit::Millimeters);
    for shape in ["rectangle", "ellipse"] {
        let mut app = kit::app(1, false);
        app.run("document.preferences", json!({"horizontalUnits": "millimeters", "verticalUnits": "millimeters"})).unwrap();
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "shape": shape, "content": "text", "text": "Hello"})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        app.run("object.textFrameOptions", json!({"ids": [id.0], "columns": 2, "gutter": 18, "inset": [9, 9, 9, 9]})).unwrap();
        app.run("selection.set", json!({"ids": [id.0]})).unwrap();
        app.ui.dialog = Some(Dialog::new("textFrameOptions", json!({})));
        let mut h = kit::harness_sized(app, vec2(1000.0, 800.0));
        let sides: &[&str] = if shape == "ellipse" { &["Inset"] } else { &["Top", "Left", "Bottom", "Right"] };
        for side in sides {
            assert_eq!(value_of(&h, &format!("{side}:")), mm(9.0), "{shape}: {side}");
        }
        assert_eq!(value_of(&h, "Gutter:"), mm(18.0), "{shape}: the gutter");
        assert_eq!(value_of(&h, "Paragraph Spacing Limit"), mm(0.0), "{shape}: the paragraph spacing limit");
        let width = value_of(&h, "Width:");
        assert!(width.ends_with(" mm"), "{shape}: the column width {width:?} in millimetres");
        // Bare numbers typed in the fields are millimetres.
        assert_eq!(kit::draft(&h).unit, Unit::Millimeters, "{shape}");
        let inset = if shape == "ellipse" { "Inset:" } else { "Top:" };
        kit::type_and_commit(&mut h, inset, 0, "5");
        kit::type_and_commit(&mut h, "Gutter:", 0, "4");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none(), "{shape}");
        let o = h.state().session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone();
        let pt = |n: f64| Unit::Millimeters.to_pt(n);
        assert!((o.inset[0] - pt(5.0)).abs() < 1e-6, "{shape}: the inset {:?}", o.inset);
        assert!((o.gutter - pt(4.0)).abs() < 1e-6, "{shape}: the gutter {}", o.gutter);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// CJK style sections and Layer Options

/// Successor of `ruby_alignment_and_shatai_angle_reach_the_style_and_its_paragraph`: Ruby Alignment, the Shatai values and the
/// Grid Alignment settings of Paragraph Style Options reach the style and the paragraph that follows it.
#[test]
fn paragraph_style_options_form_ruby_alignment_and_shatai_angle_reach_the_style_and_its_paragraph() {
    use designcraft_doc::ruby::RubyAlignment;
    let mut app = kit::app(1, false);
    app.run("style.paragraph.create", json!({"name": "Body"})).unwrap();
    let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "漢字かな"})).unwrap();
    app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 2})).unwrap();
    app.run("style.paragraph.apply", json!({"name": "Body"})).unwrap();
    app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Body", "section": "cjk.rubyPlacement"})));
    let mut h = kit::harness_sized(app, vec2(1400.0, 900.0));
    {
        let d = h.state_mut().ui.dialog.as_mut().unwrap();
        d.fields.insert("c.rubyAlignment".into(), json!("equalAki"));
        d.fields.insert("section".into(), json!("cjk.shatai"));
        d.fields.insert("c.shataiMagnification".into(), json!(20.0));
        d.fields.insert("c.shataiAngle".into(), json!(60.0));
        d.fields.insert("section".into(), json!("cjk.grid"));
        d.fields.insert("p.gridAlign".into(), json!("allLines"));
        d.fields.insert("p.gridReference".into(), json!("emCenter"));
        d.fields.insert("p.gridGyoudori".into(), json!(2));
    }
    kit::steps(&mut h, 3);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let doc = h.state().session.active().unwrap().doc.clone();
    let st = doc.styles.para("Body").unwrap();
    assert_eq!(st.chars.ruby_alignment, Some(RubyAlignment::EqualAki));
    assert_eq!((st.chars.shatai_magnification, st.chars.shatai_angle), (Some(20.0), Some(60.0)));
    assert_eq!((st.para.grid_reference, st.para.grid_gyoudori), (Some(designcraft_doc::cjk::CharacterAlignment::EmCenter), Some(2)));
    let a = h.state_mut().run("type.selectionAttrs", json!({})).unwrap();
    assert_eq!(a["chars"]["rubyAlignment"], "equalAki", "the paragraph follows its style");
    assert_eq!(a["chars"]["shataiAngle"], 60.0);
    assert_eq!(a["para"]["gridGyoudori"], 2);
}

/// Successor of `layer_options_lock_the_layers_guides` (with the double click that opens it, which the Layers panel's own
/// tests cover): Lock Guides sits beside Lock Layer in the framework Layer Options and OK locks the guides only.
#[test]
fn layer_options_form_locks_the_layers_guides() {
    let mut app = kit::app(1, false);
    app.run("layer.new", json!({"name": "Grid"})).unwrap();
    let id = app.session.active().unwrap().doc.layers.iter().find(|l| l.name == "Grid").unwrap().id.0;
    crate::dialogs::open_layer_options(&mut app, Some(id)).unwrap();
    let mut h = kit::harness(app);
    let (lock, guides) = (kit::rect_of(&h, "Lock Layer").unwrap(), kit::rect_of(&h, "Lock Guides").unwrap());
    assert!((lock.center().y - guides.center().y).abs() < 2.0 && guides.left() > lock.right(), "Lock Guides is beside Lock Layer");
    kit::click(&mut h, &[Role::CheckBox], "Lock Guides", 0);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let doc = &h.state().session.active().unwrap().doc;
    let l = doc.layers.iter().find(|l| l.name == "Grid").unwrap();
    assert!(l.lock_guides && !l.locked);
}
