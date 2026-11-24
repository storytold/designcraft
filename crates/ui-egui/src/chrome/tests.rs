/// A Control panel field is labelled by its icon, so assistive technology reads "Tracking"
/// (or "Font Size", …) with the value.
#[test]
fn icon_fields_are_labelled_by_their_icon() {
    use egui::accesskit::Role;
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx, "en");
    ctx.enable_accesskit();
    let mut update = None;
    for _ in 0..2 {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            icon_field(ui, "tracking", "Tracking", NumField::number("tracking_test", Some(0.0), "", 0));
        });
        out.textures_delta.clear();
        update = out.platform_output.accesskit_update;
    }
    let update = update.expect("an AccessKit tree");
    let named = |n: &egui::accesskit::Node| n.label() == Some("Tracking") || n.value() == Some("Tracking");
    let icon = update.nodes.iter().find(|(_, n)| named(n)).map(|(id, _)| *id).expect("the icon's node");
    assert!(update.nodes.iter().any(|(_, n)| n.role() == Role::TextInput && n.labelled_by().contains(&icon)), "the editor names its icon");
}
use egui::{pos2, vec2};
use egui_kittest::kittest::{NodeT, Queryable};

use craft_ui_shell::shell::{CaptionFamily, Captions, Frame, MacInset};

use super::*;
use crate::test_window::{self, wheel};

/// The Control panel's last control with no text frame selected.
const LAST_CONTROL: &str = "Align bottom edges";
const FIRST_CONTROL: &str = "Rotate 90° Counterclockwise";

/// The start screen and Open Recent menu look a recent file up on disk at most every few
/// seconds, not every frame.
#[test]
fn recent_files_are_looked_up_on_disk_every_few_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.designcraft");
    std::fs::write(&path, b"x").unwrap();
    let path = path.to_string_lossy().to_string();
    let ctx = egui::Context::default();
    let first = super::recent_file_stat_at(&ctx, &path, 10_000.0);
    assert!(first.exists && first.modified.is_some());
    std::fs::remove_file(&path).unwrap();
    assert_eq!(super::recent_file_stat_at(&ctx, &path, 10_000.0 + super::RECENT_STAT_MS - 1.0), first, "cached");
    assert!(!super::recent_file_stat_at(&ctx, &path, 10_000.0 + super::RECENT_STAT_MS).exists, "looked up again");
}

fn app() -> crate::DesignApp {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.control_bar = true;
    app
}

/// A document with a rectangle selected, so the object controls are enabled.
fn app_with_selection() -> crate::DesignApp {
    let mut app = app();
    app.run("file.new", serde_json::json!({})).unwrap();
    app.run("frame.create", serde_json::json!({"rect": [72, 72, 300, 200]})).unwrap();
    app.run("edit.selectAll", serde_json::json!({})).unwrap();
    app
}

/// The rect of an object Control panel group.
fn group(h: &egui_kittest::Harness<'static, test_window::Window>, id: &str) -> egui::Rect {
    h.ctx.read_response(egui::Id::unique(("object_control", id))).unwrap().rect
}

/// The rect of the node with this label, preferring the one in the Control bar's row: the
/// Properties panel has buttons with the same names.
fn rect(h: &egui_kittest::Harness<'static, test_window::Window>, label: &str) -> egui::Rect {
    let bar = test_window::panel_rect(h, "control_bar");
    h.query_all_by_label(label).map(|n| n.rect()).find(|r| bar.y_range().contains(r.center().y)).unwrap_or_else(|| h.get_by_label(label).rect())
}

/// The application bar's search field (German interface).
fn search_node<'h>(h: &'h egui_kittest::Harness<'static, test_window::Window>) -> egui_kittest::Node<'h> {
    h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.placeholder() == Some(crate::i18n::tr("de", "Search")))
}

/// A document with two frames, the Selection tool, German menus, and the search field clicked.
fn searching() -> egui_kittest::Harness<'static, test_window::Window> {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "de".into();
    let mut h = test_window::open(app, vec2(1440.0, 900.0));
    let app = &mut h.state_mut().app;
    app.run("frame.create", serde_json::json!({"rect": [36, 36, 200, 200]})).unwrap();
    app.run("frame.create", serde_json::json!({"rect": [236, 36, 400, 200]})).unwrap();
    app.run("edit.deselectAll", serde_json::json!({})).unwrap();
    h.run_steps(4);
    let field = search_node(&h).rect();
    test_window::click_at(&mut h, field.center());
    assert!(craft_ui_widgets::runtime::focus::has(&h.ctx, super::search_id()), "a click gives the field the keyboard");
    h
}

/// A match in the search's list, whose accessible label ends in its shortcut.
fn row<'h>(h: &'h egui_kittest::Harness<'static, test_window::Window>, label: &'static str) -> Option<egui_kittest::Node<'h>> {
    h.query_by(move |n| n.label().is_some_and(|l| l == label || l.starts_with(&format!("{label} "))))
}

fn items(h: &egui_kittest::Harness<'static, test_window::Window>) -> (usize, usize) {
    let d = h.state().app.session.active().unwrap();
    (d.doc.spreads.iter().flat_map(|sp| &sp.items).count(), d.selection.items.len())
}

#[test]
fn the_search_field_finds_and_runs_menu_commands() {
    let mut h = searching();
    let group = crate::i18n::tr("de", "Group");
    h.event(egui::Event::Text(group.to_lowercase()));
    h.run_steps(4);
    assert!(row(&h, group).is_some(), "the German label is listed");
    // Nothing is selected, so Group is off and Return doesn't run it.
    assert!(row(&h, group).is_some_and(|n| n.accesskit_node().is_disabled()));
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(items(&h), (2, 0), "a disabled command doesn't run");
    // Select both frames, then ↓ ↑ Return runs the first match, as Object › Group would.
    h.state_mut().app.run("edit.selectAll", serde_json::json!({})).unwrap();
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(2);
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(items(&h).0, 1, "the two frames are grouped");
    assert!(row(&h, group).is_none(), "the matches closed");
    assert!(!craft_ui_widgets::runtime::focus::has(&h.ctx, super::search_id()), "the field gave the keyboard back");
}

#[test]
fn keys_typed_in_the_search_field_stay_out_of_the_document() {
    let mut h = searching();
    h.state_mut().app.run("edit.selectAll", serde_json::json!({})).unwrap();
    // Escape would switch this tool back to the Selection tool.
    h.state_mut().app.run("tool.select", serde_json::json!({"tool": "rectangleFrame"})).unwrap();
    h.run_steps(2);
    // Tool keys, Delete and Backspace edit the query; the tool and the frames stay.
    for key in [egui::Key::P, egui::Key::T, egui::Key::M, egui::Key::Delete, egui::Key::Backspace] {
        h.key_press(key);
        h.run_steps(2);
    }
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    assert_eq!(h.state().app.session.tool_id(), "rectangleFrame");
    assert_eq!(items(&h), (2, 2), "nothing was deleted or deselected");
    h.event(egui::Event::Text("zz-no-such-command".into()));
    h.run_steps(4);
    assert!(h.query_by_label(crate::i18n::tr("de", "No matching commands")).is_some());
    // Escape closes the matches and clears the field without touching the document.
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(h.query_by_label(crate::i18n::tr("de", "No matching commands")).is_none());
    assert!(!craft_ui_widgets::runtime::focus::has(&h.ctx, super::search_id()), "the field gave the keyboard back");
    assert_eq!(h.state().app.session.tool_id(), "rectangleFrame");
    assert_eq!(items(&h), (2, 2));
    let field = search_node(&h);
    assert_eq!(field.value().unwrap_or_default(), "", "the query is cleared");
}

/// The centre of the status bar's preflight count: the control in the status bar that names Preflight or what it counts (the
/// dock's Preflight icon is a button of the same name above it).
fn preflight_count<S>(h: &egui_kittest::Harness<'static, S>) -> egui::Pos2 {
    let bar = status_bar_rect(h);
    h.query_all_by(|n| {
        n.role() != egui::accesskit::Role::Label && n.label().is_some_and(|l| l == "Preflight" || l.contains("error") || l.contains("Checking"))
    })
    .map(|n| n.rect())
    .filter(|r| bar.contains(r.center()))
    .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
    .expect("the status bar's preflight count")
    .center()
}

/// The status bar's rect.
fn status_bar_rect<S>(h: &egui_kittest::Harness<'static, S>) -> egui::Rect {
    egui::containers::panel::PanelState::load(&h.ctx, h.ui_id().with("status_bar"))
        .map(|panel| panel.outer_rect)
        .expect("the status bar is shown at the bottom of the window")
}

/// The status bar's preflight count is a menu; its Preflight Panel entry shows the panel.
#[test]
fn status_bar_error_count_opens_the_preflight_panel() {
    let mut app = app();
    app.run("file.new", serde_json::json!({})).unwrap();
    app.ui.dock_tab = "properties".into();
    let mut h = test_window::open(app, vec2(1200.0, 700.0));
    let count = preflight_count(&h);
    click_in(&mut h, count);
    h.run_steps(3);
    let entry = h.get_by_label("Preflight Panel…").rect().center();
    click_in(&mut h, entry);
    // Preflight is in the iconic column: `window.panel` shows it.
    assert!(crate::panel_docking::showing(&h.state().app, "preflight"));
    assert!(h.query_all_by_label("Preflight").count() >= 2, "its icon and its tab in the flyout");
    // With the panel already showing, the entry keeps it open.
    let count = preflight_count(&h);
    click_in(&mut h, count);
    h.run_steps(3);
    let entry = h.get_by_label("Preflight Panel…").rect().center();
    click_in(&mut h, entry);
    assert!(crate::panel_docking::showing(&h.state().app, "preflight"));
}

/// The course's chapter 25: while a parent is shown, the status bar's page box names it
/// ("A-Parent"), as InDesign's. Its menu lists the pages, then the parents: a parent shows
/// that parent, a page leaves the parents for that page.
#[test]
fn the_page_box_names_the_parent_in_view_and_lists_the_parents() {
    use egui::accesskit::Role;
    fn page_box<'a>(h: &'a egui_kittest::Harness<'static, test_window::Window>) -> egui_kittest::Node<'a> {
        h.query_all_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Page"))
            .max_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
            .expect("the status bar's page box")
    }
    let mut app = app();
    app.run("file.new", serde_json::json!({"pages": 4, "facingPages": true})).unwrap();
    app.run("layout.parents.new", serde_json::json!({"prefix": "B", "name": "Cover"})).unwrap();
    app.ui.dock_tab = "properties".into();
    let mut h = test_window::open(app, vec2(1200.0, 700.0));
    // The status bar's page box is the lowest select named Page (the Properties panel has one too).
    let shown = |h: &egui_kittest::Harness<'static, test_window::Window>| page_box(h).value().unwrap_or_default();
    assert_eq!(shown(&h), "1");
    h.state_mut().app.run("layout.parents.edit", serde_json::json!({"on": true})).unwrap();
    h.run_steps(6);
    assert_eq!(shown(&h), "A-Parent");
    // The menu: a parent.
    let at = page_box(&h).rect().center();
    click_in(&mut h, at);
    h.run_steps(3);
    for name in ["1", "2", "3", "4", "A-Parent"] {
        // The canvas or a panel may show the name too: the menu lists it.
        assert!(h.query_all_by_label(name).next().is_some(), "{name} is listed");
    }
    // The menu, drawn last, holds the last match.
    let at = h.get_all_by_label("B-Cover").last().unwrap().rect().center();
    click_in(&mut h, at);
    h.run_steps(6);
    assert_eq!(shown(&h), "B-Cover");
    assert_eq!(crate::canvas::current_parent_page(&h.state().app).map(|(i, _)| i), Some(1));
    // A page: the parents are left.
    let at = page_box(&h).rect().center();
    click_in(&mut h, at);
    h.run_steps(3);
    let at = h.get_all_by_label("3").last().unwrap().rect().center();
    click_in(&mut h, at);
    h.run_steps(6);
    assert!(!h.state().app.session.active().unwrap().editing_parents);
    assert_eq!(crate::canvas::current_page(&h.state().app), Some(2));
    assert_eq!(shown(&h), "3");
    // Its accessible name is in the interface's language.
    h.state_mut().app.ui.language = "de".into();
    h.run_steps(3);
    assert!(h.query_all_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Seite")).next().is_some(), "the page box is Seite");
}

/// As in InDesign, double-clicking the preflight count (or its light) shows the panel.
#[test]
fn double_clicking_the_status_bar_error_count_opens_the_preflight_panel() {
    let mut app = app();
    app.run("file.new", serde_json::json!({})).unwrap();
    app.ui.dock_tab = "properties".into();
    let mut h = test_window::open_stepped(app, vec2(1200.0, 700.0), 1.0, 1.0 / 60.0);
    let count = preflight_count(&h);
    click_in(&mut h, count);
    click_in(&mut h, count);
    h.run_steps(3);
    assert!(crate::panel_docking::showing(&h.state().app, "preflight"));
    assert!(h.query_all_by_label("Preflight").count() >= 2, "its icon and its tab in the flyout");
    assert!(h.query_by_label("Preflight Panel…").is_none(), "the menu isn't left open");
    // A double-click with the panel already showing keeps it open (the first click closes
    // its flyout, the double-click opens it again). Past the double-click time, so the
    // clicks are not counted with the ones before.
    h.run_steps(30);
    let count = preflight_count(&h);
    click_in(&mut h, count);
    click_in(&mut h, count);
    h.run_steps(3);
    assert!(crate::panel_docking::showing(&h.state().app, "preflight"));
}

/// The window with the interface the shipping app draws: the framework's document tabs at the top, a new document per name.
fn tabs_window(titles: &[String], active: usize, size: egui::Vec2) -> egui_kittest::Harness<'static, Framed> {
    let mut app = app();
    for title in titles {
        app.run("file.new", serde_json::json!({ "title": title })).unwrap();
    }
    app.run("file.activate", serde_json::json!({"index": active})).unwrap();
    framed_window(app, Frame::Native, size)
}

/// The tab of the document `title`: a tab named by the title, the zoom and the view mode ("Doc 3 @ 100%"); its close button's name is "Close Doc 3".
fn tab_of<S>(h: &egui_kittest::Harness<'static, S>, title: &str) -> Option<egui::Rect> {
    h.query_all_by(|n| n.role() == egui::accesskit::Role::Tab && n.label().is_some_and(|l| l.contains(title))).map(|n| n.rect()).next()
}

/// The names of twelve documents, "Doc 1" to "Doc 12".
fn doc_names(count: usize) -> Vec<String> {
    (1..=count).map(|n| format!("Doc {n}")).collect()
}

/// A very long document name made its tab grow past the window, hiding the close button
/// and the tabs after it (#603).
#[test]
fn long_document_names_keep_tabs_short_and_close_buttons_visible() {
    let latin = "Quarterly report with a very long file name ".repeat(6);
    let hebrew = "דוח רבעוני עם שם קובץ ארוך מאוד ".repeat(6);
    // Use the bundled or pinned test face; optional-input builds without either skip
    // the Hebrew tab explicitly instead of rendering tofu under the strict glyph policy.
    let hebrew_ok = test_window::install_test_hebrew_face();
    if !hebrew_ok {
        eprintln!("skipped the Hebrew tab: set DESIGNCRAFT_TEST_HEBREW_FONT to the pinned NotoSansHebrew.ttf");
    }
    let mut titles = vec![latin.clone()];
    if hebrew_ok {
        titles.push(hebrew.clone());
    }
    titles.push("Short".to_string());
    let h = tabs_window(&titles, 2.min(titles.len() - 1), vec2(1200.0, 600.0));
    let shown: Vec<&String> = titles.iter().take(titles.len() - 1).collect();
    for title in shown {
        let tab = tab_of(&h, title).unwrap_or_else(|| panic!("the tab of {title:?} is shown"));
        assert!(tab.width() <= super::DOC_TAB_MAX_W, "{title:?} tab is {} wide", tab.width());
        let close = h.get_by_label(&format!("Close {title}")).rect();
        assert!(tab.contains_rect(close), "close button {close:?} outside its tab {tab:?}");
    }
    let (first, short) = (tab_of(&h, &latin).unwrap(), tab_of(&h, "Short").expect("the short tab is shown"));
    assert!(short.min.x - first.min.x <= 2.0 * super::DOC_TAB_MAX_W, "the last tab {short:?} starts too far right");
}

/// Tabs past the bar are reached with the overflow menu, which lists every document.
#[test]
fn overflowing_document_tabs_are_reached_through_the_menu_that_lists_every_document() {
    let mut h = tabs_window(&doc_names(30), 0, vec2(1200.0, 600.0));
    h.run_steps(4);
    let window = h.ctx.content_rect();
    // No tab is cut by the bar: each one shown is whole, and the last one is not among them.
    assert!(tab_of(&h, "Doc 30 @").is_none_or(|r| !window.contains_rect(r)), "the last tab is past the bar");
    let tab = tab_of(&h, "Doc 1 @").expect("the first tab is shown");
    assert!(window.contains_rect(tab), "{tab:?} outside {window:?}");
    let more = h.get_by_label("More tabs").rect().center();
    click_in(&mut h, more);
    h.run_steps(3);
    for n in 1..=30 {
        assert!(h.query_all_by_label_contains(&format!("Doc {n} @")).next().is_some(), "Doc {n} is in the list");
    }
    // The chosen document's tab is brought into view.
    let strip = tab.y_range();
    for (n, index) in [(7_usize, 6_usize), (13, 12)] {
        let label = format!("Doc {n} @");
        let entry =
            h.query_all_by_label_contains(&label).find(|node| !strip.contains(node.rect().center().y)).unwrap_or_else(|| panic!("Doc {n} is listed"));
        let at = entry.rect().center();
        click_in(&mut h, at);
        assert_eq!(h.state().app.session.active_index(), Some(index));
        let tab = tab_of(&h, &format!("Doc {n} @")).unwrap_or_else(|| panic!("the chosen document's tab is shown"));
        assert!(window.contains_rect(tab), "{tab:?} outside {window:?}");
        if n == 7 {
            let more = h.get_by_label("More tabs").rect().center();
            click_in(&mut h, more);
            h.run_steps(3);
        }
    }
}

/// A narrower window must not leave the active tab hidden.
#[test]
fn the_active_document_tab_stays_in_view_when_the_window_shrinks() {
    let mut h = tabs_window(&doc_names(12), 11, vec2(1800.0, 600.0));
    h.run_steps(4);
    h.set_size(vec2(900.0, 600.0));
    h.run_steps(6);
    let window = h.ctx.content_rect();
    let tab = tab_of(&h, "Doc 12 @").expect("the active tab is shown");
    let close = h.get_by_label("Close Doc 12").rect();
    assert!(window.contains_rect(tab) && tab.contains_rect(close), "{tab:?} (close {close:?}) outside {window:?}");
}

/// The successor of a closed active tab takes its index, so identity decides what to reveal.
#[test]
fn closing_an_active_tab_scrolled_out_of_view_reveals_its_successor() {
    let mut h = tabs_window(&doc_names(30), 24, vec2(1200.0, 600.0));
    h.run_steps(4);
    let window = h.ctx.content_rect();
    let active = tab_of(&h, "Doc 25 @").expect("the active tab is kept in view past the others");
    assert!(window.contains_rect(active), "{active:?} outside {window:?}");
    crate::menus::close_document(&mut h.state_mut().app, Some(24));
    h.run_steps(6);
    // The next document takes the closed one's index, so only its identity tells them apart.
    assert_eq!(h.state().app.session.active_index(), Some(24));
    let tab = tab_of(&h, "Doc 26 @").expect("the successor's tab is shown");
    assert!(window.contains_rect(tab), "{tab:?} not in view in {window:?}");
}

/// Zero2Graphic's page layout video (0:48): with the Type tool, the Control panel's font size
/// has a list of sizes; the instructor picks 72 pt from it. Leading has the same list.
#[test]
fn the_type_control_panel_lists_font_sizes_and_leadings() {
    use egui::accesskit::Role;
    let mut app = app();
    app.run("file.new", serde_json::json!({})).unwrap();
    let f = app.run("frame.create", serde_json::json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Headline"})).unwrap();
    app.run("text.select", serde_json::json!({"story": f["story"], "anchor": 0, "focus": 8})).unwrap();
    app.session.set_tool("type");
    let mut h = test_window::open(app, vec2(1800.0, 700.0));
    let bar = test_window::panel_rect(&h, "control_bar");
    // The number fields of the Control panel are named by what they set.
    fn field<'h>(h: &'h egui_kittest::Harness<'static, test_window::Window>, bar: egui::Rect, label: &'h str) -> egui_kittest::Node<'h> {
        h.query_all_by_label(label)
            .filter(|n| matches!(n.accesskit_node().role(), egui::accesskit::Role::SpinButton | egui::accesskit::Role::TextInput))
            .find(|n| bar.y_range().contains(n.rect().center().y))
            .unwrap_or_else(|| panic!("no {label:?} field in the Control panel"))
    }
    // The field takes the keyboard as a screen reader gives it: the pointer would move the text selection with it.
    let focus = |h: &mut egui_kittest::Harness<'static, test_window::Window>, label: &str| {
        field(h, bar, label).focus();
        h.run_steps(2);
    };
    // Alt+Down opens a field's list of presets.
    let open_list = |h: &mut egui_kittest::Harness<'static, test_window::Window>, label: &str| {
        focus(h, label);
        h.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowDown);
        h.run_steps(2);
    };
    let type_over = |h: &mut egui_kittest::Harness<'static, test_window::Window>, label: &str, text: &str, key: egui::Key| {
        focus(h, label);
        h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.event(egui::Event::Text(text.into()));
        h.key_press(key);
        h.run_steps(3);
    };
    let sid = designcraft_doc::StoryId(f["story"].as_u64().unwrap());
    let size_of = |h: &egui_kittest::Harness<'static, test_window::Window>| {
        h.state().app.session.doc().unwrap().doc.story(sid).unwrap().format_after(0).over.size
    };
    let leading_of = |h: &egui_kittest::Harness<'static, test_window::Window>| {
        h.state().app.session.doc().unwrap().doc.story(sid).unwrap().format_after(0).over.leading
    };
    // The size's list opens; choosing 72 pt sets the size.
    open_list(&mut h, "Font Size");
    h.query_all_by_label("72 pt").last().expect("the presets list's row").click();
    h.run_steps(2);
    assert_eq!(size_of(&h), Some(72.0));
    // Auto leading has the same list beside it.
    open_list(&mut h, "Leading");
    h.query_all_by_label("60 pt").last().expect("the presets list's row").click();
    h.run_steps(2);
    assert_eq!(leading_of(&h), Some(designcraft_doc::Leading::Points(60.0)));
    // The arrow keys step the size by a point.
    focus(&mut h, "Font Size");
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(3);
    assert_eq!(size_of(&h), Some(73.0));
    // Typing the automatic value itself over it fixes the leading at it (73 pt at 120% is 87.6 pt).
    h.state_mut().app.run("type.char", serde_json::json!({"attrs": {"leading": {"kind": "auto"}}})).unwrap();
    h.run_steps(3);
    type_over(&mut h, "Leading", "87.6", egui::Key::Enter);
    assert_eq!(leading_of(&h), Some(designcraft_doc::Leading::Points(87.6)));
    // Escape cancels: the same typing, then Escape, leaves the leading automatic.
    h.state_mut().app.run("type.char", serde_json::json!({"attrs": {"leading": {"kind": "auto"}}})).unwrap();
    h.run_steps(3);
    let auto = leading_of(&h);
    type_over(&mut h, "Leading", "87.6", egui::Key::Escape);
    assert_eq!(leading_of(&h), auto, "Escape leaves it automatic");
    // Escape also reverts a different number, in the leading field and in a plain one. (It
    // leaves the text too, so each try selects it again.)
    for (label, typed) in [("Leading", "20"), ("Font Size", "50")] {
        h.state_mut().app.run("text.select", serde_json::json!({"story": f["story"], "anchor": 0, "focus": 8})).unwrap();
        h.state_mut().app.session.set_tool("type");
        h.run_steps(3);
        type_over(&mut h, label, typed, egui::Key::Escape);
    }
    assert_eq!(leading_of(&h), auto, "still automatic");
    assert_eq!(size_of(&h), Some(73.0), "the size is unchanged");
}

/// The Control panel's ¶ mode on a First Line Only paragraph: Align to Baseline Grid shows
/// pressed, and a click turns alignment off instead of making it All Lines.
#[test]
fn the_control_panel_grid_button_keeps_first_line_only() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut app = app();
    app.run("file.new", serde_json::json!({})).unwrap();
    let f = app.run("frame.create", serde_json::json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Bulbs"})).unwrap();
    app.run("text.select", serde_json::json!({"story": f["story"], "anchor": 0, "focus": 5})).unwrap();
    app.run("type.para", serde_json::json!({"attrs": {"gridAlign": "firstLineOnly"}})).unwrap();
    app.session.set_tool("type");
    let mut h = test_window::open(app, vec2(2400.0, 700.0));
    let bar = test_window::panel_rect(&h, "control_bar");
    let in_bar = move |r: egui::Rect| bar.y_range().contains(r.center().y);
    let click = |h: &mut egui_kittest::Harness<'static, test_window::Window>, label: &str| {
        let r = h.query_all_by_label(label).map(|n| n.rect()).find(|r| in_bar(*r)).unwrap_or_else(|| panic!("no {label:?} in the Control panel"));
        h.hover_at(r.center());
        h.drag_at(r.center());
        h.drop_at(r.center());
        h.run_steps(3);
    };
    click(&mut h, "Paragraph Formatting Controls");
    let pressed = h.query_all_by_label("Align to Baseline Grid").find(|n| in_bar(n.rect())).map(|n| n.accesskit_node().toggled());
    assert_eq!(pressed, Some(Some(egui::accesskit::Toggled::True)), "a framework icon button shows its on state as pressed");
    assert_eq!(crate::panels::text_attrs(&mut h.state_mut().app).unwrap()["para"]["gridAlign"], "firstLineOnly");
    click(&mut h, "Align to Baseline Grid");
    assert_eq!(crate::panels::text_attrs(&mut h.state_mut().app).unwrap()["para"]["gridAlign"], "none");
}

/// The course's chapter 16: the text Control panel has InDesign's two modes. A shows the
/// Character Formatting Controls (the case buttons, kerning and tracking, the scales…), ¶ the
/// Paragraph Formatting Controls (alignment, indents, spacing, the drop cap…).
#[test]
fn the_type_control_panel_has_character_and_paragraph_modes() {
    let mut app = app();
    app.run("file.new", serde_json::json!({})).unwrap();
    let f = app.run("frame.create", serde_json::json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Bulbs"})).unwrap();
    app.run("text.select", serde_json::json!({"story": f["story"], "anchor": 0, "focus": 5})).unwrap();
    app.session.set_tool("type");
    let mut h = test_window::open(app, vec2(2400.0, 700.0));
    let bar = test_window::panel_rect(&h, "control_bar");
    let in_bar = move |r: egui::Rect| bar.y_range().contains(r.center().y);
    let click = |h: &mut egui_kittest::Harness<'static, test_window::Window>, label: &str| {
        let r = h.query_all_by_label(label).map(|n| n.rect()).find(|r| in_bar(*r)).unwrap_or_else(|| panic!("no {label:?} in the Control panel"));
        h.hover_at(r.center());
        h.drag_at(r.center());
        h.drop_at(r.center());
        h.run_steps(3);
    };
    let has = |h: &egui_kittest::Harness<'static, test_window::Window>, label: &str| h.query_all_by_label(label).any(|n| in_bar(n.rect()));
    assert!(has(&h, "Small Caps") && has(&h, "Vertical Scale") && !has(&h, "Drop Cap Number of Lines"));
    click(&mut h, "Small Caps");
    click(&mut h, "Paragraph Formatting Controls");
    assert!(has(&h, "Drop Cap Number of Lines") && has(&h, "Space Before") && !has(&h, "Small Caps"));
    click(&mut h, "Align to Baseline Grid");
    click(&mut h, "Align center");
    let a = crate::panels::text_attrs(&mut h.state_mut().app).unwrap();
    assert_eq!(a["chars"]["capitalization"], "smallCaps");
    assert_eq!(a["para"]["gridAlign"], "allLines");
    assert_eq!(a["para"]["align"], "center");
    click(&mut h, "Character Formatting Controls");
    assert!(has(&h, "Small Caps"));
}

/// The course's chapter 21: the ¶ Control panel has InDesign's Bulleted and Numbered List
/// buttons (with the default hanging indent), and Shading and Border with their colours and
/// the Paragraph Borders and Shading dialog.
#[test]
fn the_paragraph_control_panel_has_lists_shading_and_border() {
    let mut app = app();
    app.run("file.new", serde_json::json!({})).unwrap();
    let f = app
        .run(
            "frame.create",
            serde_json::json!({"rect": [72, 72, 400, 300], "content": "text", "text": "4 eggs
1 cup milk"}),
        )
        .unwrap();
    app.run("text.select", serde_json::json!({"story": f["story"], "anchor": 0, "focus": 15})).unwrap();
    app.session.set_tool("type");
    let mut h = test_window::open(app, vec2(2400.0, 700.0));
    let bar = test_window::panel_rect(&h, "control_bar");
    let in_bar = move |r: egui::Rect| bar.y_range().contains(r.center().y);
    let click = |h: &mut egui_kittest::Harness<'static, test_window::Window>, label: &str, nth: usize| {
        let r = h
            .query_all_by_label(label)
            .map(|n| n.rect())
            .filter(|r| in_bar(*r))
            .nth(nth)
            .unwrap_or_else(|| panic!("no {label:?} in the Control panel"));
        h.hover_at(r.center());
        h.drag_at(r.center());
        h.drop_at(r.center());
        h.run_steps(3);
    };
    click(&mut h, "Paragraph Formatting Controls", 0);
    click(&mut h, "Bulleted List", 0);
    let a = crate::panels::text_attrs(&mut h.state_mut().app).unwrap();
    assert_eq!(
        (a["para"]["listType"].as_str(), a["para"]["leftIndent"].as_f64(), a["para"]["firstLineIndent"].as_f64()),
        (Some("bullets"), Some(18.0), Some(-18.0))
    );
    click(&mut h, "Numbered List", 0);
    assert_eq!(crate::panels::text_attrs(&mut h.state_mut().app).unwrap()["para"]["listType"], "numbers");
    click(&mut h, "Shading", 0);
    click(&mut h, "Border", 0);
    let a = crate::panels::text_attrs(&mut h.state_mut().app).unwrap();
    assert_eq!((a["para"]["shadingOn"].as_bool(), a["para"]["borderOn"].as_bool()), (Some(true), Some(true)));
    assert!(h.query_all_by_label("Paragraph Shading").any(|n| in_bar(n.rect())), "the shading colour");
    // The icon beside Border opens the dialog on its Border tab.
    click(&mut h, "Paragraph Borders and Shading…", 1);
    let d = h.state().app.ui.dialog.clone().expect("the dialog");
    assert_eq!((d.id.as_str(), d.s("section").as_str()), ("paragraphLocal", "border"));
    // The Last Line Right Indent takes a negative value, as the First Line Left Indent does.
    h.state_mut().app.ui.dialog = None;
    h.state_mut().app.run("text.select", serde_json::json!({"story": f["story"], "anchor": 0, "focus": 15})).unwrap();
    h.state_mut().app.session.set_tool("type");
    h.run_steps(3);
    // The field is labelled by its icon.
    let at = h
        .query_all_by_label("Last Line Right Indent")
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::TextInput)
        .map(|n| n.rect())
        .find(|r| in_bar(*r))
        .expect("the last line indent")
        .center();
    h.hover_at(at);
    h.drag_at(at);
    h.drop_at(at);
    h.run_steps(2);
    h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
    h.event(egui::Event::Text("-0p6".into()));
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(crate::panels::text_attrs(&mut h.state_mut().app).unwrap()["para"]["lastLineIndent"].as_f64(), Some(-6.0));
}

#[test]
fn the_page_tool_has_its_own_control_panel() {
    // Chris Cicchino's Page Tool and Liquid Layout video (1:02): with the Page tool, the
    // Control panel shows the page's size, preset, orientation and liquid page rule.
    let mut app = app();
    app.run("file.new", serde_json::json!({"width": 252, "height": 144, "facingPages": false})).unwrap();
    app.session.set_tool("page");
    let mut h = test_window::open(app, vec2(1800.0, 600.0));
    let _ = h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Liquid Page Rule:");
    // Pages don't move within their spread, so there is no "Objects Move with Page" option, and
    // no "Controlled by Parent" rule until parent pages have rules of their own.
    assert!(h.query_by_label("Objects Move with Page").is_none());
    assert!(!crate::panels::interactive::LIQUID_RULES.iter().any(|(r, _)| *r == designcraft_doc::LiquidRule::ControlledByParent));
    assert_eq!(crate::panels::properties::preset_name(252.0, 144.0), "US Business Card");
    h.get_by_label("Show Parent Page Overlay").click();
    h.run_steps(4);
    assert!(h.state().app.session.doc().unwrap().doc.spreads[0].pages[0].show_parent_overlay);
}

#[test]
fn a_narrow_window_scrolls_the_control_bar() {
    let w = 700.0;
    let mut h = test_window::open(app_with_selection(), vec2(w, 600.0));
    let bar = test_window::panel_rect(&h, "control_bar");
    assert!(rect(&h, LAST_CONTROL).max.x > w, "the controls are wider than the window");
    // A plain mouse wheel scrolls the bar sideways.
    let first = rect(&h, FIRST_CONTROL).center();
    wheel(&mut h, first, vec2(0.0, -3000.0));
    let last = rect(&h, LAST_CONTROL);
    let window = egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(w, 600.0));
    assert!(window.contains_rect(last) && bar.contains_rect(last), "{last:?} outside the bar {bar:?}");
    assert_eq!(test_window::panel_rect(&h, "control_bar").height(), bar.height(), "the bar keeps its height");
    // A popup opened from a scrolled control opens at that control.
    let appearance = group(&h, "appearance");
    let fill = egui::Rect::from_min_size(appearance.min, vec2(30.0, crate::widgets::FIELD_H));
    let chip = pos2(fill.min.x + 9.0, fill.center().y);
    h.hover_at(chip);
    h.drag_at(chip);
    h.drop_at(chip);
    h.run_steps(4);
    let item = rect(&h, "[Black]");
    assert!(item.min.y > fill.max.y && (item.min.x - chip.x).abs() < 120.0, "popup item at {item:?}, control at {chip:?}");
}

/// A window frame to run the application bar in: the runtime's own idea of the frame, so the shell's title bar draws what
/// the platform's would (the caption buttons of an integrated frame, the room for the traffic lights of an inset one).
fn framed_context(frame: Frame) -> egui::Context {
    use craft_host::target::{ShellInfo, WindowChrome, WindowInfo, WindowKind};
    use craft_window_sdk::policy::{Decision, Desktop, DesktopKind};
    let chrome = match &frame {
        Frame::Native => WindowChrome::Native,
        Frame::Inset(_) => WindowChrome::Inset,
        Frame::Integrated(_) => WindowChrome::Integrated,
    };
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(true);
    let desktop = Desktop { kind: DesktopKind::Other, ssd: None, gtk_theme: None, icon_theme: None };
    let spec = craft_ui_widgets::InstallSpec::new(
        ShellInfo::default().window(WindowInfo::new(WindowKind::Main).chrome(chrome)),
        std::sync::Arc::new(craft_window_sdk::system::SystemWindows),
    )
    .frame(Decision { frame, overridden: None, desktop });
    craft_ui_widgets::install(&ctx, spec).expect("the runtime installs on a new context");
    crate::panel_docking::note_runtime(&ctx);
    // The app's own root values, as `DesignApp::root_env` gives the shipping window.
    let mut root = craft_ui_widgets::runtime::env::RootEnv::new();
    root.values.set(craft_ui_widgets::tabs::DocumentTabLook::Shrinking);
    craft_ui_widgets::runtime::frame::set_input(&ctx, craft_ui_widgets::runtime::frame::FrameInput::new(root));
    ctx
}

/// The Windows frame the app draws itself: Minimize, Maximize and Close at the right end.
fn integrated() -> Frame {
    Frame::Integrated(Captions { family: CaptionFamily::Windows, layout: craft_window_sdk::Layout::default() })
}

/// The macOS frame: the traffic lights over the app's bar, in the standard geometry.
fn inset() -> (Frame, MacInset) {
    let lights = MacInset::default();
    (Frame::Inset(lights), lights)
}

/// The whole window, as `test_window::open` runs it, on a context whose window frame is `frame`.
struct Framed {
    app: DesignApp,
    ready: bool,
}

fn framed_window(mut app: DesignApp, frame: Frame, size: egui::Vec2) -> egui_kittest::Harness<'static, Framed> {
    match &frame {
        Frame::Integrated(_) => app.custom_titlebar = true,
        Frame::Inset(_) => app.integrated_titlebar = true,
        Frame::Native => {}
    }
    if app.session.active().is_none() {
        app.run("file.new", serde_json::json!({})).unwrap();
    }
    let mut h =
        egui_kittest::Harness::builder().with_context(framed_context(frame)).with_size(size).with_max_steps(1000).with_step_dt(0.05).build_ui_state(
            |ui, w: &mut Framed| {
                // The builder runs a frame before the texture size can be raised: skip it.
                if !w.ready {
                    return;
                }
                let ctx = ui.ctx().clone();
                w.app.logic(&ctx);
                w.app.ui(ui);
            },
            Framed { app, ready: false },
        );
    h.input_mut().max_texture_side = Some(8192);
    h.state_mut().ready = true;
    h.run_steps(6);
    h
}

/// Whether the window reports itself maximized, as the OS does in its viewport info.
fn set_maximized<S>(h: &mut egui_kittest::Harness<'static, S>, on: bool) {
    h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().maximized = Some(on);
    h.run_steps(2);
}

/// The title bar: the application bar's top panel.
fn title_bar<S>(h: &egui_kittest::Harness<'static, S>) -> egui::Rect {
    egui::containers::panel::PanelState::load(&h.ctx, h.ui_id().with("app_bar"))
        .map(|panel| panel.outer_rect)
        .expect("the title bar is shown at the top of the window")
}

/// The rect of the title bar's control named `label`.
fn in_bar<S>(h: &egui_kittest::Harness<'static, S>, label: &str) -> egui::Rect {
    let bar = title_bar(h);
    h.query_all_by_label(label)
        .map(|n| n.rect())
        .find(|r| bar.y_range().contains(r.center().y))
        .unwrap_or_else(|| panic!("no {label} in the title bar {bar:?}"))
}

#[test]
fn the_mac_app_bar_centres_and_clears_the_traffic_lights() {
    let (frame, lights) = inset();
    let h = framed_window(app(), frame, vec2(1400.0, 900.0));
    // The bar's controls share one centre line (the lights' bar), start after the room the lights take and stay inside the bar.
    let bar = title_bar(&h);
    let home = in_bar(&h, "Home");
    assert!((home.center().y - bar.center().y).abs() <= 0.5, "Home {home:?} is not on the centre line of {bar:?}");
    assert!(home.min.y >= bar.min.y + 2.0 && home.min.x >= bar.min.x + lights.leading, "{home:?} in {bar:?}, lights take {}", lights.leading);
    for label in ["File", "Help", "Share"] {
        let r = in_bar(&h, label);
        assert!((r.center().y - bar.center().y).abs() <= 0.5 && r.max.y <= bar.max.y, "{label} at {r:?} in {bar:?}");
    }
}

fn press<S>(h: &egui_kittest::Harness<'static, S>, pos: egui::Pos2, pressed: bool) {
    h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
}

/// The window commands the app sends over the next `steps` frames.
fn window_commands<S>(h: &mut egui_kittest::Harness<'static, S>, steps: usize) -> Vec<egui::ViewportCommand> {
    let mut out = Vec::new();
    for _ in 0..steps {
        h.step();
        if let Some(v) = h.output().viewport_output.get(&egui::ViewportId::ROOT) {
            out.extend(v.commands.iter().cloned());
        }
    }
    out
}

fn double_click<S>(h: &mut egui_kittest::Harness<'static, S>, pos: egui::Pos2) -> Vec<egui::ViewportCommand> {
    h.hover_at(pos);
    h.step();
    // One press and release a frame, 50 ms apart: closer together than a double-click allows.
    let mut out = Vec::new();
    for pressed in [true, false, true, false] {
        press(h, pos, pressed);
        out.extend(window_commands(h, 1));
    }
    out.extend(window_commands(h, 2));
    out
}

fn drag<S>(h: &mut egui_kittest::Harness<'static, S>, from: egui::Pos2) -> Vec<egui::ViewportCommand> {
    h.hover_at(from);
    h.step();
    press(h, from, true);
    let mut out = window_commands(h, 1);
    for i in 1..=6 {
        h.hover_at(from + vec2(8.0 * i as f32, 3.0));
        out.extend(window_commands(h, 1));
    }
    press(h, from + vec2(48.0, 3.0), false);
    out.extend(window_commands(h, 2));
    out
}

/// A primary click at `pos`, then a few frames.
fn click_in<S>(h: &mut egui_kittest::Harness<'static, S>, pos: egui::Pos2) {
    h.hover_at(pos);
    h.drag_at(pos);
    h.drop_at(pos);
    h.run_steps(4);
}

/// A click: the window commands it sends.
fn click_commands<S>(h: &mut egui_kittest::Harness<'static, S>, pos: egui::Pos2) -> Vec<egui::ViewportCommand> {
    h.hover_at(pos);
    h.step();
    press(h, pos, true);
    let mut out = window_commands(h, 1);
    press(h, pos, false);
    out.extend(window_commands(h, 2));
    out
}

#[test]
fn the_empty_mac_title_bar_zooms_and_moves_the_window() {
    use egui::ViewportCommand::{Maximized, Minimized, StartDrag};
    let (frame, lights) = inset();
    let mut h = framed_window(app(), frame, vec2(1400.0, 900.0));
    let bar = title_bar(&h);
    // Under the centred title, and between the traffic lights and the Home button.
    let empty = [pos2(bar.center().x, bar.center().y), pos2(bar.min.x + lights.leading - 4.0, bar.center().y)];
    for at in empty {
        // Tests read the setting as its default, zoom.
        let cmds = double_click(&mut h, at);
        assert!(cmds.contains(&Maximized(true)), "double-click at {at:?}: {cmds:?}");
        let cmds = drag(&mut h, at);
        assert!(cmds.contains(&StartDrag), "drag from {at:?}: {cmds:?}");
    }
    // The bar's controls keep their clicks and drags.
    for label in ["Home", "Share", "File"] {
        let at = in_bar(&h, label).center();
        let cmds = double_click(&mut h, at);
        assert!(!cmds.iter().any(|c| matches!(c, Maximized(_) | Minimized(_))), "double-click on {label}: {cmds:?}");
        if label == "Home" {
            assert_eq!(h.state().app.ui.status, "Home", "Home took its clicks");
        }
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        let cmds = drag(&mut h, at);
        assert!(!cmds.contains(&StartDrag), "drag from {label}: {cmds:?}");
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
    }
}

#[test]
fn the_mac_window_is_not_movable_on_the_title_bar_controls() {
    let (frame, lights) = inset();
    let mut h = framed_window(app(), frame, vec2(1400.0, 900.0));
    let bar = title_bar(&h);
    let search = h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.placeholder() == Some("Search")).rect();
    let controls = [in_bar(&h, "Home").center(), in_bar(&h, "Share").center(), in_bar(&h, "File").center(), search.center()];
    // The OS moves the window (macOS `NSWindow.movable`) only where a press would go to the bar's free band, as the shell
    // writes it each pass: a press on a control never moves it.
    let mut movable_at = |at: egui::Pos2| {
        h.hover_at(at);
        h.run_steps(2);
        craft_ui_shell::shell::title_bar::window_movable(&h.ctx)
    };
    for at in [pos2(bar.center().x, bar.center().y), pos2(bar.min.x + lights.leading - 4.0, bar.center().y)] {
        assert!(movable_at(at), "empty title bar at {at:?}");
    }
    for at in controls {
        assert!(!movable_at(at), "control at {at:?}");
    }
}

#[test]
fn a_narrow_window_scrolls_the_app_bar() {
    // The framework's menu bar keeps the titles that fit and puts the rest into a trailing "…" menu, so in a narrow
    // window every menu is still reached and the workspace controls stay in view and clear of the menus.
    let w = 640.0;
    let mut h = test_window::open(app(), vec2(w, 600.0));
    let window = egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(w, 600.0));
    let share = rect(&h, "Share");
    assert!(window.contains_rect(share), "{share:?} outside the window");
    let help = h.query_all_by_label("Help").map(|n| n.rect()).next();
    let menus_end = match help {
        Some(help) => help,
        None => {
            // Help does not fit: it is in the overflow menu, which is the last title of the bar.
            let more = h.query_all_by_label("…").map(|n| n.rect()).next().expect("the menus that do not fit are reached through the overflow title");
            assert!(more.max.x <= share.min.x, "the overflow title ({more:?}) and the workspace controls ({share:?}) overlap");
            test_window::click_at(&mut h, more.center());
            h.run_steps(3);
            assert!(h.query_all_by_label("Help").next().is_some(), "Help is listed in the overflow menu");
            more
        }
    };
    assert!(menus_end.max.x <= share.min.x, "the menus ({menus_end:?}) and the workspace controls ({share:?}) overlap");
}

#[test]
fn a_wide_window_does_not_scroll_the_bars() {
    let mut h = test_window::open(app(), vec2(1400.0, 900.0));
    let before = [rect(&h, FIRST_CONTROL), rect(&h, LAST_CONTROL), rect(&h, "Help"), rect(&h, "Share")];
    assert!(before.iter().all(|r| r.max.x <= 1400.0), "{before:?}");
    wheel(&mut h, before[0].center(), vec2(0.0, -3000.0));
    wheel(&mut h, before[2].center(), vec2(0.0, -3000.0));
    let after = [rect(&h, FIRST_CONTROL), rect(&h, LAST_CONTROL), rect(&h, "Help"), rect(&h, "Share")];
    assert_eq!(after, before);
}

/// At 115% the app bar fits a wide window exactly: it must not scroll sideways by a rounding error.
#[test]
fn the_app_bar_does_not_scroll_at_115_percent() {
    let w = 1400.0;
    let mut h = test_window::open_scaled(app(), vec2(w, 900.0), 1.15);
    assert_eq!(h.ctx.pixels_per_point(), 1.15);
    let before = [rect(&h, "Help"), rect(&h, "Share")];
    assert!(before.iter().all(|r| r.max.x <= w), "{before:?}");
    wheel(&mut h, before[0].center(), vec2(0.0, -3000.0));
    wheel(&mut h, before[0].center(), vec2(-3000.0, 0.0));
    let after = [rect(&h, "Help"), rect(&h, "Share")];
    assert_eq!(after, before, "the app bar scrolled sideways although everything fits");
}

#[test]
fn a_scrolled_bar_keeps_its_captions_on_one_line() {
    let mut app = app();
    app.run("file.new", serde_json::json!({})).unwrap();
    let f = app.run("frame.create", serde_json::json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hello"})).unwrap();
    app.run("selection.set", serde_json::json!({"ids": [f["id"]]})).unwrap();
    let mut h = test_window::open(app, vec2(700.0, 600.0));
    let bar = test_window::panel_rect(&h, "control_bar").center();
    wheel(&mut h, bar, vec2(0.0, -3000.0));
    // The text frame's last group keeps its full size, not only what is left of the row.
    let columns = group(&h, "columns");
    assert!(columns.width() >= 48.0 && columns.max.x <= 700.0, "Columns group squeezed or off screen: {columns:?}");
}

/// The Windows window the app draws the frame of: the shell's title bar with its caption buttons.
fn integrated_window(size: egui::Vec2) -> egui_kittest::Harness<'static, Framed> {
    framed_window(app(), integrated(), size)
}

#[test]
fn caption_buttons_minimize_maximize_restore_and_close() {
    use egui::ViewportCommand::{Close, Maximized, Minimized};
    let mut h = integrated_window(vec2(1440.0, 900.0));
    for (label, command) in [("Minimize", Minimized(true)), ("Maximize", Maximized(true))] {
        let p = in_bar(&h, label).center();
        assert!(click_commands(&mut h, p).contains(&command), "{label}");
    }
    set_maximized(&mut h, true);
    let p = in_bar(&h, "Restore").center();
    assert!(click_commands(&mut h, p).contains(&Maximized(false)));
    let p = in_bar(&h, "Close").center();
    assert!(click_commands(&mut h, p).contains(&Close));
}

#[test]
fn title_area_drags_and_double_clicks_but_menus_do_not_drag() {
    use egui::ViewportCommand::{Maximized, StartDrag};
    let band = |h: &egui_kittest::Harness<'static, Framed>| {
        let bar = title_bar(h);
        pos2(bar.center().x, bar.center().y)
    };
    let mut h = integrated_window(vec2(1440.0, 900.0));
    let p = band(&h);
    assert!(drag(&mut h, p).contains(&StartDrag), "a drag from the free band moves the window");

    let mut h = integrated_window(vec2(1440.0, 900.0));
    let p = band(&h);
    assert!(double_click(&mut h, p).contains(&Maximized(true)), "a double click on the free band maximizes");

    let mut h = integrated_window(vec2(1440.0, 900.0));
    let file = in_bar(&h, "File").center();
    assert!(!drag(&mut h, file).contains(&StartDrag), "a menu title pressed and dragged doesn't move the window");
}

#[test]
fn edges_resize_only_when_the_window_is_restored() {
    use egui::ViewportCommand::BeginResize;
    let mut h = integrated_window(vec2(1440.0, 900.0));
    let p = pos2(1438.0, 400.0);
    h.hover_at(p);
    h.step();
    press(&h, p, true);
    let mut commands = window_commands(&mut h, 1);
    h.hover_at(p + vec2(-20.0, 0.0));
    commands.extend(window_commands(&mut h, 2));
    assert!(commands.contains(&BeginResize(egui::ResizeDirection::East)), "{commands:?}");
    press(&h, p + vec2(-20.0, 0.0), false);
    h.run_steps(2);

    let mut h = integrated_window(vec2(1440.0, 900.0));
    set_maximized(&mut h, true);
    h.hover_at(p);
    h.step();
    press(&h, p, true);
    let mut commands = window_commands(&mut h, 1);
    h.hover_at(p + vec2(-20.0, 0.0));
    commands.extend(window_commands(&mut h, 2));
    assert!(!commands.iter().any(|c| matches!(c, BeginResize(_))), "{commands:?}");
}

#[test]
fn native_window_actions_are_unavailable_without_custom_chrome() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    assert!(!crate::menus::enabled(&app, "window.close"));
    assert!(app.run("window.close", json!({})).is_err());
    assert!(app.pending_window_commands.is_empty());
    app.custom_titlebar = true;
    assert!(app.run("window.resize", json!({"edge": "invalid"})).is_err());
    assert!(app.pending_window_commands.is_empty());
}

#[test]
fn caption_buttons_stay_put_while_a_narrow_app_bar_scrolls() {
    use egui::ViewportCommand::Close;
    let mut h = integrated_window(vec2(640.0, 560.0));
    let close = in_bar(&h, "Close");
    assert!(close.left() >= 0.0 && close.right() <= 640.0, "{close:?}");
    // The wheel over the bar moves what the bar can scroll, never the caption buttons.
    let p = pos2(200.0, title_bar(&h).center().y);
    h.hover_at(p);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -3000.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
        source: egui::MouseWheelSource::Unknown,
    });
    h.run_steps(10);
    assert_eq!(in_bar(&h, "Close"), close, "the caption buttons did not move");
    assert!(click_commands(&mut h, close.center()).contains(&Close));
}

#[test]
fn the_window_drags_from_the_title_bar_at_the_minimum_width() {
    use egui::ViewportCommand::StartDrag;
    // At 900 px the menus and tools fill the bar's row; the bar's top edge, over its controls, still moves the window,
    // and the free band is not drawn over the caption buttons.
    let mut h = integrated_window(vec2(900.0, 560.0));
    let bar = title_bar(&h);
    let close = in_bar(&h, "Close");
    assert!(bar.max.x >= close.right() - 0.5, "{bar:?} {close:?}");
    let p = pos2(450.0, bar.min.y + 1.0);
    assert!(drag(&mut h, p).contains(&StartDrag), "a drag from the bar's top edge at {p:?}");
    assert!(!drag(&mut h, close.center()).contains(&StartDrag), "a drag from the Close button moves nothing");
}

#[test]
fn caption_buttons_fit_the_minimum_window_width() {
    let mut h = integrated_window(vec2(900.0, 560.0));
    for label in ["Minimize", "Maximize", "Close"] {
        let rect = in_bar(&h, label);
        assert!(rect.left() >= 0.0 && rect.right() <= 900.0, "{label}: {rect:?}");
        assert!(!click_commands(&mut h, rect.center()).is_empty(), "{label} remains clickable");
    }
}

#[test]
fn parent_resize_properties_keep_size_and_inline_parent_margins() {
    parent_resize_panel(true);
}

#[test]
fn parent_resize_control_changes_the_parent_not_document_page_zero() {
    parent_resize_panel(false);
}

fn parent_resize_panel(properties: bool) {
    {
        let mut app = app();
        app.run("file.new", serde_json::json!({"width": 400, "height": 600, "pages": 3, "facingPages": true})).unwrap();
        app.run("layout.parents.new", serde_json::json!({"prefix": "B"})).unwrap();
        app.run("layout.pages.applyParent", serde_json::json!({"pages": [0], "parent": "A"})).unwrap();
        if properties {
            app.run("layout.marginsAndColumns", json!({"parent": 1, "pages": [0], "margins": 24})).unwrap();
            app.run(
                "layout.marginsAndColumns",
                json!({"parent": 1, "pages": [1], "margins": {"top": 36, "bottom": 48, "inside": 60, "outside": 72}}),
            )
            .unwrap();
            app.run("layout.pages.applyParent", json!({"pages": [0, 1, 2], "parent": "B"})).unwrap();
            app.run("layout.marginsAndColumns", json!({"pages": [0], "margins": 90})).unwrap();
        }
        app.run("layout.parents.edit", serde_json::json!({"on": true, "parent": "A"})).unwrap();
        app.session.set_tool("page");
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0)));
        *app.view_mut().unwrap() = crate::View { zoom: 1.0, origin: designcraft_geom::Point::new(-600.0, 0.0), fitted: true, rotation: 0 };
        let before = app.session.doc().unwrap().doc.clone();
        let mut h = test_window::builder().with_size(vec2(1600.0, 900.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("parent_resize_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                if properties {
                    panels::properties::show(app, ui);
                } else {
                    super::control_bar::control_page(app, ui);
                }
            },
            app,
        );
        h.run_steps(3);
        edit_parent_resize_field(&mut h, "33p4", "500 pt");
        let d = &h.state().session.doc().unwrap().doc;
        assert_eq!(d.parents[0].pages[0].width, 500.0, "the parent page in view must change");
        assert_eq!(d.spreads, before.spreads);
        assert_eq!(d.parents[1], before.parents[1]);
        let b = before.parents[1].id.0;
        h.state_mut().run("layout.selectParentPage", json!({"parent": b, "page": 1})).unwrap();
        h.run_steps(3);
        assert_eq!(crate::canvas::current_parent_page(h.state()), Some((1, 1)));
        if properties {
            h.state_mut().session.set_tool("selection");
        }
        h.run_steps(3);
        edit_parent_resize_field(&mut h, "50p0", "700 pt");
        edit_parent_resize_field(&mut h, "33p4", "560 pt");
        let d = &h.state().session.doc().unwrap().doc;
        assert_eq!((d.parents[1].pages[1].width, d.parents[1].pages[1].height), (560.0, 700.0));
        assert_eq!(d.parents[1].pages[0].width, 400.0);
        assert_eq!(d.parents[0].pages[0].width, 500.0);
        assert_eq!(d.spreads, before.spreads);
        let parents = d.parents.clone();
        if properties {
            let saved = d.clone();
            check_parent_margin_edits(&mut h);
            h.get_by_label("Back").click();
            h.run_steps(3);
            assert!(!h.state().session.doc().unwrap().editing_parents, "Back returns to document properties");
            assert_eq!(h.state().session.tool_id(), "selection");
            assert!(h.query_by_label("Back").is_none(), "the next frame must leave the page properties screen");
            assert!(std::sync::Arc::ptr_eq(&saved, &h.state().session.doc().unwrap().doc));
        }
        h.state_mut().run("layout.parents.edit", json!({"on": false})).unwrap();
        h.state_mut().session.set_tool("page");
        h.state_mut().run("layout.selectPage", json!({"page": 1})).unwrap();
        h.run_steps(3);
        edit_parent_resize_field(&mut h, "33p4", "530 pt");
        let d = &h.state().session.doc().unwrap().doc;
        assert_eq!(d.spreads[0].pages[0].width, 530.0);
        assert_eq!(d.parents, parents);
        if properties {
            let saved = d.clone();
            h.get_by_label("Back").click();
            h.run_steps(3);
            assert_eq!(h.state().session.tool_id(), "selection");
            assert!(h.query_by_label("Back").is_none());
            assert_eq!(h.state().session.doc().unwrap().selection.page, None);
            assert!(std::sync::Arc::ptr_eq(&saved, &h.state().session.doc().unwrap().doc));
        }
    }
}

fn check_parent_margin_edits(h: &mut egui_kittest::Harness<'_, DesignApp>) {
    let saved = h.state().session.doc().unwrap().doc.clone();
    assert_eq!(h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Top").value().as_deref(), Some("3p0"));
    edit_parent_resize_field(h, "3p0", "72 pt");
    let edited = &h.state().session.doc().unwrap().doc;
    assert_eq!(edited.parents[1].pages[1].margins, designcraft_doc::Margins::uniform(72.0));
    assert_eq!(edited.parents[1].pages[0], saved.parents[1].pages[0]);
    assert_eq!(edited.parents[0], saved.parents[0]);
    assert_eq!(edited.page(0), saved.page(0), "local document margins stay unchanged");
    assert_eq!(edited.page(1), saved.page(1), "the other parent page's inheritor stays unchanged");
    assert_eq!(edited.page(2).unwrap().margins, designcraft_doc::Margins::uniform(72.0));
    assert_eq!(edited.settings, saved.settings);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().unwrap().doc, saved);
    h.get_by_label("Make all settings the same").click();
    h.run_steps(3);
    edit_parent_resize_field(h, "3p0", "84 pt");
    let edited = &h.state().session.doc().unwrap().doc;
    let margins = designcraft_doc::Margins { top: 84.0, bottom: 48.0, inside: 60.0, outside: 72.0 };
    assert_eq!(edited.parents[1].pages[1].margins, margins);
    assert_eq!(edited.parents[1].pages[0], saved.parents[1].pages[0]);
    assert_eq!(edited.parents[0], saved.parents[0]);
    assert_eq!(edited.page(0), saved.page(0));
    assert_eq!(edited.page(1), saved.page(1));
    assert_eq!(edited.page(2).unwrap().margins, margins);
    assert_eq!(edited.settings, saved.settings);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().unwrap().doc, saved);
}

fn edit_parent_resize_field(h: &mut egui_kittest::Harness<'_, DesignApp>, value: &str, input: &str) {
    h.query_all_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some(value)).next().expect("page size field").click();
    h.run_steps(2);
    h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
    h.event(egui::Event::Text(input.into()));
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
}
