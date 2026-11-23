use egui::{Event, Modifiers, PointerButton};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

use super::*;
use crate::test_window;

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
    // Parent rows show a rendered thumbnail (of the revision before the double click until the
    // document has been unchanged for THUMB_QUIET_SECS).
    let id = st.doc.parents[2].id;
    let thumbs = super::THUMBS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(thumbs.as_ref().unwrap().keys().any(|k| k.0 == st.uid && k.1 == super::ThumbOf::Parent(id)));
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
            let layouts = doc.layouts();
            page_menu(app, ui, &doc, &layouts, abs, si);
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
        keys.into_iter().filter(|k| k.0 == st.uid && k.1 == super::ThumbOf::Parent(id)).map(|k| k.2).collect::<Vec<_>>()
    };
    assert_eq!(parent_thumb_heights(true), vec![IconSize::Large.row_h() as u32 - 5]);
    assert_eq!(parent_thumb_heights(false), Vec::<u32>::new());
}

/// Page thumbnails are rendered only for rows in view. After an edit the previous thumbnails stay
/// until the document has been unchanged for `THUMB_QUIET_SECS`, and then one is drawn again per
/// frame. Thumbnails of deleted pages are dropped.
#[test]
fn an_edit_redraws_only_thumbnails_in_view_one_per_frame() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 60, "facingPages": false})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut DesignApp, time: f64| {
        let before = THUMB_RENDERS.with(std::cell::Cell::get);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(280.0, 520.0))),
            time: Some(time),
            ..Default::default()
        };
        ctx.run_ui(raw, |ui| show(app, ui)).textures_delta.clear();
        THUMB_RENDERS.with(std::cell::Cell::get) - before
    };
    let shown: usize = (0..3).map(|_| frame(&mut app, 0.0)).sum();
    assert!(shown > 0 && shown < 60, "only the rows in view are rendered: {shown}");
    assert_eq!(frame(&mut app, 0.0), 0, "an unchanged document renders nothing");

    app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap();
    assert_eq!(frame(&mut app, 1.0), 0, "the edit's frame keeps the previous thumbnails");
    assert_eq!(frame(&mut app, 1.0 + THUMB_QUIET_SECS / 2.0), 0, "still editing");
    let quiet = 1.0 + THUMB_QUIET_SECS + 0.01;
    let mut redrawn = 0;
    for _ in 0..shown {
        let n = frame(&mut app, quiet);
        assert_eq!(n, THUMB_REFRESHES_PER_FRAME, "one thumbnail per frame");
        redrawn += n;
    }
    assert_eq!(redrawn, shown);
    assert_eq!(frame(&mut app, quiet), 0, "every thumbnail in view is current");

    // Deleted pages lose their thumbnails.
    let uid = app.session.active().unwrap().uid;
    let cached = |abs: usize| {
        THUMBS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().unwrap().keys().any(|k| k.0 == uid && k.1 == ThumbOf::Page(abs))
    };
    assert!(cached(2));
    app.run("layout.pages.delete", json!({"pages": (2..60).collect::<Vec<_>>()})).unwrap();
    frame(&mut app, quiet + 1.0);
    assert!((2..60).all(|abs| !cached(abs)), "no thumbnail of a deleted page is kept");
}

/// Switching to a document whose thumbnails are all current stores that document in the clock,
/// so the next frames do not scan the whole cache again.
#[test]
fn switching_to_a_cached_document_stores_it_in_the_clock() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let frame = |app: &mut DesignApp| {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(280.0, 520.0))),
            time: Some(0.0),
            ..Default::default()
        };
        ctx.run_ui(raw, |ui| show(app, ui)).textures_delta.clear();
    };
    for _ in 0..2 {
        app.run("file.new", json!({"pages": 2, "facingPages": false})).unwrap();
        frame(&mut app);
    }
    app.session.set_active(0);
    let st = app.session.active().unwrap();
    let first = (st.uid, st.revision);
    let before = THUMB_RENDERS.with(std::cell::Cell::get);
    frame(&mut app);
    assert_eq!(THUMB_RENDERS.with(std::cell::Cell::get), before, "the first document's thumbnails are current");
    let clock = ctx.data(|d| d.get_temp::<ThumbClock>(egui::Id::unique("pages_thumb_clock"))).unwrap();
    assert_eq!(clock.doc, first);
}

#[test]
fn parents_are_added_duplicated_and_renamed_from_the_panel() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 2, "facingPages": false})).unwrap();
    let mut h = harness(app);
    // One click on New Parent adds B-Parent and selects it.
    h.get_by_label("New Parent").click();
    h.run_steps(3);
    let doc = h.state().session.doc().unwrap().doc.clone();
    let b = doc.parents.iter().find(|p| p.parent.as_ref().is_some_and(|i| i.label() == "B-Parent")).map(|p| p.id);
    assert!(b.is_some());
    assert_eq!(selected_parent(h.state()), b);
    // Its menu duplicates it as C-Parent.
    h.get_by_label("B-Parent").click_secondary();
    h.run_steps(3);
    h.get_by_label("Duplicate Parent Spread").click();
    h.run_steps(3);
    let labels: Vec<String> = h.state().session.doc().unwrap().doc.parents.iter().filter_map(|p| p.parent.as_ref().map(|i| i.label())).collect();
    assert_eq!(labels, ["A-Parent", "B-Parent", "C-Parent"]);
    // Parent Options… renames prefix and name.
    h.get_by_label("C-Parent").click_secondary();
    h.run_steps(3);
    h.get_by_label("Parent Options…").click();
    h.run_steps(3);
    let app = h.state_mut();
    let d = app.ui.dialog.as_mut().unwrap();
    assert_eq!(d.id, "parentOptions");
    assert_eq!((d.s("prefix"), d.s("name")), ("C".into(), "Parent".into()));
    d.fields.insert("prefix".into(), json!("CV"));
    d.fields.insert("name".into(), json!("Cover"));
    d.fields.insert("basedOn".into(), json!(b.unwrap().0.to_string()));
    crate::dialogs::confirm(app).unwrap();
    let doc = &app.session.doc().unwrap().doc;
    let info = doc.parents[2].parent.as_ref().unwrap();
    assert_eq!((info.label(), info.based_on), ("CV-Cover".into(), b));
}

#[test]
fn the_panel_menu_names_the_selected_parent_or_lists_the_parents() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 2, "facingPages": false})).unwrap();
    let h = menu_harness(app, 0, 0);
    assert!(h.query_by_label("Parent Options").is_some(), "no parent selected: a submenu of the parents");
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 2, "facingPages": false})).unwrap();
    let first = app.session.doc().unwrap().doc.parents[0].id;
    select_parent(&mut app, Some(first));
    let mut h = menu_harness(app, 0, 0);
    assert!(h.query_by_label("Delete Parent “A-Parent”").is_some());
    h.get_by_label("Duplicate Parent Spread “A-Parent”").click();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().unwrap().doc.parents.len(), 2);
    // Duplicating selected the copy, so the menu now names it.
    assert!(h.query_by_label("Parent Options “B-Parent”…").is_some());
    h.get_by_label("Parent Options “B-Parent”…").click();
    h.run_steps(3);
    assert!(h.state().ui.dialog.as_ref().is_some_and(|d| d.id == "parentOptions" && d.s("prefix") == "B"));
}

#[test]
fn alternate_layouts_show_as_columns_with_their_own_menus() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 2, "width": 612, "height": 792})).unwrap();
    let single = sized_harness(app, vec2(600.0, 520.0));
    assert!(single.query_by_label("Layout 1").is_none(), "one layout: no column headers");
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 2, "width": 612, "height": 792})).unwrap();
    app.run("layout.createAlternate", json!({"name": "Tablet H", "width": 1024, "height": 768})).unwrap();
    let mut h = sized_harness(app, vec2(600.0, 520.0));
    let (a, b) = (h.get_by_label("Layout 1").rect(), h.get_by_label("Tablet H").rect());
    assert!(b.min.x > a.max.x - 1.0, "the alternate layout is a column to the right: {a:?} {b:?}");
    // The + column creates another, from the current layout.
    h.get_by_label("Create Alternate Layout…").click();
    h.run_steps(3);
    let d = h.state().ui.dialog.clone().unwrap();
    assert_eq!(d.id, "cmd:layout.createAlternate");
    assert_eq!(d.s("from"), "Layout 1");
    h.state_mut().ui.dialog = None;
    // The header's menu duplicates and deletes.
    h.get_by_label("Layout Options: Tablet H").click();
    h.run_steps(3);
    h.get_by_label("Duplicate Alternate Layout").click();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().unwrap().doc.layouts().len(), 3);
    h.get_by_label("Layout Options: Tablet H").click();
    h.run_steps(3);
    h.get_by_label("Delete Alternate Layout").click();
    h.run_steps(3);
    let names: Vec<String> = h.state().session.doc().unwrap().doc.layouts().into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["Layout 1", "Tablet H copy"]);
}

#[test]
fn renaming_an_alternate_layout_keeps_its_page_sizes() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 1, "width": 600, "height": 800, "facingPages": false})).unwrap();
    app.run("layout.createAlternate", json!({"name": "Wide", "width": 900, "height": 600})).unwrap();
    app.run("layout.pages.insert", json!({"after": 1, "count": 1})).unwrap();
    app.run("layout.pageSize", json!({"pages": [3], "width": 400, "height": 400})).unwrap();
    let sizes = |app: &DesignApp| (1..3).map(|i| app.session.doc().unwrap().doc.page(i).map(|p| (p.width, p.height))).collect::<Vec<_>>();
    let open = |app: &mut DesignApp| {
        let doc = app.session.doc().unwrap().doc.clone();
        crate::panels::pages::open_layout_options(app, &doc, &doc.layouts(), 1);
    };
    // The dialog shows the first page's size (900 × 600); renaming leaves the 400 × 400 page.
    open(&mut app);
    app.ui.dialog.as_mut().unwrap().fields.insert("name".into(), json!("Landscape"));
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(sizes(&app), vec![Some((900.0, 600.0)), Some((400.0, 400.0))]);
    assert_eq!(app.session.doc().unwrap().doc.layouts()[1].name, "Landscape");
    // A changed width still resizes the layout's pages.
    open(&mut app);
    app.ui.dialog.as_mut().unwrap().fields.insert("width".into(), json!("1000"));
    crate::dialogs::confirm(&mut app).unwrap();
    assert!(sizes(&app).iter().all(|s| s.is_some_and(|(w, _)| (w - 1000.0).abs() < 1e-6)), "{:?}", sizes(&app));
}

#[test]
fn layout_menu_options_open_the_pages_panel_dialogs() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 2, "facingPages": false})).unwrap();
    app.run("layout.parents.new", json!({"prefix": "B"})).unwrap();
    app.run("app.parentOptions", json!({})).unwrap();
    assert!(app.ui.dialog.as_ref().is_some_and(|d| d.id == "parentOptions" && d.s("prefix") == "A"), "no selection: the first parent");
    let b = app.session.doc().unwrap().doc.parents[1].id;
    select_parent(&mut app, Some(b));
    app.run("app.parentOptions", json!({})).unwrap();
    assert!(app.ui.dialog.as_ref().is_some_and(|d| d.s("prefix") == "B"));
    // A prefix the engine refuses (A is taken): the dialog stays, with what was typed and why.
    if let Some(d) = app.ui.dialog.as_mut() {
        d.fields.insert("prefix".into(), json!("A"));
    }
    assert!(crate::dialogs::confirm(&mut app).is_err());
    let d = app.ui.dialog.as_ref().expect("the dialog stays open");
    assert!(d.id == "parentOptions" && d.s("prefix") == "A" && !d.s("status").is_empty());
    app.ui.dialog = None;
    app.run("layout.createAlternate", json!({"name": "Wide", "width": 900, "height": 600})).unwrap();
    app.run("app.alternateLayoutOptions", json!({})).unwrap();
    assert!(app.ui.dialog.as_ref().is_some_and(|d| d.id == "cmd:layout.alternate.options" && d.s("layout") == "Layout 1"));
    // A selection belongs to its document.
    app.run("file.new", json!({"pages": 1})).unwrap();
    assert_eq!(selected_parent(&app), None);
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
    assert!(h.state().ui.dialog.as_ref().is_some_and(|d| d.id == "insertPages"));
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

/// The panel on the installed framework, driven by real pointer and key events through the app's own input hook.
#[cfg(not(target_arch = "wasm32"))]
mod framework_path {
    use std::sync::Arc;

    use egui::Key;

    use super::*;

    type Installed = Harness<'static, DesignApp>;

    fn app_with(pages: u32, facing: bool) -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": pages, "facingPages": facing})).unwrap();
        app
    }

    /// The panel on the installed framework, with a canvas for its clicks to move.
    fn open(mut app: DesignApp, size: egui::Vec2) -> Installed {
        app.framework_opacity = true;
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0)));
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        assert!(app.framework_installed, "the framework panel needs the installed runtime");
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(size).with_step_dt(1.0 / 60.0).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    ready = true;
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 4);
        h
    }

    fn event(h: &mut Installed, event: Event, modifiers: Modifiers) {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(Event::ModifiersChanged(modifiers));
        raw.events.push(event);
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    fn steps(h: &mut Installed, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn press(h: &mut Installed, at: egui::Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) {
        event(h, Event::PointerButton { pos: at, button, pressed, modifiers }, modifiers);
    }

    fn click_at(h: &mut Installed, at: egui::Pos2, button: PointerButton) {
        event(h, Event::PointerMoved(at), Modifiers::NONE);
        for pressed in [true, false] {
            press(h, at, button, pressed, Modifiers::NONE);
        }
        steps(h, 4);
    }

    fn pos_of(h: &Installed, label: &str) -> egui::Pos2 {
        h.get_by_label(label).rect().center()
    }

    fn click(h: &mut Installed, label: &str) {
        let at = pos_of(h, label);
        click_at(h, at, PointerButton::Primary);
    }

    fn right_click(h: &mut Installed, label: &str) {
        let at = pos_of(h, label);
        click_at(h, at, PointerButton::Secondary);
    }

    fn double_click(h: &mut Installed, label: &str) {
        let at = pos_of(h, label);
        event(h, Event::PointerMoved(at), Modifiers::NONE);
        for pressed in [true, false, true, false] {
            press(h, at, PointerButton::Primary, pressed, Modifiers::NONE);
        }
        steps(h, 4);
    }

    /// A press on `from` that moves past the drag slop, follows the pointer to `to` and is released there.
    fn drag(h: &mut Installed, from: egui::Pos2, to: egui::Pos2, modifiers: Modifiers) {
        event(h, Event::PointerMoved(from), modifiers);
        press(h, from, PointerButton::Primary, true, modifiers);
        event(h, Event::PointerMoved(from + vec2(12.0, 0.0)), modifiers);
        steps(h, 2);
        for n in 1..=5 {
            event(h, Event::PointerMoved(from + (to - from) * (n as f32 / 5.0)), modifiers);
        }
        press(h, to, PointerButton::Primary, false, modifiers);
        steps(h, 4);
    }

    fn key(h: &mut Installed, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }, Modifiers::NONE);
        }
        steps(h, 3);
    }

    fn document(h: &Installed) -> Arc<designcraft_doc::Document> {
        h.state().session.doc().unwrap().doc.clone()
    }

    fn journal_from(h: &Installed, start: usize) -> Vec<(String, serde_json::Value)> {
        h.state().session.journal[start..].to_vec()
    }

    fn selected(h: &Installed, label: &str) -> bool {
        h.get_by_label(label).accesskit_node().is_selected() == Some(true)
    }

    /// A right-click on a parent offers the parent commands of that parent, whichever way the parents are laid out.
    #[test]
    fn the_parent_menu_deletes_the_clicked_id_even_with_a_shared_prefix() {
        for vertical in [true, false] {
            let mut app = app_with(2, false);
            app.ui.pages_panel.parents_vertical = vertical;
            let original = app.session.doc().unwrap().doc.parents[0].id;
            let added = app.run("layout.parents.new", json!({"prefix": "A", "name": "Other"})).unwrap()["id"].as_u64().unwrap();
            let mut h = open(app, vec2(280.0, 520.0));
            right_click(&mut h, "A-Other");
            click(&mut h, "Delete Parent");
            let doc = document(&h);
            assert_eq!(doc.parents.len(), 1, "vertical: {vertical}");
            assert_eq!(doc.parents[0].id, original);
            assert!(doc.parents.iter().all(|p| p.id.0 != added));
            assert_eq!(doc.page_count(), 2, "document pages are not deleted");
            h.state_mut().run("edit.undo", json!({})).unwrap();
            assert_eq!(h.state().session.doc().unwrap().doc.parents.len(), 2);
        }
    }

    /// A double click on a parent edits that parent; its row is the selected one.
    #[test]
    fn double_clicking_a_parent_shows_that_parent() {
        for vertical in [true, false] {
            let mut app = app_with(2, false);
            app.ui.pages_panel.parents_vertical = vertical;
            for prefix in ["B", "C", "D"] {
                app.run("layout.parents.new", json!({"prefix": prefix})).unwrap();
            }
            let wanted = app.session.doc().unwrap().doc.parents[2].id;
            let mut h = open(app, vec2(400.0, 520.0));
            assert!(!selected(&h, "C-Parent"), "not selected yet");
            let start = h.state().session.journal.len();
            double_click(&mut h, "C-Parent");
            assert!(h.state().session.active().unwrap().editing_parents, "vertical: {vertical}");
            assert_eq!(journal_from(&h, start).last(), Some(&("layout.parents.edit".to_owned(), json!({"on": true, "parent": wanted.0}))));
            // Parent rows show a rendered thumbnail.
            let uid = h.state().session.active().unwrap().uid;
            let thumbs = super::super::THUMBS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            assert!(thumbs.as_ref().unwrap().keys().any(|k| k.0 == uid && k.1 == super::super::ThumbOf::Parent(wanted)));
        }
    }

    /// The menu of a page in a spread offers Delete Page and Delete Spread; an empty page goes at once, a page with
    /// objects asks first.
    #[test]
    fn a_page_menu_offers_delete_page_and_delete_spread() {
        let mut app = app_with(5, true);
        let doc = app.session.doc().unwrap().doc.clone();
        let (si, pi) = doc.page_loc(2).unwrap();
        assert_eq!(doc.spreads[si].pages.len(), 2);
        let x = doc.spreads[si].pages[pi].x;
        app.run("frame.create", json!({"spread": si, "rect": [x + 36.0, 36.0, x + 200.0, 200.0]})).unwrap();
        let mut h = open(app, vec2(300.0, 700.0));
        right_click(&mut h, "Page 3");
        assert!(h.query_by_label("Delete Spread").is_some());
        click(&mut h, "Delete Page");
        assert_eq!(document(&h).page_count(), 5, "a page with objects asks first");
        let d = h.state().ui.dialog.clone().unwrap();
        assert_eq!((d.id.as_str(), d.fields["pages"].clone()), ("deletePages", json!([2])));
        crate::dialogs::cancel(h.state_mut());
        right_click(&mut h, "Page 3");
        click(&mut h, "Delete Spread");
        assert_eq!(h.state().ui.dialog.as_ref().unwrap().fields["pages"], json!([1, 2]));
        crate::dialogs::cancel(h.state_mut());
        // An empty page goes without a question.
        right_click(&mut h, "Page 1");
        assert!(h.query_by_label("Delete Spread").is_none(), "a single-page spread has no Delete Spread");
        click(&mut h, "Delete Page");
        assert_eq!(document(&h).page_count(), 4);
        assert!(h.state().ui.dialog.is_none());
    }

    /// The panel menu follows InDesign's order, as the native menu does.
    #[test]
    fn the_page_menu_follows_indesigns_order() {
        let mut h = open(app_with(2, false), vec2(320.0, 700.0));
        right_click(&mut h, "Page 1");
        let ys: Vec<f32> = ["Insert Pages…", "Move Pages…", "Duplicate Spread", "Duplicate Spread at End of Document", "Delete Page", "New Parent…"]
            .iter()
            .map(|label| h.get_by_label(label).rect().center().y)
            .collect();
        assert!(ys.windows(2).all(|w| w[0] < w[1]), "{ys:?}");
        click(&mut h, "Duplicate Spread at End of Document");
        assert_eq!(document(&h).page_count(), 3);
        right_click(&mut h, "Page 1");
        click(&mut h, "Insert Pages…");
        assert!(h.state().ui.dialog.as_ref().is_some_and(|d| d.id == "insertPages"));
    }

    /// As InDesign's: the footer stays at the bottom however many pages there are, and the divider between parents and
    /// pages shows more or fewer parents.
    #[test]
    fn the_footer_stays_put_and_the_divider_resizes_the_parents() {
        let mut app = app_with(60, true);
        for prefix in ["B", "C", "D", "E", "F", "G"] {
            app.run("layout.parents.new", json!({"prefix": prefix})).unwrap();
        }
        let mut h = open(app, vec2(320.0, 900.0));
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 900.0));
        for label in ["60 Pages in 31 Spreads", "Create new page", "Delete selected pages"] {
            assert!(window.contains_rect(h.get_by_label(label).rect()), "{label} is on screen");
        }
        let divider_node = h.get_by_label("Parents and pages divider");
        assert_eq!(divider_node.accesskit_node().role(), egui::accesskit::Role::Splitter, "the divider is a splitter");
        let divider = divider_node.rect();
        let before = divider.center().y;
        assert!(h.query_by_label("G-Parent").is_none_or(|n| n.rect().top() > divider.top()), "the last parent is scrolled away");
        let from = divider.center();
        drag(&mut h, from, from + vec2(0.0, 80.0), Modifiers::NONE);
        let after = h.get_by_label("Parents and pages divider").rect().center().y;
        assert!(after > before + 40.0, "the divider moved down: {before} → {after}");
        assert!(h.state().ui.pages_parents_height.is_some());
        assert!(window.contains_rect(h.get_by_label("Create new page").rect()), "the footer is still there");
    }

    /// Panel Options' sizes and arrangement reach the installed panel, from the menu on its empty part.
    #[test]
    fn panel_options_size_and_arrange_the_panel() {
        let mut app = app_with(6, true);
        app.run("layout.parents.new", json!({"prefix": "B"})).unwrap();
        let mut h = open(app, vec2(400.0, 900.0));
        let page = |h: &Installed| h.get_by_label("Page 1").rect();
        let medium = page(&h).height();
        let empty = pos_of(&h, "Create new page") - vec2(0.0, 40.0);
        click_at(&mut h, empty, PointerButton::Secondary);
        click(&mut h, "Panel Options…");
        let d = h.state().ui.dialog.clone().unwrap();
        assert_eq!(
            (d.id.as_str(), d.fields["pageSize"].clone(), d.fields["parentSize"].clone(), d.fields["layout"].clone()),
            ("pagesPanelOptions", json!("medium"), json!("small"), json!("parentsOnTop"))
        );
        let set = |h: &mut Installed, k: &str, v: serde_json::Value| {
            h.state_mut().ui.dialog.as_mut().unwrap().fields.insert(k.into(), v);
        };
        set(&mut h, "pageSize", json!("jumbo"));
        set(&mut h, "parentsVertical", json!(false));
        set(&mut h, "layout", json!("pagesOnTop"));
        crate::dialogs::confirm(h.state_mut()).unwrap();
        steps(&mut h, 4);
        assert!(page(&h).height() > 2.5 * medium, "Jumbo: {} vs {medium}", page(&h).height());
        let o = &h.state().ui.pages_panel;
        assert_eq!((o.page_size, o.parents_vertical, o.pages_on_top), (IconSize::Jumbo, false, true));
        // Pages on top: the parents come after the pages; side by side: [None] and A sit in one row.
        let (none, a) = (h.get_by_label("[None]").rect(), h.get_by_label("A-Parent").rect());
        assert!(none.top() > page(&h).top(), "parents below the pages");
        assert!((none.top() - a.top()).abs() < 1.0 && a.left() > none.right(), "side by side");
        crate::panels::pages::open_panel_options(h.state_mut());
        set(&mut h, "pageSize", json!("extraSmall"));
        crate::dialogs::confirm(h.state_mut()).unwrap();
        steps(&mut h, 4);
        assert!(page(&h).height() < 0.5 * medium);
    }

    /// Parent rows taller than the room left for the parents draw and drag without a panic.
    #[test]
    fn large_parent_rows_in_a_short_panel_do_not_panic() {
        for size in [IconSize::ExtraLarge, IconSize::Jumbo] {
            for vertical in [true, false] {
                let mut app = app_with(4, false);
                app.ui.pages_panel.parent_size = size;
                app.ui.pages_panel.parents_vertical = vertical;
                let mut h = open(app, vec2(260.0, 160.0));
                let a = pos_of(&h, "Parents and pages divider");
                for dy in [20.0, 40.0, -60.0] {
                    drag(&mut h, a, a + vec2(0.0, dy), Modifiers::NONE);
                }
                assert!(h.state().ui.pages_parents_height.is_some(), "{size:?}: the divider was dragged");
            }
        }
    }

    /// Panel Options' parent size sets the height of the parents' rendered thumbnails, and Show Thumbnails off leaves
    /// them blank.
    #[test]
    fn parent_thumbnails_follow_panel_options() {
        let heights = |thumbnails: bool| {
            let mut app = app_with(1, false);
            app.ui.pages_panel.parent_size = IconSize::Large;
            app.ui.pages_panel.parent_thumbnails = thumbnails;
            let h = open(app, vec2(400.0, 900.0));
            let st = h.state().session.active().unwrap();
            let id = st.doc.parents[0].id;
            let thumbs = super::super::THUMBS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let keys = thumbs.as_ref().map(|m| m.keys().copied().collect::<Vec<_>>()).unwrap_or_default();
            keys.into_iter().filter(|k| k.0 == st.uid && k.1 == super::super::ThumbOf::Parent(id)).map(|k| k.2).collect::<Vec<_>>()
        };
        assert_eq!(heights(true), vec![IconSize::Large.row_h() as u32 - 5]);
        assert_eq!(heights(false), Vec::<u32>::new());
    }

    /// Thumbnails of pages are rendered only for the pages in view, and an unchanged document renders nothing more.
    #[test]
    fn only_the_thumbnails_in_view_are_rendered() {
        let before = THUMB_RENDERS.with(std::cell::Cell::get);
        let mut h = open(app_with(60, false), vec2(280.0, 520.0));
        steps(&mut h, 8);
        let shown = THUMB_RENDERS.with(std::cell::Cell::get) - before;
        assert!(shown > 0 && shown < 60, "only the rows in view are rendered: {shown}");
        let settled = THUMB_RENDERS.with(std::cell::Cell::get);
        steps(&mut h, 8);
        assert_eq!(THUMB_RENDERS.with(std::cell::Cell::get), settled, "an unchanged document renders nothing");
    }

    /// New Parent adds the next parent and selects it, its menu duplicates it, and Parent Options… opens for it.
    #[test]
    fn parents_are_added_duplicated_and_given_options_from_the_panel() {
        let mut h = open(app_with(2, false), vec2(300.0, 520.0));
        click(&mut h, "New Parent");
        let doc = document(&h);
        let b = doc.parents.iter().find(|p| p.parent.as_ref().is_some_and(|i| i.label() == "B-Parent")).map(|p| p.id);
        assert!(b.is_some());
        assert_eq!(selected_parent(h.state()), b);
        right_click(&mut h, "B-Parent");
        click(&mut h, "Duplicate Parent Spread");
        let labels: Vec<String> = h.state().session.doc().unwrap().doc.parents.iter().filter_map(|p| p.parent.as_ref().map(|i| i.label())).collect();
        assert_eq!(labels, ["A-Parent", "B-Parent", "C-Parent"]);
        right_click(&mut h, "C-Parent");
        click(&mut h, "Parent Options…");
        let d = h.state().ui.dialog.clone().unwrap();
        assert_eq!((d.id.as_str(), d.s("prefix"), d.s("name")), ("parentOptions", "C".into(), "Parent".into()));
    }

    /// Alternate layouts stand side by side, each with its own header and menu.
    #[test]
    fn alternate_layouts_show_as_columns_with_their_own_menus() {
        let mut app = app_with(2, false);
        app.run("layout.createAlternate", json!({"name": "Tablet H", "width": 1024, "height": 768})).unwrap();
        let mut h = open(app, vec2(600.0, 520.0));
        let (a, b) = (h.get_by_label("Layout 1").rect(), h.get_by_label("Tablet H").rect());
        assert!(b.min.x > a.max.x - 1.0, "the alternate layout is a column to the right: {a:?} {b:?}");
        click(&mut h, "Create Alternate Layout…");
        let d = h.state().ui.dialog.clone().unwrap();
        assert_eq!((d.id.as_str(), d.s("from")), ("cmd:layout.createAlternate", "Layout 1".into()));
        h.state_mut().ui.dialog = None;
        click(&mut h, "Layout Options: Tablet H");
        click(&mut h, "Duplicate Alternate Layout");
        assert_eq!(document(&h).layouts().len(), 3);
        click(&mut h, "Layout Options: Tablet H");
        click(&mut h, "Delete Alternate Layout");
        let names: Vec<String> = document(&h).layouts().into_iter().map(|l| l.name).collect();
        assert_eq!(names, ["Layout 1", "Tablet H copy"]);
        // A double click on a header opens its options.
        double_click(&mut h, "Layout 1");
        assert!(h.state().ui.dialog.as_ref().is_some_and(|d| d.id == "cmd:layout.alternate.options" && d.s("layout") == "Layout 1"));
    }

    /// A page dragged onto another page moves there; with Alt, a page that is a spread of its own is copied.
    #[test]
    fn a_page_dragged_onto_a_page_moves_and_with_alt_copies_it() {
        let mut h = open(app_with(4, false), vec2(300.0, 700.0));
        let start = h.state().session.journal.len();
        let (from, to) = (pos_of(&h, "Page 1"), pos_of(&h, "Page 3"));
        drag(&mut h, from, to, Modifiers::NONE);
        assert_eq!(journal_from(&h, start), [("layout.pages.move".to_owned(), json!({"from": 0, "to": 2}))]);
        let start = h.state().session.journal.len();
        let (from, to) = (pos_of(&h, "Page 2"), pos_of(&h, "Page 4"));
        drag(&mut h, from, to, Modifiers::ALT);
        assert_eq!(journal_from(&h, start), [("layout.pages.duplicateSpread".to_owned(), json!({"spread": 1, "to": 3}))]);
        assert_eq!(document(&h).page_count(), 5);
        // A page dropped on itself changes nothing.
        let start = h.state().session.journal.len();
        let at = pos_of(&h, "Page 1");
        drag(&mut h, at, at + vec2(1.0, 0.0), Modifiers::NONE);
        assert_eq!(h.state().session.journal.len(), start);
    }

    /// A parent dragged from the list, or from a tile, onto a page applies to that page.
    #[test]
    fn a_parent_dragged_onto_a_page_applies_it() {
        for vertical in [true, false] {
            let mut app = app_with(3, false);
            app.ui.pages_panel.parents_vertical = vertical;
            let parent = app.run("layout.parents.new", json!({"prefix": "B"})).unwrap()["id"].as_u64().unwrap();
            let mut h = open(app, vec2(300.0, 700.0));
            let start = h.state().session.journal.len();
            let (from, to) = (pos_of(&h, "B-Parent"), pos_of(&h, "Page 2"));
            drag(&mut h, from, to, Modifiers::NONE);
            assert_eq!(
                journal_from(&h, start),
                [("layout.pages.applyParent".to_owned(), json!({"pages": [1], "parent": parent}))],
                "vertical: {vertical}"
            );
            // [None] applies no parent.
            let start = h.state().session.journal.len();
            let (from, to) = (pos_of(&h, "[None]"), pos_of(&h, "Page 3"));
            drag(&mut h, from, to, Modifiers::NONE);
            assert_eq!(journal_from(&h, start), [("layout.pages.applyParent".to_owned(), json!({"pages": [2], "parent": null}))]);
        }
    }

    /// A page dropped on Create new page is duplicated at the end of the document.
    #[test]
    fn a_page_dropped_on_create_new_page_is_duplicated_at_the_end() {
        // Chris Cicchino's Page Tool video (2:06): "click and drag onto my plus sign I can duplicate that".
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width": 252, "height": 144, "facingPages": false})).unwrap();
        app.run("layout.pages.insert", json!({"count": 1})).unwrap();
        app.run("layout.pageSize", json!({"pages": [2], "width": 612, "height": 792})).unwrap();
        let mut h = open(app, vec2(280.0, 520.0));
        let (from, plus) = (pos_of(&h, "Page 2"), pos_of(&h, "Create new page"));
        drag(&mut h, from, plus, Modifiers::NONE);
        let d = document(&h);
        assert_eq!(d.page_count(), 3);
        let (si, pi) = d.page_loc(2).unwrap();
        assert_eq!(d.spreads[si].pages[pi].width, 612.0, "a copy of the letterhead");
        // A parent dropped on it adds no page.
        let mut app = app_with(2, false);
        app.run("layout.parents.new", json!({"prefix": "B"})).unwrap();
        let mut h = open(app, vec2(280.0, 520.0));
        let (from, plus) = (pos_of(&h, "B-Parent"), pos_of(&h, "Create new page"));
        drag(&mut h, from, plus, Modifiers::NONE);
        assert_eq!(document(&h).page_count(), 2);
    }

    /// A section marker opens its page's numbering options on a double click or from the keyboard, not on one click, and
    /// leaves parent editing without editing the document.
    #[test]
    fn a_section_marker_opens_its_options_from_a_double_click_or_the_keyboard_but_not_on_one_click() {
        let mut app = app_with(9, true);
        app.run("layout.section", json!({"page": 1, "startNumber": 1, "style": "lowerRoman"})).unwrap();
        app.run("layout.section", json!({"page": 3, "startNumber": 5})).unwrap();
        app.run("layout.parents.edit", json!({"on": true})).unwrap();
        let mut h = open(app, vec2(280.0, 900.0));
        let before = document(&h);
        click(&mut h, "Numbering & Section Options: 5");
        assert!(h.state().ui.dialog.is_none(), "one pointer click doesn't open the options");
        double_click(&mut h, "Numbering & Section Options: 5");
        let dialog = h.state().ui.dialog.as_ref().unwrap();
        assert_eq!((dialog.id.as_str(), dialog.fields.get("page")), ("numberingSection", Some(&json!(3))));
        assert!(!h.state().session.active().unwrap().editing_parents);
        assert!(Arc::ptr_eq(&h.state().session.doc().unwrap().doc, &before), "opening options does not edit the section");
        h.state_mut().ui.dialog = None;
        h.get_by_label("Numbering & Section Options: 5").click_accesskit();
        steps(&mut h, 3);
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some("numberingSection"));
    }

    /// A spread's label is a button that goes to the spread, also from Edit Parents; each page is named for screen readers.
    #[test]
    fn a_spread_label_goes_to_its_spread_and_leaves_edit_parents() {
        let mut app = app_with(9, true);
        app.run("layout.section", json!({"page": 3, "startNumber": 1})).unwrap();
        app.run("layout.parents.edit", json!({"on": true})).unwrap();
        let mut h = open(app, vec2(260.0, 900.0));
        let _ = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some("2, 1"));
        assert_eq!(h.query_all_by_label("Page 1").count(), 2, "two pages are numbered 1");
        if let Some(view) = h.state_mut().view_mut() {
            view.fitted = true;
        }
        click(&mut h, "2–3");
        assert!(!h.state().session.active().unwrap().editing_parents);
        assert!(!h.state().view().unwrap().fitted);
        assert_eq!(document(&h).page_loc(crate::canvas::current_page(h.state()).unwrap()).unwrap().0, 2);
    }

    /// A page is reached by Tab and gone to with Enter, as every control of the framework is.
    #[test]
    fn a_page_is_reached_by_tab_and_gone_to_with_enter() {
        let mut h = open(app_with(4, false), vec2(300.0, 700.0));
        let mut tries = 0;
        while !h.get_by_label("Page 3").accesskit_node().is_focused() {
            key(&mut h, Key::Tab);
            tries += 1;
            assert!(tries < 40, "Tab reaches Page 3");
        }
        key(&mut h, Key::Enter);
        assert_eq!(crate::canvas::current_page(h.state()), Some(2));
        assert!(selected(&h, "Page 3"));
    }
}
