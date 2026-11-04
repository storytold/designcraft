use std::path::Path;

use egui::{Pos2, Rect, vec2};
use egui_kittest::Harness;
use serde_json::json;

use super::*;
use crate::canvas::link_badges::{self, Badge};

fn png(w: u32) -> Vec<u8> {
    designcraft_render::Rendered { width: w, height: 4, pixels: vec![200u8; (w * 16) as usize] }.to_png()
}

/// An app that checks links on the calling thread, with a new document.
fn app() -> DesignApp {
    let launch: crate::edit_original::LaunchFn = Box::new(|_, _| Ok(()));
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services { launch: Some(launch), ..Default::default() });
    app.link_watch.inline = true;
    app.run("file.new", json!({})).unwrap();
    app
}

/// Places `file` (linked) at 100,100, 200 pt wide, with nothing left selected → (frame, asset).
fn place(app: &mut DesignApp, file: &Path) -> (u64, u64) {
    let r = app.run("file.place", json!({"path": file.to_string_lossy(), "x": 100, "y": 100, "width": 200})).unwrap();
    app.run("selection.set", json!({"ids": []})).unwrap();
    (r["id"].as_u64().unwrap(), r["asset"].as_u64().unwrap())
}

fn state(app: &DesignApp, asset: u64) -> Option<LinkState> {
    app.link_watch.state(app.session.active().unwrap().uid, asset)
}

fn pixels(app: &DesignApp, asset: u64) -> Option<(u32, u32)> {
    app.session.doc().unwrap().doc.assets[&designcraft_doc::AssetId(asset)].pixels
}

fn undo_len(app: &DesignApp) -> usize {
    app.session.doc().unwrap().history.undo.len()
}

/// Coming back to the window checks every linked graphic; between checks nothing is looked at.
#[test]
fn focus_regained_checks_the_links_again_and_so_does_the_timer() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    let mut app = app();
    let (_, asset) = place(&mut app, &file);
    tick(&mut app, 0.0, Some(true));
    assert_eq!(state(&app, asset), Some(LinkState::Ok));
    // Another application saves a wider image while the window is away.
    std::fs::write(&file, png(9)).unwrap();
    tick(&mut app, 0.5, Some(true));
    assert_eq!(state(&app, asset), Some(LinkState::Ok), "not due yet");
    tick(&mut app, 0.6, Some(false));
    tick(&mut app, 0.7, Some(true));
    assert_eq!(state(&app, asset), Some(LinkState::Modified), "focus regained: checked");
    // The preference is off: shown, not updated.
    assert_eq!(pixels(&app, asset), Some((4, 4)));
    // Missing, found by the timer alone.
    std::fs::remove_file(&file).unwrap();
    tick(&mut app, 1.0, Some(true));
    assert_eq!(state(&app, asset), Some(LinkState::Modified), "not due yet");
    tick(&mut app, 0.7 + CHECK_EVERY, Some(true));
    assert_eq!(state(&app, asset), Some(LinkState::Missing));
    // Another document: what was found about this one isn't shown for it.
    let uid = app.session.active().unwrap().uid;
    app.run("file.new", json!({})).unwrap();
    tick(&mut app, 10.0, Some(true));
    assert_eq!(app.link_watch.state(uid, asset), None);
}

/// With Update Modified Links When DesignCraft Comes Back to the Front on, focus regained updates
/// every modified graphic in one undo step; off (the default), it only shows them.
#[test]
fn the_auto_update_preference_updates_modified_links_on_focus() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.png"), dir.path().join("b.png"));
    std::fs::write(&a, png(4)).unwrap();
    std::fs::write(&b, png(5)).unwrap();
    let mut app = app();
    assert!(!app.session.prefs.auto_update_links, "off by default");
    let (_, aa) = place(&mut app, &a);
    let (_, ab) = place(&mut app, &b);
    tick(&mut app, 0.0, Some(true));
    std::fs::write(&a, png(8)).unwrap();
    std::fs::write(&b, png(10)).unwrap();
    // Off: focus back, nothing updated.
    let undo = undo_len(&app);
    tick(&mut app, 0.1, Some(false));
    tick(&mut app, 0.2, Some(true));
    assert_eq!((pixels(&app, aa), pixels(&app, ab)), (Some((4, 4)), Some((5, 4))));
    assert_eq!(undo_len(&app), undo);
    // On: focus back updates both, in one step.
    app.run("prefs.set", json!({"autoUpdateLinks": true})).unwrap();
    tick(&mut app, 0.3, Some(false));
    tick(&mut app, 0.4, Some(true));
    assert_eq!((pixels(&app, aa), pixels(&app, ab)), (Some((8, 4)), Some((10, 4))));
    assert_eq!(undo_len(&app), undo + 1, "one undo step");
    assert_eq!((state(&app, aa), state(&app, ab)), (Some(LinkState::Ok), Some(LinkState::Ok)));
    // The timer alone never updates, even with the preference on.
    std::fs::write(&a, png(3)).unwrap();
    tick(&mut app, 0.4 + CHECK_EVERY, Some(true));
    assert_eq!(state(&app, aa), Some(LinkState::Modified));
    assert_eq!(pixels(&app, aa), Some((8, 4)));
    // Undo puts the old copy back; the next check shows it modified again.
    app.run("prefs.set", json!({"autoUpdateLinks": false})).unwrap();
    app.run("edit.undo", json!({})).unwrap();
    tick(&mut app, 0.4 + CHECK_EVERY + AFTER_EDIT, Some(true));
    assert_eq!(state(&app, ab), Some(LinkState::Modified));
}

/// A graphic opened with Edit Original is updated once a check sees its file changed while the
/// window is in front, even when the host never reported the window losing focus.
#[test]
fn an_edit_original_file_is_updated_without_a_focus_change() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    let mut app = app();
    let (frame, asset) = place(&mut app, &file);
    app.run("selection.set", json!({"ids": [frame]})).unwrap();
    app.run(crate::edit_original::EDIT_ORIGINAL, json!({})).unwrap();
    tick(&mut app, 0.0, Some(true));
    std::fs::write(&file, png(7)).unwrap();
    // The window stays focused as far as the host says: the timer finds it and updates it.
    tick(&mut app, CHECK_EVERY, Some(true));
    assert_eq!(pixels(&app, asset), Some((7, 4)));
    // While the window is away it waits.
    std::fs::write(&file, png(6)).unwrap();
    tick(&mut app, 2.0 * CHECK_EVERY, Some(false));
    assert_eq!(pixels(&app, asset), Some((7, 4)));
}

/// During a gesture nothing is updated (its next preview would put the old copy back) and the
/// Edit Original file isn't marked as seen: the update comes once the gesture ends.
#[test]
fn updates_wait_for_a_gesture_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    let mut app = app();
    let (frame, asset) = place(&mut app, &file);
    app.run("selection.set", json!({"ids": [frame]})).unwrap();
    app.run(crate::edit_original::EDIT_ORIGINAL, json!({})).unwrap();
    tick(&mut app, 0.0, Some(true));
    std::fs::write(&file, png(7)).unwrap();
    app.session.begin_interaction("Move").unwrap();
    assert_eq!(crate::edit_original::refresh_edited(&mut app), 0);
    tick(&mut app, CHECK_EVERY, Some(true));
    assert_eq!(pixels(&app, asset), Some((4, 4)), "not during the gesture");
    app.session.commit_interaction().unwrap();
    tick(&mut app, CHECK_EVERY + 0.1, Some(true));
    assert_eq!(pixels(&app, asset), Some((7, 4)), "after it");
}

/// A worker whose thread has ended doesn't stop the checks: the check runs here and the worker is
/// dropped, for the next frame to start another.
#[test]
fn a_dead_worker_is_replaced_and_the_check_runs_here() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    let mut app = app();
    let (_, asset) = place(&mut app, &file);
    let (tx, jobs) = std::sync::mpsc::channel();
    drop(jobs);
    let (_, rx) = std::sync::mpsc::channel();
    app.link_watch.inline = false;
    app.link_watch.worker = Some(Worker { tx, rx });
    tick(&mut app, 0.0, Some(true));
    assert!(app.link_watch.worker.is_none());
    assert!(!app.link_watch.busy);
    assert_eq!(state(&app, asset), Some(LinkState::Ok));
}

// ---- Canvas badges ----

fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(0.25).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn shown_badges(h: &Harness<'_, DesignApp>) -> Vec<Badge> {
    let app = h.state();
    let xf = crate::canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    link_badges::badges(app, &xf, Rect::EVERYTHING)
}

fn click(h: &mut Harness<'_, DesignApp>, p: Pos2) {
    h.hover_at(p);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
        h.step();
    }
    h.run_steps(2);
}

/// The modified badge sits inside the frame's top-left corner at a fixed size on screen; a click
/// on it updates the graphic in one undo step and selects nothing; undo shows it again.
#[test]
fn clicking_a_modified_badge_updates_the_graphic_without_selecting_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    let mut app = app();
    let (frame, asset) = place(&mut app, &file);
    let mut h = harness(app);
    assert!(shown_badges(&h).is_empty(), "an unchanged link has no badge");
    std::fs::write(&file, png(9)).unwrap();
    h.run_steps(16);
    let badges = shown_badges(&h);
    assert_eq!(badges.len(), 1);
    let b = badges[0];
    assert_eq!((b.asset, b.state), (asset, LinkState::Modified));
    assert_eq!(b.rect.size(), vec2(link_badges::SIZE, link_badges::SIZE));
    let frame_rect = {
        let app = h.state();
        let xf = crate::canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        let doc = &app.session.doc().unwrap().doc;
        let layout = designcraft_tools::CanvasLayout::new(doc, false);
        xf.rect(layout.slots[0].xf.transform_rect_bbox(doc.item(designcraft_doc::ItemId(frame)).unwrap().bounds()))
    };
    assert!(frame_rect.contains(b.rect.min) && (b.rect.min - frame_rect.min).length() < 8.0, "{:?} in {frame_rect:?}", b.rect);
    // The same size at another zoom.
    h.state_mut().run("view.zoomIn", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(shown_badges(&h)[0].rect.size(), vec2(link_badges::SIZE, link_badges::SIZE));
    let b = shown_badges(&h)[0];
    let undo = undo_len(h.state());
    let item_before = h.state().session.doc().unwrap().doc.item(designcraft_doc::ItemId(frame)).unwrap().bounds();
    click(&mut h, b.rect.center());
    let app = h.state();
    assert_eq!(pixels(app, asset), Some((9, 4)), "updated");
    assert_eq!(undo_len(app), undo + 1, "one undo step");
    assert!(app.session.doc().unwrap().selection.items.is_empty(), "the click didn't select the frame");
    assert_eq!(app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(frame)).unwrap().bounds(), item_before, "nor move it");
    assert!(shown_badges(&h).is_empty(), "the badge goes at once");
    // Undo: the old copy, so the badge is back after the next check.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(16);
    assert_eq!(shown_badges(&h).iter().map(|b| b.state).collect::<Vec<_>>(), [LinkState::Modified]);
    // Hide Frame Edges hides the badges; so does Preview.
    h.state_mut().ui.frame_edges = false;
    assert!(shown_badges(&h).is_empty());
    h.state_mut().ui.frame_edges = true;
    h.state_mut().ui.screen_mode = crate::ScreenMode::Preview;
    assert!(shown_badges(&h).is_empty());
}

/// A press on a badge dragged away is no click: nothing is updated, and the tool never saw the
/// press (no frame moved or selected).
#[test]
fn a_drag_from_a_badge_neither_updates_nor_moves_the_frame() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    let mut app = app();
    let (frame, asset) = place(&mut app, &file);
    std::fs::write(&file, png(9)).unwrap();
    let mut h = harness(app);
    h.run_steps(16);
    let b = shown_badges(&h)[0];
    let before = h.state().session.doc().unwrap().doc.item(designcraft_doc::ItemId(frame)).unwrap().bounds();
    let (a, to) = (b.rect.center(), b.rect.center() + vec2(80.0, 60.0));
    h.hover_at(a);
    h.step();
    h.event(egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    for k in 1..=4 {
        h.hover_at(a + (to - a) * (k as f32 / 4.0));
        h.step();
    }
    h.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
    let app = h.state();
    assert_eq!(pixels(app, asset), Some((4, 4)), "not updated");
    assert_eq!(app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(frame)).unwrap().bounds(), before, "not moved");
    assert!(app.session.doc().unwrap().selection.items.is_empty(), "not selected");
}

/// A missing file's badge relinks: the click asks for the file.
#[test]
fn clicking_a_missing_badge_relinks() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    let other = dir.path().join("found.png");
    std::fs::write(&file, png(4)).unwrap();
    std::fs::write(&other, png(6)).unwrap();
    let mut app = app();
    let (_, asset) = place(&mut app, &file);
    std::fs::remove_file(&file).unwrap();
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(other.to_string_lossy().to_string())));
    let mut h = harness(app);
    h.run_steps(16);
    let b = shown_badges(&h)[0];
    assert_eq!(b.state, LinkState::Missing);
    click(&mut h, b.rect.center());
    h.state_mut().settle_dialogs();
    h.run_steps(16);
    let app = h.state();
    assert_eq!(pixels(app, asset), Some((6, 4)), "relinked to the file chosen");
    assert!(shown_badges(&h).is_empty());
}

/// A check that finishes after the document changed (an edit, a Revert) doesn't update from what
/// it found: it shows the badges, and the next check, of the document as it is now, updates.
#[test]
fn a_check_of_an_older_revision_updates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    let mut app = app();
    let (_, asset) = place(&mut app, &file);
    std::fs::write(&file, png(9)).unwrap();
    let st = app.session.active().unwrap();
    let stale = Done { token: (st.uid, st.revision.wrapping_sub(1)), states: vec![(asset, LinkState::Modified)], update: true };
    let undo = undo_len(&app);
    apply(&mut app, stale);
    assert_eq!(state(&app, asset), Some(LinkState::Modified), "the badge shows");
    assert_eq!((pixels(&app, asset), undo_len(&app)), (Some((4, 4)), undo), "nothing updated");
    assert!(app.link_watch.again && app.link_watch.update, "checked again, to update");
    tick(&mut app, 0.0, Some(true));
    assert_eq!(pixels(&app, asset), Some((9, 4)), "updated from a check of this revision");
}

/// A check that found nothing modified but finished after the document changed keeps the request
/// to update: the next check, which finds the file modified, updates it.
#[test]
fn a_stale_check_that_found_nothing_keeps_the_update_request() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, png(4)).unwrap();
    let mut app = app();
    let (_, asset) = place(&mut app, &file);
    let st = app.session.active().unwrap();
    let stale = Done { token: (st.uid, st.revision.wrapping_sub(1)), states: vec![(asset, LinkState::Ok)], update: true };
    apply(&mut app, stale);
    assert!(app.link_watch.again && app.link_watch.update, "checked again, to update");
    std::fs::write(&file, png(9)).unwrap();
    tick(&mut app, 0.0, Some(true));
    assert_eq!(pixels(&app, asset), Some((9, 4)), "the request to update survived");
}
