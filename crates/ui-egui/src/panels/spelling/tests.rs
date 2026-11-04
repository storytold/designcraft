use egui_kittest::{Harness, kittest::Queryable};

use super::*;
use crate::menus;

const TEXT: &str = "The jumpd fox. A jumpd dog.";

fn app_with(text: &str) -> (DesignApp, designcraft_doc::StoryId) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".into();
    app.run("file.new", json!({})).unwrap();
    let frame = app.run("frame.create", json!({"rect": [36, 36, 400, 200], "content": "text", "text": text})).unwrap();
    (app, designcraft_doc::StoryId(frame["story"].as_u64().unwrap()))
}

fn panel(app: DesignApp) -> Harness<'static, DesignApp> {
    let mut h = Harness::builder().with_size(egui::vec2(320.0, 800.0)).build_ui_state(|ui, app: &mut DesignApp| show(app, ui), app);
    h.run_steps(2);
    h
}

fn text_of(app: &DesignApp, sid: designcraft_doc::StoryId) -> String {
    app.session.doc().unwrap().doc.story(sid).unwrap().text.clone()
}

#[test]
fn check_spelling_opens_the_panel_on_the_first_issue_and_selects_it() {
    let (mut app, sid) = app_with(TEXT);
    app.run("spelling.check", json!({"suggestions": false})).unwrap();
    assert!(!app.spelling.started(), "the query alone opens nothing");
    menus::activate(&mut app, "spelling.check", &Value::Null);
    assert_eq!(app.spelling.current().unwrap().word, "jumpd");
    assert!(app.ui.dialog.is_none(), "the review does not block the document");
    let selection = app.session.active().unwrap().selection.text.unwrap();
    assert_eq!((selection.story, selection.anchor, selection.focus), (sid, 4, 9), "the word is selected in the text");
}

#[test]
fn a_suggestion_chosen_in_the_panel_corrects_and_undo_restores() {
    let (mut app, sid) = app_with(TEXT);
    open(&mut app).unwrap();
    let suggestion = app.spelling.current().unwrap().suggestions[1].clone();
    let mut h = panel(app);
    h.get_by_label(&suggestion).click();
    h.run_steps(2);
    assert_eq!(text_of(h.state(), sid), TEXT, "a click only chooses");
    h.get_by_label("Change").click();
    h.run_steps(2);
    let app = h.state_mut();
    assert_eq!(text_of(app, sid), format!("The {suggestion} fox. A jumpd dog."));
    assert_eq!(app.spelling.current().unwrap().start, 17 + suggestion.len() - 5, "on to the next");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(text_of(app, sid), TEXT);
}

/// A review marks what is left of its pass on the canvas, whatever Dynamic Spelling is set to.
#[test]
fn the_review_underlines_what_is_left_while_its_panel_shows() {
    let (mut app, sid) = app_with(TEXT);
    assert!(review_marks(&app).is_empty(), "nothing before a review");
    open(&mut app).unwrap();
    assert!(!app.ui.dynamic_spelling);
    let starts = |app: &DesignApp| review_marks(app).get(&sid.0).map(|marks| marks.iter().map(|m| m.0.start).collect::<Vec<_>>());
    assert_eq!(starts(&app), Some(vec![4, 17]));
    action(&mut app, "ignoreOnce", None).unwrap();
    assert_eq!(starts(&app), Some(vec![17]), "an ignored occurrence is no longer marked");
    app.run("window.panel", json!({"panel": "pages"})).unwrap();
    assert!(review_marks(&app).is_empty(), "the panel behind another one marks nothing");
}

/// The dock scrolls most panels itself, which would take the actions out of view with the rest.
#[test]
fn the_docked_panel_keeps_its_actions_in_view() {
    let (mut app, _) = app_with(TEXT);
    open(&mut app).unwrap();
    let height = 460.0;
    let h = crate::test_window::open(app, egui::vec2(1400.0, height));
    for action in ["Change", "Ignore Once", "Ignore All", "Add to Dictionary"] {
        let bottom = h.get_by_label(action).rect().max.y;
        assert!(bottom <= height, "{action} ends at {bottom}");
    }
}

#[test]
fn the_user_dictionary_and_an_edit_reach_the_review() {
    let (mut app, sid) = app_with(TEXT);
    open(&mut app).unwrap();
    action(&mut app, "add", None).unwrap();
    assert!(app.spelling.current().is_none(), "a word in the document's dictionary is no issue");
    assert!(app.run("spelling.words", json!({})).unwrap().as_array().unwrap().contains(&json!("jumpd")));
    let (mut app, _) = app_with(TEXT);
    open(&mut app).unwrap();
    let first = app.spelling.current().unwrap().suggestions[0].clone();
    let sid2 = app.spelling.current().unwrap().story;
    app.run("spelling.change", json!({"story": sid2, "start": 0, "end": 0, "to": "A "})).unwrap();
    let before = app.session.doc().unwrap().doc.story(designcraft_doc::StoryId(sid2)).unwrap().text.clone();
    assert!(action(&mut app, "change", Some(&first)).unwrap_err().contains("changed"));
    assert_eq!(app.session.doc().unwrap().doc.story(designcraft_doc::StoryId(sid2)).unwrap().text, before);
    let _ = sid;
    action(&mut app, "check", None).unwrap();
    assert_eq!(app.spelling.current().unwrap().start, 6);
    // Selecting elsewhere is not an edit: the review carries on.
    app.run("text.select", json!({"story": sid2, "anchor": 0, "focus": 0})).unwrap();
    action(&mut app, "ignoreOnce", None).unwrap();
}

#[test]
fn another_document_ends_the_review() {
    let (mut app, _) = app_with(TEXT);
    open(&mut app).unwrap();
    app.run("file.new", json!({})).unwrap();
    assert!(action(&mut app, "ignoreOnce", None).unwrap_err().contains("document changed"));
    assert!(action(&mut app, "nonsense", None).is_err());
}

fn context_harness(app: DesignApp) -> Harness<'static, DesignApp> {
    let mut h = Harness::builder().with_size(egui::vec2(320.0, 500.0)).build_ui_state(|ui, app: &mut DesignApp| context_items(app, ui), app);
    h.run_steps(1);
    h
}

#[test]
fn a_right_click_on_a_misspelled_word_offers_its_suggestions() {
    let (mut app, sid) = app_with(TEXT);
    app.run("text.select", json!({"story": sid.0, "anchor": 6, "focus": 6})).unwrap();
    prepare_context(&mut app);
    let suggestion = app.spelling_context.as_ref().expect("the caret is in a misspelled word").suggestions[0].clone();
    let mut h = context_harness(app);
    h.get_by_label(&suggestion).click();
    h.run_steps(2);
    assert_eq!(text_of(h.state(), sid), format!("The {suggestion} fox. A jumpd dog."));
    assert!(h.state().spelling_context.is_none(), "the section is gone with the word");
}

#[test]
fn a_right_click_adds_to_the_dictionary_or_opens_the_panel() {
    let (mut app, sid) = app_with(TEXT);
    app.run("text.select", json!({"story": sid.0, "anchor": 6, "focus": 6})).unwrap();
    prepare_context(&mut app);
    let mut h = context_harness(app);
    h.get_by_label("Check Spelling…").click();
    h.run_steps(2);
    assert_eq!(h.state().spelling.current().unwrap().word, "jumpd");
    let app = h.state_mut();
    app.run("text.select", json!({"story": sid.0, "anchor": 6, "focus": 6})).unwrap();
    prepare_context(app);
    h.run_steps(2);
    h.get_by_label("Add to Dictionary").click();
    h.run_steps(2);
    assert!(h.state_mut().run("spelling.words", json!({})).unwrap().as_array().unwrap().contains(&json!("jumpd")));
}

/// A cell's offsets are into its own story: at those offsets in the frame's story is some other word.
#[test]
fn a_right_click_in_a_table_cell_offers_nothing_for_the_text_around_it() {
    let (mut app, sid) = app_with(TEXT);
    app.run("text.select", json!({"story": sid.0, "anchor": 6, "focus": 6})).unwrap();
    app.run("table.insert", json!({"rows": 1, "cols": 1})).unwrap();
    let table = *app.session.doc().unwrap().doc.story(sid).unwrap().tables.keys().next().unwrap();
    let cell = designcraft_doc::CellAddr { table, row: 0, col: 0 };
    let selection = designcraft_doc::TextSel { story: sid, anchor: 6, focus: 6, frame: None, cell: Some(cell) };
    app.session.doc_mut().unwrap().selection.text = Some(selection);
    prepare_context(&mut app);
    assert!(app.spelling_context.is_none());
}

#[test]
fn a_right_click_in_a_longer_selection_isnt_about_the_word_at_its_end() {
    let (mut app, sid) = app_with(TEXT);
    app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 6})).unwrap();
    prepare_context(&mut app);
    assert!(app.spelling_context.is_none());
    app.run("text.select", json!({"story": sid.0, "anchor": 4, "focus": 9})).unwrap();
    prepare_context(&mut app);
    assert_eq!(app.spelling_context.as_ref().map(|i| i.word.as_str()), Some("jumpd"), "exactly the word, as the review selects it");
}

/// In the icon column `window.panel` closes an open flyout: the review mustn't go on out of sight.
#[test]
fn checking_again_keeps_the_panel_open() {
    let (mut app, _) = app_with(TEXT);
    open(&mut app).unwrap();
    assert!(crate::panel_docking::showing(&app, "spelling"));
    open(&mut app).unwrap();
    assert!(crate::panel_docking::showing(&app, "spelling"));
    assert!(!review_marks(&app).is_empty());
}

#[test]
fn the_panel_says_what_it_doesnt_check() {
    let (mut app, sid) = app_with(TEXT);
    assert!(AppHost(&mut app).notice().is_none_or(|n| !n.contains(NOTES_UNCHECKED)));
    app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
    app.run("table.insert", json!({"rows": 1, "cols": 1})).unwrap();
    assert!(AppHost(&mut app).notice().is_some_and(|n| n.contains(NOTES_UNCHECKED)));
}

#[test]
fn a_right_click_elsewhere_adds_nothing_to_the_menu() {
    let (mut app, sid) = app_with(TEXT);
    app.run("text.select", json!({"story": sid.0, "anchor": 1, "focus": 1})).unwrap();
    prepare_context(&mut app);
    assert!(app.spelling_context.is_none(), "a correct word");
    let h = context_harness(app);
    assert!(h.query_by_label("Add to Dictionary").is_none());
}
