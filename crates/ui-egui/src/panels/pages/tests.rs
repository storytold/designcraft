use egui::{Event, Modifiers, PointerButton};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

use super::*;
use crate::test_window;

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_panicking_thumbnail_leaves_its_row_undrawn_and_the_worker_alive() {
    // A renderer bug on one document must not end the thread: its slot would stay reserved, and
    // four of them would stop the panel from drawing any thumbnail again.
    let (drawn, panicked) = super::worker::contained(|| -> Option<u32> { panic!("a renderer bug") });
    assert!(drawn.is_none() && panicked);
    assert_eq!(super::worker::contained(|| Some(7)), (Some(7), false), "the next row draws");
}

#[test]
fn parent_context_menu_deletes_the_clicked_id_even_with_a_shared_prefix() {
    for vertical in [true, false] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 2, "facingPages": false})).unwrap();
        app.ui.pages_panel.parents_vertical = vertical;
        let original = app.session.doc().unwrap().doc.parents[0].id;
        let added = app.run("layout.parents.new", json!({"prefix": "A", "name": "Other"})).unwrap()["id"].as_u64().unwrap();
        let mut h = harness(app);
        // Target the actual parent row in either panel layout, not a menu helper.
        h.get_by_label("A-Other").click_secondary();
        h.run_steps(3);
        h.get_by_label("Delete Parent").click();
        h.run_steps(3);
        let doc = &h.state().session.doc().unwrap().doc;
        assert_eq!(doc.parents.len(), 1);
        assert_eq!(doc.parents[0].id, original);
        assert!(doc.parents.iter().all(|p| p.id.0 != added));
        assert_eq!(doc.page_count(), 2, "document pages are not deleted");
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.doc().unwrap().doc.parents.len(), 2);
    }
}

#[test]
fn double_clicking_a_parent_shows_that_parent() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    for prefix in ["B", "C", "D"] {
        app.run("layout.parents.new", json!({"prefix": prefix})).unwrap();
    }
    app.ui.dock_tab = "pages".into();
    let mut h = test_window::open(app, vec2(1400.0, 900.0));
    let names = ["A-Parent", "B-Parent", "C-Parent", "D-Parent"];
    for name in names {
        assert_ne!(h.get_by_label(name).accesskit_node().toggled(), Some(egui::accesskit::Toggled::True), "{name} is not selected yet");
    }
    // Both clicks in one frame: the harness's event queue spaces events too far apart for a
    // double click.
    let at = h.get_by_label("C-Parent").rect().center();
    h.input_mut().events.push(Event::PointerMoved(at));
    for pressed in [true, false, true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run_steps(4);
    let app = &h.state().app;
    assert!(app.session.active().unwrap().editing_parents);
    let st = app.session.active().unwrap();
    let layout = designcraft_tools::CanvasLayout::new(&st.doc, true);
    let slot = crate::canvas::current_slot(app, &layout).unwrap();
    assert_eq!(layout.slots[slot].spread, designcraft_doc::SpreadRef::Parent(2));
    for name in names {
        assert_eq!(h.get_by_label(name).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True), name == "C-Parent", "{name}");
    }
    // Parent rows show a rendered thumbnail.
    let id = st.doc.parents[2].id;
    let rev = (st.uid, st.revision);
    let thumbs = super::THUMBS.lock().unwrap();
    assert!(thumbs.as_ref().unwrap().keys().any(|k| (k.0, k.1) == rev && k.2 == super::ThumbOf::Parent(id)));
}

fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
    sized_harness(app, vec2(280.0, 520.0))
}

fn sized_harness(app: DesignApp, size: egui::Vec2) -> Harness<'static, DesignApp> {
    let mut h = Harness::builder().with_size(size).build_ui_state(
        |ui, app: &mut DesignApp| {
            let key = egui::Id::unique("test_fonts");
            if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(key, true));
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// Release a dragged page over `at`.
fn drop_page(h: &mut Harness<'_, DesignApp>, page: usize, at: egui::Pos2, alt: bool) {
    let modifiers = egui::Modifiers { alt, ..Default::default() };
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers });
    egui::DragAndDrop::set_payload(&h.ctx, PageDrag::Page(page));
    h.event(egui::Event::PointerMoved(at));
    h.run_steps(2);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers });
    h.run_steps(3);
}

/// The page menu for page `abs` of spread `si`.
fn menu_harness(app: DesignApp, abs: usize, si: usize) -> Harness<'static, DesignApp> {
    let mut h = Harness::builder().with_size(vec2(320.0, 600.0)).build_ui_state(
        move |ui, app: &mut DesignApp| {
            let key = egui::Id::unique("test_fonts");
            if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(key, true));
                return;
            }
            let doc = app.session.doc().unwrap().doc.clone();
            page_menu(app, ui, &doc, abs, si);
        },
        app,
    );
    h.run_steps(3);
    h
}

#[test]
fn a_page_in_a_spread_offers_delete_page_and_delete_spread() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 5, "facingPages": true})).unwrap();
    let doc = app.session.doc().unwrap().doc.clone();
    let (si, _) = doc.page_loc(2).unwrap();
    assert_eq!(doc.spreads[si].pages.len(), 2);
    let mut h = menu_harness(app, 2, si);
    let _ = h.get_by_label("Delete Spread");
    h.get_by_label("Delete Page").click();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().unwrap().doc.page_count(), 4, "only the clicked page");
}

/// As in InDesign: deleting a page with objects asks first; an empty page goes at once, and
/// "Don't show again" stops the question.
#[test]
fn deleting_a_page_with_objects_asks_first() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 5, "facingPages": true})).unwrap();
    let doc = app.session.doc().unwrap().doc.clone();
    let (si, pi) = doc.page_loc(2).unwrap();
    let x = doc.spreads[si].pages[pi].x;
    app.run("frame.create", json!({"spread": si, "rect": [x + 36.0, 36.0, x + 200.0, 200.0]})).unwrap();
    let count = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.page_count();
    let mut h = menu_harness(app, 2, si);
    h.get_by_label("Delete Page").click();
    h.run_steps(3);
    assert_eq!(count(&h), 5, "nothing deleted yet");
    let d = h.state().ui.dialog.clone().unwrap();
    assert_eq!((d.id.as_str(), d.fields["pages"].clone()), ("deletePages", json!([2])));
    // Cancel keeps the page.
    crate::dialogs::cancel(h.state_mut());
    assert_eq!(count(&h), 5);
    // The whole spread: one question for both pages.
    h.get_by_label("Delete Spread").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_ref().unwrap().fields["pages"], json!([1, 2]));
    crate::dialogs::cancel(h.state_mut());
    // OK deletes it.
    h.get_by_label("Delete Page").click();
    h.run_steps(3);
    crate::dialogs::confirm(h.state_mut()).unwrap();
    assert_eq!(count(&h), 4);
    assert!(!h.state().ui.skip_delete_pages_warning);
    // An empty page goes without a question.
    let app = h.state_mut();
    crate::dialogs::delete_pages(app, vec![0]).unwrap();
    assert!(app.ui.dialog.is_none());
    assert_eq!(app.session.doc().unwrap().doc.page_count(), 3);
    // "Don't show again".
    let doc = app.session.doc().unwrap().doc.clone();
    let (si, pi) = doc.page_loc(1).unwrap();
    let x = doc.spreads[si].pages[pi].x;
    app.run("frame.create", json!({"spread": si, "rect": [x + 36.0, 36.0, x + 200.0, 200.0]})).unwrap();
    app.run("frame.create", json!({"spread": si, "rect": [x + 36.0, 300.0, x + 200.0, 400.0]})).unwrap();
    crate::dialogs::delete_pages(app, vec![1]).unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("dontShowAgain".into(), json!(true));
    crate::dialogs::confirm(app).unwrap();
    assert!(app.ui.skip_delete_pages_warning);
    assert_eq!(app.session.doc().unwrap().doc.page_count(), 2);
    let doc = app.session.doc().unwrap().doc.clone();
    let (si, pi) = doc.page_loc(0).unwrap();
    let x = doc.spreads[si].pages[pi].x;
    app.run("frame.create", json!({"spread": si, "rect": [x + 36.0, 36.0, x + 200.0, 200.0]})).unwrap();
    crate::dialogs::delete_pages(app, vec![0]).unwrap();
    assert!(app.ui.dialog.is_none(), "no more questions");
    assert_eq!(app.session.doc().unwrap().doc.page_count(), 1);
}

/// As InDesign's: the footer stays at the bottom however many pages there are, and the
/// divider between parents and pages shows more or fewer parents.
#[test]
fn the_footer_stays_put_and_the_divider_resizes_the_parents() {
    use egui::{Event, Modifiers, PointerButton};
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 60, "facingPages": true})).unwrap();
    for prefix in ["B", "C", "D", "E", "F", "G"] {
        app.run("layout.parents.new", json!({"prefix": prefix})).unwrap();
    }
    app.ui.dock_tab = "pages".into();
    let mut h = crate::test_window::open(app, vec2(1400.0, 900.0));
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 900.0));
    for label in ["60 Pages in 31 Spreads", "Create new page", "Delete selected pages"] {
        assert!(window.contains_rect(h.get_by_label(label).rect()), "{label} is on screen");
    }
    // Seven parent rows: four and a half show at first; the rest scroll.
    let divider_node = h.get_by_label("Parents and pages divider");
    assert_eq!(divider_node.accesskit_node().role(), egui::accesskit::Role::Splitter, "the divider is a splitter, not an unknown node");
    let divider = divider_node.rect();
    let before = divider.center().y;
    assert!(h.query_by_label("G-Parent").is_none_or(|n| n.rect().top() > divider.top()), "the last parent is scrolled away");
    let (a, b) = (divider.center(), divider.center() + vec2(0.0, 80.0));
    h.input_mut().events.push(Event::PointerMoved(a));
    h.input_mut().events.push(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    for k in 1..=4 {
        h.input_mut().events.push(Event::PointerMoved(a + (b - a) * (k as f32 / 4.0)));
        h.step();
    }
    h.input_mut().events.push(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    let after = h.get_by_label("Parents and pages divider").rect().center().y;
    assert!(after > before + 40.0, "the divider moved down: {before} → {after}");
    assert!(h.state().app.ui.pages_parents_height.is_some());
    assert!(window.contains_rect(h.get_by_label("Create new page").rect()), "the footer is still there");
}

/// InDesign's Panel Options: thumbnail sizes, blank pages without thumbnails, parents side by
/// side, pages on top.
#[test]
fn panel_options_size_and_arrange_the_panel() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 6, "facingPages": true})).unwrap();
    app.run("layout.parents.new", json!({"prefix": "B"})).unwrap();
    app.ui.dock_tab = "pages".into();
    let mut h = crate::test_window::open(app, vec2(1400.0, 900.0));
    let page = |h: &Harness<'_, crate::test_window::Window>| h.get_by_label("Page 1").rect();
    let medium = page(&h).height();
    // The right-click menu on the panel's empty part leads to Panel Options.
    let empty = h.get_by_label("Create new page").rect().center() - vec2(0.0, 60.0);
    h.input_mut().events.push(egui::Event::PointerMoved(empty));
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: empty,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(3);
    h.get_by_label("Panel Options…").click();
    h.run_steps(3);
    let d = h.state().app.ui.dialog.clone().unwrap();
    assert_eq!(
        (d.id.as_str(), d.fields["pageSize"].clone(), d.fields["parentSize"].clone(), d.fields["layout"].clone()),
        ("pagesPanelOptions", json!("medium"), json!("small"), json!("parentsOnTop"))
    );
    // Jumbo pages, parents side by side, pages on top.
    let set = |h: &mut Harness<'_, crate::test_window::Window>, k: &str, v: serde_json::Value| {
        h.state_mut().app.ui.dialog.as_mut().unwrap().fields.insert(k.into(), v);
    };
    set(&mut h, "pageSize", json!("jumbo"));
    set(&mut h, "parentsVertical", json!(false));
    set(&mut h, "layout", json!("pagesOnTop"));
    crate::dialogs::confirm(&mut h.state_mut().app).unwrap();
    h.run_steps(4);
    assert!(page(&h).height() > 2.5 * medium, "Jumbo: {} vs {medium}", page(&h).height());
    let o = &h.state().app.ui.pages_panel;
    assert_eq!((o.page_size, o.parents_vertical, o.pages_on_top), (IconSize::Jumbo, false, true));
    // Pages on top: the parents come after the pages; side by side: [None] and A sit in one row.
    let (none, a) = (h.get_by_label("[None]").rect(), h.get_by_label("A-Parent").rect());
    assert!(none.top() > page(&h).top(), "parents below the pages");
    assert!((none.top() - a.top()).abs() < 1.0 && a.left() > none.right(), "side by side");
    // Extra Small: no thumbnails, just pages.
    crate::panels::pages::open_panel_options(&mut h.state_mut().app);
    set(&mut h, "pageSize", json!("extraSmall"));
    crate::dialogs::confirm(&mut h.state_mut().app).unwrap();
    h.run_steps(4);
    assert!(page(&h).height() < 0.5 * medium);
}

/// Panel Options' parent size sets the height of the parents' rendered thumbnails (cached per
/// parent and size), and Show Thumbnails off leaves them blank.
/// Parent rows taller than the room left for the parents (Extra Large or Jumbo in a short
/// panel) draw and drag without a panic.
#[test]
fn large_parent_rows_in_a_short_panel_do_not_panic() {
    use egui::{Event, Modifiers, PointerButton};
    for size in [IconSize::ExtraLarge, IconSize::Jumbo] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 4})).unwrap();
        app.ui.pages_panel.parent_size = size;
        let mut h = sized_harness(app, vec2(260.0, 160.0));
        let a = h.get_by_label("Parents and pages divider").rect().center();
        h.input_mut().events.push(Event::PointerMoved(a));
        h.input_mut().events.push(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.step();
        for dy in [20.0, 40.0, -60.0] {
            h.input_mut().events.push(Event::PointerMoved(a + vec2(0.0, dy)));
            h.step();
        }
        h.input_mut().events.push(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
        assert!(h.state().ui.pages_parents_height.is_some(), "{size:?}: the divider was dragged");
    }
}

#[test]
fn parent_thumbnails_follow_panel_options() {
    let parent_thumb_heights = |thumbnails: bool| {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.ui.dock_tab = "pages".into();
        app.ui.pages_panel.parent_size = IconSize::Large;
        app.ui.pages_panel.parent_thumbnails = thumbnails;
        let h = crate::test_window::open(app, vec2(1400.0, 900.0));
        let st = h.state().app.session.active().unwrap();
        let id = st.doc.parents[0].id;
        let thumbs = super::THUMBS.lock().unwrap();
        let keys = thumbs.as_ref().map(|m| m.keys().copied().collect::<Vec<_>>()).unwrap_or_default();
        keys.into_iter().filter(|k| k.0 == st.uid && k.2 == super::ThumbOf::Parent(id)).map(|k| k.3).collect::<Vec<_>>()
    };
    assert_eq!(parent_thumb_heights(true), vec![IconSize::Large.row_h() as u32 - 5]);
    assert_eq!(parent_thumb_heights(false), Vec::<u32>::new());
}

/// A long document draws thumbnails for the rows on screen, not for every page: the first frame
/// of a 190-page catalogue used to render all 190 on the interface thread (a 40-second freeze,
/// repeated on every edit).
#[test]
fn only_visible_pages_get_thumbnails() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 120, "facingPages": false})).unwrap();
    app.ui.dock_tab = "pages".into();
    let h = crate::test_window::open(app, vec2(1400.0, 900.0));
    let st = h.state().app.session.active().unwrap();
    let (uid, rev) = (st.uid, st.revision);
    let thumbs = super::THUMBS.lock().unwrap();
    let drawn = thumbs.as_ref().map_or(0, |m| m.keys().filter(|k| k.0 == uid && k.1 == rev && matches!(k.2, super::ThumbOf::Page(_))).count());
    assert!(drawn > 0, "the rows on screen are drawn");
    assert!(drawn < 40, "120 pages, but only the visible rows are drawn: {drawn}");
    assert!(
        !thumbs.as_ref().is_some_and(|m| m.keys().any(|k| k.0 == uid && k.1 == rev && k.2 == super::ThumbOf::Page(119))),
        "the last page is far below the panel"
    );
}

/// Background slots are capped, so a fast scroll across a long document queues a few thumbnails
/// rather than every page.
#[test]
fn background_thumbnails_are_capped() {
    let keys: Vec<ThumbKey> = (0..10).map(|i| (u64::MAX - 1, 0, ThumbOf::Page(i), 24)).collect();
    let taken = keys.iter().filter(|k| begin(**k)).count();
    assert_eq!(taken, super::MAX_IN_FLIGHT);
    assert!(!begin(keys[0]), "a slot is reserved once");
    for k in &keys {
        ended(*k);
    }
    assert!(begin(keys[0]), "released slots are free again");
    ended(keys[0]);
}

#[test]
fn the_panel_menu_follows_indesigns_order() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 2, "facingPages": false})).unwrap();
    let mut h = menu_harness(app, 0, 0);
    assert!(h.query_by_label("Delete Spread").is_none(), "a single-page spread has no Delete Spread");
    // InDesign's items, top to bottom.
    let ys: Vec<f32> = ["Insert Pages…", "Move Pages…", "Duplicate Spread", "Duplicate Spread at End of Document", "Delete Page", "New Parent…"]
        .iter()
        .map(|l| h.get_by_label(l).rect().center().y)
        .collect();
    assert!(ys.windows(2).all(|w| w[0] < w[1]), "{ys:?}");
    h.get_by_label("Duplicate Spread at End of Document").click();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().unwrap().doc.page_count(), 3);
    h.get_by_label("Insert Pages…").click();
    h.run_steps(3);
    assert!(h.state().ui.dialog.as_ref().is_some_and(|d| d.id == "cmd:layout.pages.insert"));
}

#[test]
fn a_page_dropped_on_create_new_page_is_duplicated_at_the_end() {
    // Chris Cicchino's Page Tool video (2:06): "click and drag onto my plus sign I can duplicate that".
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"width": 252, "height": 144, "facingPages": false})).unwrap();
    app.run("layout.pages.insert", json!({"count": 1})).unwrap();
    app.run("layout.pageSize", json!({"pages": [2], "width": 612, "height": 792})).unwrap();
    let mut h = harness(app);
    let plus = h.get_by_label("Create new page").rect().center();
    drop_page(&mut h, 1, plus, false);
    let d = h.state().session.doc().unwrap().doc.clone();
    assert_eq!(d.page_count(), 3);
    let (si, pi) = d.page_loc(2).unwrap();
    assert_eq!(d.spreads[si].pages[pi].width, 612.0, "a copy of the letterhead");
    let _ = h.get_by_label("Edit page size");
}
