//! Successors of the native dialog tests of `dialogs/tests.rs`, written against the framework path: each runs the real
//! dialog host over an installed framework (`table_layout_kit`), drives the form with pointer and keyboard events as a
//! user does, and checks the same outcome the native test did (document, commands, undo steps, draft fields). Which old
//! test each one replaces is listed in `work/ui-port-design/TEST-SUCCESSORS-wave4.md`.

use super::table_layout_kit as kit;
use super::*;
use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use serde_json::{Value, json};

/// The items of the first spread.
fn item_count(h: &egui_kittest::Harness<'_, DesignApp>) -> usize {
    h.state().session.doc().unwrap().doc.spreads[0].items.len()
}

/// The highest top edge (smallest y) of the first spread's items.
fn lowest_top(h: &egui_kittest::Harness<'_, DesignApp>) -> f64 {
    h.state().session.doc().unwrap().doc.spreads[0].items.iter().map(|item| item.bounds().min_y()).fold(f64::MAX, f64::min)
}

/// The corners of item `id`.
fn corners(h: &egui_kittest::Harness<'_, DesignApp>, id: u64) -> [designcraft_geom::corners::Corner; 4] {
    h.state().session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().corners.corners
}

// ---------------------------------------------------------------------------------------------------------------------
// Step and Repeat

/// A document with one selected frame near the bottom of the page.
fn step_app() -> (DesignApp, u64) {
    let mut app = kit::app(1, false);
    let id = app.run("frame.create", json!({"rect": [72, 600, 540, 640]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    (app, id)
}

/// Successor of `step_and_repeat_dialog_previews_and_makes_the_copies_as_one_undo_step`: the framework form previews the
/// copies without an undo step, follows its fields, Preview off and Cancel take them away, OK after a preview makes them
/// once as one undo step, a grid makes rows times columns, and a value that is no measure keeps the dialog open.
#[test]
fn step_and_repeat_form_previews_and_makes_the_copies_as_one_undo_step() {
    let (mut app, id) = step_app();
    kit::open(&mut app, "edit.stepAndRepeat");
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(STEP_AND_REPEAT));
    let mut h = kit::harness(app);
    for shown in ["Repeat", "Count:", "Offset", "Vertical:", "Horizontal:"] {
        assert!(kit::shows(&h, shown), "{shown} is shown");
    }
    let (n0, u0) = (item_count(&h), kit::undo_steps(&h));
    // Preview: the copies show, and change with the fields, without an undo step.
    kit::type_and_commit(&mut h, "Count:", 0, "5");
    kit::type_and_commit(&mut h, "Vertical:", 0, "-0.75 in");
    kit::type_and_commit(&mut h, "Horizontal:", 0, "0");
    assert_eq!(item_count(&h), n0, "nothing shows before Preview is on");
    kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
    assert_eq!((item_count(&h), kit::undo_steps(&h)), (n0 + 5, u0));
    kit::type_and_commit(&mut h, "Count:", 0, "3");
    assert_eq!(item_count(&h), n0 + 3, "the preview follows the field");
    kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
    assert_eq!(item_count(&h), n0, "Preview off takes them away");
    kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
    assert_eq!(item_count(&h), n0 + 3);
    // Cancel puts the document back.
    kit::click(&mut h, &[Role::Button], "Cancel", 0);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!((item_count(&h), kit::undo_steps(&h)), (n0, u0));
    // OK after a preview makes the copies once, as one undo step, 0.75 in apart.
    h.state_mut().run("selection.set", json!({"ids": [id]})).unwrap();
    kit::reopen(&mut h, "edit.stepAndRepeat");
    kit::type_and_commit(&mut h, "Count:", 0, "2");
    kit::type_and_commit(&mut h, "Vertical:", 0, "-0.75 in");
    kit::type_and_commit(&mut h, "Horizontal:", 0, "0");
    kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!((item_count(&h), kit::undo_steps(&h)), (n0 + 2, u0 + 1));
    let top = lowest_top(&h);
    assert!((top - (600.0 - 108.0)).abs() < 1e-6, "{top}");
    let sent = kit::journal(&h, "edit.stepAndRepeat");
    assert_eq!(sent.last().map(|(_, params)| params["count"].clone()), Some(json!(2)), "the command the native form sent: {sent:?}");
    // Create as a grid: Rows x Columns cells, the original being one.
    h.state_mut().run("selection.set", json!({"ids": [id]})).unwrap();
    kit::reopen(&mut h, "edit.stepAndRepeat");
    let before_grid = item_count(&h);
    kit::click(&mut h, &[Role::CheckBox], "Create as a grid", 0);
    assert!(kit::shows(&h, "Rows:") && kit::shows(&h, "Columns:"), "a grid asks for rows and columns");
    kit::type_and_commit(&mut h, "Rows:", 0, "2");
    kit::type_and_commit(&mut h, "Columns:", 0, "3");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!(item_count(&h), before_grid + 5, "2 x 3 cells make five copies");
    // A value that isn't a measure keeps the dialog open and says so.
    let (items, undo) = (item_count(&h), kit::undo_steps(&h));
    kit::reopen(&mut h, "edit.stepAndRepeat");
    kit::type_and_commit(&mut h, "Vertical:", 0, "up");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_some(), "the dialog stays open");
    assert_eq!(kit::draft(&h).s("status"), "Invalid value: up");
    assert_eq!((item_count(&h), kit::undo_steps(&h)), (items, undo), "nothing was made");
}

// ---------------------------------------------------------------------------------------------------------------------
// Corner Options

/// Successor of `corner_options_dialog_sets_each_corner_with_preview_and_one_undo_step`: seeded from the frame and linked,
/// a linked corner is all four, unlinked only the top-right one rounds, Preview shows it without an undo step and Preview
/// off puts it back, OK is one undo step, reopening shows the frame's corners, and a draft the form cannot take keeps
/// the dialog open and Cancel leaves the corners alone.
#[test]
fn corner_options_form_sets_each_corner_with_preview_and_one_undo_step() {
    use designcraft_geom::corners::CornerShape;
    let mut app = kit::app(1, false);
    let id = app.run("frame.create", json!({"rect": [72, 72, 360, 360]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    kit::open(&mut app, "object.cornerOptions");
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(CORNER_OPTIONS), "the command opens the corner dialog, not a form of its params");
    let mut h = kit::harness(app);
    assert!(kit::shows(&h, "Corner Size and Shape:"));
    assert!(!kit::shows(&h, "Ids"), "no raw command fields");
    let d = kit::draft(&h);
    assert_eq!((d.s("size1"), d.s("shape1"), d.b("link")), ("0p0".into(), "none".into(), true), "seeded from the frame, linked");
    let u0 = kit::undo_steps(&h);
    let rounded = crate::i18n::tr("en", CornerShape::Rounded.label()).to_string();
    // Linked: a corner changed is all four.
    kit::type_and_commit(&mut h, "Top Left:", 0, "1p0");
    assert_eq!(kit::draft(&h).s("size2"), "1p0", "the linked corners follow");
    // Unlinked, only the top-right corner rounds, as in the tutorial.
    kit::click(&mut h, kit::TOGGLE, "Make all settings the same", 0);
    assert!(!kit::draft(&h).b("link"));
    for corner in ["Top Left:", "Bottom Right:", "Bottom Left:"] {
        kit::type_and_commit(&mut h, corner, 0, "0");
    }
    kit::type_and_commit(&mut h, "Top Right:", 0, "3 cm");
    kit::pick(&mut h, "Top Right: Shape", &rounded);
    assert_eq!(kit::draft(&h).s("shape1"), "rounded");
    assert_eq!(corners(&h, id)[1].shape, CornerShape::None, "nothing shows before Preview is on");
    kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
    assert_eq!(corners(&h, id)[1].shape, CornerShape::Rounded, "Preview shows it");
    assert_eq!(kit::undo_steps(&h), u0, "without an undo step");
    kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
    assert_eq!(corners(&h, id)[1].shape, CornerShape::None, "Preview off puts it back");
    kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let c = corners(&h, id);
    assert_eq!(c.map(|c| c.shape), [CornerShape::None, CornerShape::Rounded, CornerShape::None, CornerShape::None]);
    assert!((c[1].size - 3.0 / 2.54 * 72.0).abs() < 1e-6, "{}", c[1].size);
    assert_eq!(kit::undo_steps(&h), u0 + 1, "one undo step");
    // Reopened, it shows the frame's corners, no longer linked.
    kit::reopen(&mut h, "object.cornerOptions");
    let d = kit::draft(&h);
    assert_eq!((d.s("shape1"), d.s("shape0"), d.b("link")), ("rounded".into(), "none".into(), false));
    // A draft the form cannot take keeps the dialog open with the error; so does a shape it doesn't offer.
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("size0".into(), json!("abc"));
    kit::steps(&mut h, 2);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_some(), "a size that does not parse keeps the dialog open");
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("size0".into(), json!("0"));
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("shape0".into(), json!("spiky"));
    kit::steps(&mut h, 2);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_some(), "a shape the dialog doesn't offer keeps it open");
    kit::click(&mut h, &[Role::Button], "Cancel", 0);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!(corners(&h, id), c);
    assert_eq!(kit::undo_steps(&h), u0 + 1);
}

/// Successor of `corner_options_keep_each_objects_untouched_corners`: with two frames selected, an unlinked change sets
/// only that corner on both and each keeps its others; a linked change sets all four on both.
#[test]
fn corner_options_form_keeps_each_objects_untouched_corners() {
    use designcraft_geom::corners::CornerShape;
    let mut app = kit::app(1, false);
    let a = app.run("frame.create", json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let b = app.run("frame.create", json!({"rect": [272, 72, 400, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("object.cornerOptions", json!({"ids": [b], "shape": "bevel", "size": 12})).unwrap();
    app.run("selection.set", json!({"ids": [a, b]})).unwrap();
    let mut h = kit::harness({
        kit::open(&mut app, "object.cornerOptions");
        app
    });
    let rounded = crate::i18n::tr("en", CornerShape::Rounded.label()).to_string();
    let b0 = corners(&h, b);
    // Unlinked: only the top-right corner rounds, on both.
    kit::click(&mut h, kit::TOGGLE, "Make all settings the same", 0);
    assert!(!kit::draft(&h).b("link"));
    kit::type_and_commit(&mut h, "Top Right:", 0, "1p0");
    kit::pick(&mut h, "Top Right: Shape", &rounded);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let (ca, cb) = (corners(&h, a), corners(&h, b));
    assert_eq!(ca.map(|c| c.shape), [CornerShape::None, CornerShape::Rounded, CornerShape::None, CornerShape::None]);
    assert_eq!(cb.map(|c| c.shape), [CornerShape::Bevel, CornerShape::Rounded, CornerShape::Bevel, CornerShape::Bevel], "b keeps its others");
    assert_eq!((cb[0], cb[2], cb[3]), (b0[0], b0[2], b0[3]));
    assert_eq!(ca[1], cb[1]);
    // Linked: a corner set to the first object's own value still sets all four, on both.
    kit::reopen(&mut h, "object.cornerOptions");
    if !kit::draft(&h).b("link") {
        kit::click(&mut h, kit::TOGGLE, "Make all settings the same", 0);
    }
    assert!(kit::draft(&h).b("link"));
    kit::type_and_commit(&mut h, "Top Left:", 0, "0p6");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let (ca, cb) = (corners(&h, a), corners(&h, b));
    assert_eq!(ca, cb, "all four, on both");
    assert!(ca.iter().all(|c| c.shape == CornerShape::None && (c.size - 6.0).abs() < 1e-6), "{ca:?}");
}

// ---------------------------------------------------------------------------------------------------------------------
// Table of Contents

/// The sample document, whose paragraph styles include Headline, in English.
fn sample_app() -> DesignApp {
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".to_owned();
    app.run("file.newSample", json!({})).unwrap();
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
    app
}

/// Successor of `toc_dialog_adds_a_style_and_ok_makes_the_table_of_contents`: Add moves the picked style into the
/// included list, OK makes the table of contents once as one undo step, and the reopened dialog shows it and replaces it.
#[test]
fn toc_form_adds_a_style_and_ok_makes_the_table_of_contents() {
    let mut app = sample_app();
    kit::open(&mut app, "toc.generate");
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(TOC));
    let mut h = kit::harness(app);
    for shown in ["Title:", "Include Paragraph Styles:", "Other Styles:", "« Add", "Remove »", "Replace Existing Table of Contents"] {
        assert!(kit::shows(&h, shown), "{shown} is shown");
    }
    kit::click_first(&mut h, "Headline");
    kit::click(&mut h, &[Role::Button], "« Add", 0);
    assert_eq!(super::toc_rows(kit::draft(&h)).iter().map(|row| row.style.as_str()).collect::<Vec<_>>(), ["Headline"], "Add includes the style");
    for shown in ["Entry Style:", "Between Entry and Number:", "Level:"] {
        assert!(kit::shows(&h, shown), "{shown} is shown for the included style");
    }
    let undo = kit::undo_steps(&h);
    assert!(h.state().session.doc().unwrap().doc.toc.is_none(), "nothing is made before OK");
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let d = &h.state().session.doc().unwrap().doc;
    let toc = d.toc.clone().unwrap();
    assert_eq!(toc.entries, vec![designcraft_doc::TocEntry::new("Headline", 1)]);
    let text = &d.story(toc.story).unwrap().text;
    assert!(text.starts_with("Contents\n") && text.contains("\nNotes on the Grid\t"), "{text}");
    assert_eq!(kit::undo_steps(&h), undo + 1);
    // Reopened, the dialog shows the table of contents and replaces it.
    kit::reopen(&mut h, "toc.generate");
    let d = kit::draft(&h);
    assert!(d.b("replace"));
    assert_eq!(super::toc_rows(d).iter().map(|row| row.style.as_str()).collect::<Vec<_>>(), ["Headline"]);
}

/// Successor of `toc_more_options_preserves_hidden_settings`: Fewer Options hides the entry's page number, separator and
/// sort settings, More Options shows them again, and neither the hidden settings nor the document change meanwhile.
#[test]
fn toc_form_more_options_preserves_hidden_settings() {
    let mut app = sample_app();
    kit::open(&mut app, "toc.generate");
    let mut h = kit::harness(app);
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert(
        "included".into(),
        json!([{
            "style": "Headline", "level": 3, "separator": " ^t", "pageNumber": "before",
            "pageNumberStyle": "Accent", "separatorStyle": "Emphasis", "sort": true
        }]),
    );
    kit::steps(&mut h, 3);
    let before = super::toc_params(h.state(), kit::draft(&h));
    let revision = h.state().session.doc().unwrap().revision;
    assert!(kit::shows(&h, "Between Entry and Number:"));
    kit::click(&mut h, &[Role::Button], "Fewer Options", 0);
    assert!(!kit::shows(&h, "Between Entry and Number:"), "the separator is hidden");
    assert!(kit::shows(&h, "Entry Style:"), "the entry style stays");
    assert_eq!(super::toc_params(h.state(), kit::draft(&h)), before, "hiding changes no setting");
    kit::click(&mut h, &[Role::Button], "More Options", 0);
    assert!(kit::shows(&h, "Between Entry and Number:"));
    assert_eq!(super::toc_params(h.state(), kit::draft(&h)), before);
    assert_eq!(h.state().session.doc().unwrap().revision, revision);
}

/// Successor of `enter_on_the_toc_options_toggle_only_toggles`: Enter on the focused options toggle toggles it and neither
/// closes the dialog nor makes a table of contents.
#[test]
fn toc_form_enter_on_the_options_toggle_only_toggles() {
    let mut app = sample_app();
    kit::open(&mut app, "toc.generate");
    let mut h = kit::harness(app);
    let revision = h.state().session.doc().unwrap().revision;
    h.query_all_by_label("Fewer Options").next().expect("the toggle is shown").focus();
    kit::steps(&mut h, 2);
    kit::key(&mut h, Key::Enter, Modifiers::NONE);
    let d = h.state().ui.dialog.as_ref().expect("the dialog stays open");
    assert_eq!((d.id.as_str(), d.b("moreOptions")), (TOC, false), "the options are fewer");
    assert_eq!(h.state().session.doc().unwrap().revision, revision, "no table of contents made");
    assert!(kit::shows(&h, "More Options"));
}

/// Successor of `toc_title_field_shows_the_whole_default_title`: the Title field holds the whole default title and follows
/// the draft, and in a small window it narrows so the style selector beside it stays inside the dialog.
#[test]
fn toc_form_title_field_shows_the_whole_default_title() {
    let mut app = sample_app();
    kit::open(&mut app, "toc.generate");
    let mut h = kit::harness(app);
    let title = |h: &egui_kittest::Harness<'_, DesignApp>| h.get_by_role_and_label(Role::TextInput, "Title:").value();
    assert_eq!(title(&h).as_deref(), Some("Contents"));
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("title".into(), json!("Table of Contents"));
    kit::steps(&mut h, 3);
    assert_eq!(title(&h).as_deref(), Some("Table of Contents"));
    // A small window: the title narrows and the style selector beside it stays in view.
    h.set_size(egui::vec2(450.0, 280.0));
    kit::steps(&mut h, 4);
    let style = h.query_all_by_label("Style:").next().expect("the style selector is shown").rect();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(450.0, 280.0));
    assert!(screen.contains_rect(style), "the style selector stays in the window: {style:?} in {screen:?}");
}

/// Successor of `toc_bookmarks_checkbox_sets_and_reopens_the_persisted_option`: Create PDF Bookmarks starts on, OK keeps the
/// bookmarks and the option, unticking it and OK removes the bookmarks, and the dialog reopens with the option as left.
#[test]
fn toc_form_bookmarks_checkbox_sets_and_reopens_the_persisted_option() {
    let mut app = sample_app();
    kit::open(&mut app, "toc.generate");
    assert!(app.ui.dialog.as_ref().unwrap().b("createBookmarks"));
    let mut h = kit::harness(app);
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("included".into(), json!([{"style": "Headline", "level": 1}]));
    kit::steps(&mut h, 3);
    assert!(kit::shows(&h, "Create PDF Bookmarks"));
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    assert!(h.state().session.doc().unwrap().doc.toc.as_ref().unwrap().create_bookmarks);
    assert!(!h.state().session.doc().unwrap().doc.bookmarks.is_empty());
    kit::reopen(&mut h, "toc.generate");
    kit::click(&mut h, &[Role::CheckBox], "Create PDF Bookmarks", 0);
    assert!(!kit::draft(&h).b("createBookmarks"));
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().session.doc().unwrap().doc.bookmarks.is_empty());
    kit::reopen(&mut h, "toc.generate");
    assert!(!kit::draft(&h).b("createBookmarks"));
}

// ---------------------------------------------------------------------------------------------------------------------
// Cell Options and Diagonal Lines

/// A document with a two by two table in a text frame, the whole table selected, and the table's id.
fn table_app() -> (DesignApp, u64) {
    let mut app = kit::app(1, false);
    app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
    let table = app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    (app, table)
}

fn select_table(app: &mut DesignApp) {
    app.run("table.select", json!({"what": "table"})).unwrap();
}

/// The table `id` of the active document.
fn table_of(h: &egui_kittest::Harness<'_, DesignApp>, id: u64) -> designcraft_doc::Table {
    h.state().session.active().unwrap().doc.stories.values().find_map(|story| story.tables.get(&id)).map(|table| (**table).clone()).unwrap()
}

/// Successor of `cell_options_dialog_draws_and_applies_only_changed_values_to_the_chosen_edges`: the framework form shows the
/// cells' weight, a gap colour and fill picked in it apply to the chosen edges only, the inner edges keep theirs, and a
/// value left alone (the weight) is not applied. The edge choice is set in the draft (as the native test did): the edge
/// proxy has its own tests in the Cell Stroke panel.
#[test]
fn cell_options_form_applies_only_changed_values_to_the_chosen_edges() {
    let (mut app, table) = table_app();
    select_table(&mut app);
    app.run("app.cellOptionsDialog", json!({})).unwrap();
    let mut h = kit::harness(app);
    let d = kit::draft(&h);
    assert_eq!(d.id, "cellOptions");
    assert_eq!(d.fields["weight"], json!(1.0));
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("edges".into(), json!(["top", "bottom", "left", "right"]));
    kit::steps(&mut h, 3);
    // Colour pickers in order: the stroke Color, the Gap Color, then the Cell Fill Color; and the tints likewise.
    kit::pick_named_like(&mut h, "Gap Color", 0, "[Paper]");
    kit::pick_named_like(&mut h, "Color", 2, "[Black]");
    kit::type_and_commit_named_like(&mut h, "Tint", 2, "20");
    let d = kit::draft(&h);
    assert_eq!((d.s("gapColor"), d.s("fill")), ("[Paper]".into(), "[Black]".into()));
    assert!(d.n("fillTint").is_some_and(|tint| (tint - 0.2).abs() < 1e-6), "{:?}", d.n("fillTint"));
    let undo = kit::undo_steps(&h);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let t = table_of(&h, table);
    let gap = |row: usize, col: usize, side: usize| t.cell(row, col).unwrap().strokes[side].gap_color.clone();
    assert_eq!([gap(0, 0, 0), gap(0, 0, 1), gap(1, 1, 2), gap(1, 1, 3)], ["[Paper]"; 4].map(String::from), "outer edges");
    assert_eq!([gap(0, 0, 2), gap(0, 0, 3)], ["[None]"; 2].map(String::from), "inner edges keep theirs");
    assert!(t.cells.iter().all(|c| c.fill == "[Black]" && (c.fill_tint - 0.2).abs() < 1e-6));
    assert!(t.cells.iter().all(|c| c.strokes.iter().all(|e| e.weight == 1.0)), "unchanged values are not applied");
    assert_eq!(kit::undo_steps(&h), undo + 1, "one OK is one undo step");
}

/// Successor of `diagonal_lines_dialog_draws_and_applies_its_values_but_not_a_mixed_one`: the form shows its sections,
/// Crossed Diagonal Lines, Content In Front and a weight apply to every cell, and a colour the cells do not share is left
/// as it is.
#[test]
fn diagonal_lines_form_applies_its_values_but_not_a_mixed_one() {
    let (mut app, table) = table_app();
    app.run("table.setCell", json!({"row": 0, "col": 0, "diagonal": {"color": "[Paper]"}})).unwrap();
    select_table(&mut app);
    app.run("app.diagonalLinesDialog", json!({})).unwrap();
    let mut h = kit::harness(app);
    assert!(kit::shows(&h, "Line Stroke"), "Line Stroke is shown");
    // Diagonal In Front is the entry the Draw picker shows: its value, not a name.
    assert!(kit::picker_shows(&h, "Draw", "Diagonal In Front"), "Diagonal In Front is shown");
    let d = kit::draft(&h);
    assert_eq!(d.id, "diagonalLines");
    assert_eq!((&d.fields["lines"], &d.fields["color"], &d.fields["inFront"]), (&json!("none"), &Value::Null, &json!(true)));
    kit::click_first(&mut h, "Crossed Diagonal Lines");
    assert_eq!(kit::draft(&h).fields["lines"], json!("crossed"));
    kit::pick_named_like(&mut h, "Draw", 0, "Content In Front");
    assert_eq!(kit::draft(&h).fields["inFront"], json!(false));
    kit::type_and_commit_named_like(&mut h, "Weight", 0, "0.5");
    assert_eq!(kit::draft(&h).fields["weight"], json!(0.5));
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let t = table_of(&h, table);
    assert!(
        t.cells
            .iter()
            .all(|c| c.diagonal.lines == designcraft_doc::DiagonalLines::Crossed && !c.diagonal.in_front && c.diagonal.stroke.weight == 0.5)
    );
    let colors: Vec<_> = t.cells.iter().map(|c| c.diagonal.stroke.color.as_str()).collect();
    assert_eq!(colors, ["[Paper]", "[Black]", "[Black]", "[Black]"], "a mixed value is left as it is");
}

/// Successor of `enter_on_a_diagonal_line_button_picks_it_and_leaves_the_dialog_open`: Enter on a focused line choice picks
/// it and leaves the dialog open with nothing applied, and Enter with no line choice focused confirms.
#[test]
fn diagonal_lines_form_enter_on_a_line_button_picks_it_and_leaves_the_dialog_open() {
    let (mut app, table) = table_app();
    select_table(&mut app);
    app.run("app.diagonalLinesDialog", json!({})).unwrap();
    let mut h = kit::harness(app);
    h.query_all_by_label("Diagonal Line from Top Left to Bottom Right").next().expect("the line choice is shown").focus();
    kit::steps(&mut h, 2);
    kit::key(&mut h, Key::Enter, Modifiers::NONE);
    let d = h.state().ui.dialog.as_ref().expect("Enter on a line button leaves the dialog open");
    assert_eq!(d.fields["lines"], json!("topLeft"));
    assert!(table_of(&h, table).cells.iter().all(|c| c.diagonal.lines == designcraft_doc::DiagonalLines::None), "nothing is applied yet");
    // Enter with no line button focused still confirms.
    let ctx = h.ctx.clone();
    ctx.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    kit::steps(&mut h, 2);
    kit::key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().ui.dialog.is_none(), "Enter confirms");
    assert!(table_of(&h, table).cells.iter().all(|c| c.diagonal.lines == designcraft_doc::DiagonalLines::TopLeft));
}
