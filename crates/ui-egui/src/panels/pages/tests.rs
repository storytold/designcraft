use egui::{Event, Modifiers, PointerButton};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

use super::*;
use crate::test_window;
use egui::vec2;

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
        assert_ne!(h.get_by_label(name).accesskit_node().is_selected(), Some(true), "{name} is not selected yet");
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
        assert_eq!(h.get_by_label(name).accesskit_node().is_selected() == Some(true), name == "C-Parent", "{name}");
    }
    // Parent rows show a rendered thumbnail (of the revision before the double click until the
    // document has been unchanged for THUMB_QUIET_SECS).
    let id = st.doc.parents[2].id;
    let thumbs = super::THUMBS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(thumbs.as_ref().unwrap().keys().any(|k| k.0 == st.uid && k.1 == super::ThumbOf::Parent(id)));
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

/// The panel on the installed framework, driven by real pointer and key events through the app's own input hook.
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
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0)));
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
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

    /// The panel menu follows InDesign's order.
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

    /// Switching to a document whose thumbnails are all current stores that document in the clock, so the next frames do not
    /// scan the whole cache again.
    #[test]
    fn switching_to_a_cached_document_stores_it_in_the_clock() {
        let app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        let mut h = open(app, vec2(280.0, 520.0));
        for _ in 0..2 {
            h.state_mut().run("file.new", json!({"pages": 2, "facingPages": false})).unwrap();
            steps(&mut h, 4);
        }
        h.state_mut().session.set_active(0);
        let st = h.state().session.active().unwrap();
        let first = (st.uid, st.revision);
        let before = super::super::THUMB_RENDERS.with(std::cell::Cell::get);
        steps(&mut h, 1);
        assert_eq!(super::super::THUMB_RENDERS.with(std::cell::Cell::get), before, "the first document's thumbnails are current");
        let clock = h.ctx.data(|d| d.get_temp::<super::super::ThumbClock>(egui::Id::unique("pages_thumb_clock"))).unwrap();
        assert_eq!(clock.doc, first);
    }

    /// The page menu names the selected parent in its parent commands, or lists the parents as submenus when none is selected.
    #[test]
    fn the_panel_menu_names_the_selected_parent_or_lists_the_parents() {
        let mut h = open(app_with(2, false), vec2(320.0, 700.0));
        right_click(&mut h, "Page 1");
        assert!(h.query_by_label("Parent Options").is_some(), "no parent selected: a submenu of the parents");
        let mut app = app_with(2, false);
        let first = app.session.doc().unwrap().doc.parents[0].id;
        select_parent(&mut app, Some(first));
        let mut h = open(app, vec2(320.0, 700.0));
        right_click(&mut h, "Page 1");
        assert!(h.query_by_label("Delete Parent “A-Parent”").is_some());
        click(&mut h, "Duplicate Parent Spread “A-Parent”");
        assert_eq!(document(&h).parents.len(), 2);
        // Duplicating selected the copy, so the menu now names it.
        right_click(&mut h, "Page 1");
        assert!(h.query_by_label("Parent Options “B-Parent”…").is_some());
        click(&mut h, "Parent Options “B-Parent”…");
        assert!(h.state().ui.dialog.as_ref().is_some_and(|d| d.id == "parentOptions" && d.s("prefix") == "B"));
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

    /// The pages area is the framework's scroll region: the wheel over it scrolls the spreads, and pages out of view are
    /// not built until they scroll in.
    #[test]
    fn the_pages_area_scrolls_with_the_wheel() {
        let mut h = open(app_with(60, false), vec2(280.0, 520.0));
        assert!(h.query_by_label("Page 50").is_none(), "Page 50 starts out of view");
        let over = pos_of(&h, "Page 1");
        event(&mut h, Event::PointerMoved(over), Modifiers::NONE);
        // Wheel in small notches until Page 50 comes into view: how far one notch goes is the scroll region's, so the
        // test does not depend on it, only on the wheel bringing later pages in.
        let mut notches = 0;
        while h.query_by_label("Page 50").is_none() && notches < 600 {
            event(
                &mut h,
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, -40.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                    source: egui::MouseWheelSource::Wheel,
                },
                Modifiers::NONE,
            );
            notches += 1;
        }
        assert!(h.query_by_label("Page 50").is_some(), "Page 50 scrolled into view");
        assert!(h.query_by_label("Page 1").is_none(), "Page 1 scrolled out of it");
    }

    /// The area follows the current page: going to a page far down brings its spread into view, once.
    #[test]
    fn the_pages_area_follows_the_current_page() {
        let mut h = open(app_with(60, false), vec2(280.0, 520.0));
        assert!(h.query_by_label("Page 45").is_none(), "Page 45 starts out of view");
        let canvas = h.state().canvas_rect;
        assert!(canvas.is_some());
        crate::canvas::go_to_page(h.state_mut(), 44);
        steps(&mut h, 12);
        let page = h.query_by_label("Page 45").expect("the current page's spread was scrolled into view").rect();
        assert!(page.min.y >= 0.0 && page.max.y <= 520.0, "the page lies inside the panel: {page:?}");
    }
}
