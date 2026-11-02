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
    assert!(h.ctx.memory(|m| m.has_focus(super::search_id())), "a click gives the field the keyboard");
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
    assert!(!h.ctx.memory(|m| m.has_focus(super::search_id())), "the field gave the keyboard back");
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
    assert!(!h.ctx.text_edit_focused());
    assert_eq!(h.state().app.session.tool_id(), "rectangleFrame");
    assert_eq!(items(&h), (2, 2));
    let field = search_node(&h);
    assert_eq!(field.value().unwrap_or_default(), "", "the query is cleared");
}

/// The status bar's preflight count is a menu; its Preflight Panel entry shows the panel.
#[test]
fn status_bar_error_count_opens_the_preflight_panel() {
    let mut app = app();
    app.run("file.new", serde_json::json!({})).unwrap();
    app.ui.dock_tab = "properties".into();
    let mut h = test_window::open(app, vec2(1200.0, 700.0));
    h.get_by_label("No errors ▾").click();
    h.run_steps(3);
    h.get_by_label("Preflight Panel…").click();
    h.run_steps(3);
    // Preflight is in the iconic column: `window.panel` shows it.
    assert!(crate::panel_docking::showing(&h.state().app, "preflight"));
    assert!(h.query_all_by_label("Preflight").count() >= 2, "its icon and its tab in the flyout");
    // With the panel already showing, the entry keeps it open.
    h.get_by_label("No errors ▾").click();
    h.run_steps(3);
    h.get_by_label("Preflight Panel…").click();
    h.run_steps(3);
    assert!(crate::panel_docking::showing(&h.state().app, "preflight"));
}

/// The course's chapter 25: while a parent is shown, the status bar's page box names it
/// ("A-Parent"), as InDesign's. Its menu lists the pages, then the parents: a parent shows
/// that parent, a page leaves the parents for that page.
#[test]
fn the_page_box_names_the_parent_in_view_and_lists_the_parents() {
    use egui::accesskit::Role;
    let mut app = app();
    app.run("file.new", serde_json::json!({"pages": 4, "facingPages": true})).unwrap();
    app.run("layout.parents.new", serde_json::json!({"prefix": "B", "name": "Cover"})).unwrap();
    app.ui.dock_tab = "properties".into();
    let mut h = test_window::open(app, vec2(1200.0, 700.0));
    let shown = |h: &egui_kittest::Harness<'static, test_window::Window>| {
        let b = h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Page"));
        b.value().unwrap_or_default()
    };
    assert_eq!(shown(&h), "1");
    h.state_mut().app.run("layout.parents.edit", serde_json::json!({"on": true})).unwrap();
    h.run_steps(6);
    assert_eq!(shown(&h), "A-Parent");
    // The menu: a parent.
    h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Page")).click();
    h.run_steps(3);
    for name in ["1", "2", "3", "4", "A-Parent"] {
        // The canvas or a panel may show the name too: the menu lists it.
        assert!(h.query_all_by_label(name).next().is_some(), "{name} is listed");
    }
    // The menu, drawn last, holds the last match.
    h.get_all_by_label("B-Cover").last().unwrap().click();
    h.run_steps(6);
    assert_eq!(shown(&h), "B-Cover");
    assert_eq!(crate::canvas::current_parent_page(&h.state().app).map(|(i, _)| i), Some(1));
    // A page: the parents are left.
    h.get_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Page")).click();
    h.run_steps(3);
    h.get_all_by_label("3").last().unwrap().click();
    h.run_steps(6);
    assert!(!h.state().app.session.active().unwrap().editing_parents);
    assert_eq!(crate::canvas::current_page(&h.state().app), Some(2));
    assert_eq!(shown(&h), "3");
    // Its accessible name is in the interface's language.
    h.state_mut().app.ui.language = "de".into();
    h.run_steps(3);
    assert!(h.query_by(|n| n.role() == Role::ComboBox && n.label().as_deref() == Some("Seite")).is_some(), "the page box is Seite");
}

/// As in InDesign, double-clicking the preflight count (or its light) shows the panel.
#[test]
fn double_clicking_the_status_bar_error_count_opens_the_preflight_panel() {
    let mut app = app();
    app.run("file.new", serde_json::json!({})).unwrap();
    app.ui.dock_tab = "properties".into();
    let mut h = test_window::open_stepped(app, vec2(1200.0, 700.0), 1.0, 1.0 / 60.0);
    let count = h.get_by_label("No errors ▾");
    count.click();
    count.click();
    h.run_steps(3);
    assert!(crate::panel_docking::showing(&h.state().app, "preflight"));
    assert!(h.query_all_by_label("Preflight").count() >= 2, "its icon and its tab in the flyout");
    assert!(h.query_by_label("Preflight Panel…").is_none(), "the menu isn't left open");
    // A double-click with the panel already showing keeps it open (the first click closes
    // its flyout, the double-click opens it again). Past the double-click time, so the
    // clicks are not counted with the ones before.
    h.run_steps(30);
    let count = h.get_by_label("No errors ▾");
    count.click();
    count.click();
    h.run_steps(3);
    assert!(crate::panel_docking::showing(&h.state().app, "preflight"));
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
    let mut app = app();
    app.run("file.new", serde_json::json!({ "title": latin })).unwrap();
    if hebrew_ok {
        app.run("file.new", serde_json::json!({ "title": hebrew })).unwrap();
    }
    app.run("file.new", serde_json::json!({ "title": "Short" })).unwrap();
    let h = test_window::open(app, vec2(1200.0, 600.0));
    let titles = if hebrew_ok { vec![latin.as_str(), hebrew.as_str()] } else { vec![latin.as_str()] };
    for title in titles {
        let tab = rect(&h, title);
        assert!(tab.width() <= super::DOC_TAB_MAX_W, "{title:?} tab is {} wide", tab.width());
        let close = rect(&h, &format!("Close {title}"));
        assert!(tab.contains_rect(close), "close button {close:?} outside its tab {tab:?}");
    }
    let (first, short) = (rect(&h, &latin), rect(&h, "Short"));
    assert!(short.min.x - first.min.x <= 2.0 * super::DOC_TAB_MAX_W, "the last tab {short:?} starts too far right");
}

/// Zero2Graphic's page layout video (0:48): with the Type tool, the Control panel's font size
/// has a list of sizes; the instructor picks 72 pt from it. Leading has the same list.
#[test]
fn the_type_control_panel_lists_font_sizes_and_leadings() {
    let mut app = app();
    app.run("file.new", serde_json::json!({})).unwrap();
    let f = app.run("frame.create", serde_json::json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Headline"})).unwrap();
    app.run("text.select", serde_json::json!({"story": f["story"], "anchor": 0, "focus": 8})).unwrap();
    app.session.set_tool("type");
    let mut h = test_window::open(app, vec2(1800.0, 700.0));
    let bar = test_window::panel_rect(&h, "control_bar");
    let in_bar = |r: &egui::Rect| bar.y_range().contains(r.center().y);
    let field = |h: &egui_kittest::Harness<'static, test_window::Window>, value: &str| {
        h.query_all_by_value(value).map(|n| n.rect()).find(in_bar).unwrap_or_else(|| panic!("no {value:?} field in the Control panel"))
    };
    let press = |h: &mut egui_kittest::Harness<'static, test_window::Window>, p: egui::Pos2| {
        h.hover_at(p);
        h.drag_at(p);
        h.drop_at(p);
    };
    let size_of = |h: &egui_kittest::Harness<'static, test_window::Window>| {
        let st = h.state().app.session.doc().unwrap();
        let sid = designcraft_doc::StoryId(f["story"].as_u64().unwrap());
        st.doc.story(sid).unwrap().format_after(0).over.size
    };
    // The ⌄ segment right of the size field opens the list; choosing 72 pt sets the size.
    let size = field(&h, "12 pt");
    let chevron = pos2(size.max.x + 1.0 + crate::widgets::SEG_W / 2.0, size.center().y);
    press(&mut h, chevron);
    h.run_steps(3);
    h.get_by_label("72 pt").click();
    h.run_steps(3);
    assert_eq!(size_of(&h), Some(72.0));
    // Auto leading shows in parentheses, with the same list beside it.
    let lead = field(&h, "(86.4 pt)");
    press(&mut h, pos2(lead.max.x + 1.0 + crate::widgets::SEG_W / 2.0, lead.center().y));
    h.run_steps(3);
    h.get_by_label("60 pt").click();
    h.run_steps(3);
    let st = h.state().app.session.doc().unwrap();
    let sid = designcraft_doc::StoryId(f["story"].as_u64().unwrap());
    assert_eq!(st.doc.story(sid).unwrap().format_after(0).over.leading, Some(designcraft_doc::Leading::Points(60.0)));
    // The spinner left of the size steps it by a point.
    let size = field(&h, "72 pt");
    press(&mut h, pos2(size.min.x - 5.0 - crate::widgets::SEG_W / 2.0, size.min.y + 3.0));
    h.run_steps(3);
    assert_eq!(size_of(&h), Some(73.0));
    // Typing the automatic value itself over "(87.6 pt)" fixes the leading at it.
    h.state_mut().app.run("type.char", serde_json::json!({"attrs": {"leading": {"kind": "auto"}}})).unwrap();
    h.run_steps(3);
    let lead = field(&h, "(87.6 pt)");
    press(&mut h, lead.center());
    h.run_steps(2);
    h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
    h.event(egui::Event::Text("87.6".into()));
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let st = h.state().app.session.doc().unwrap();
    let sid = designcraft_doc::StoryId(f["story"].as_u64().unwrap());
    assert_eq!(st.doc.story(sid).unwrap().format_after(0).over.leading, Some(designcraft_doc::Leading::Points(87.6)));
    // Escape cancels: the same typing, then Escape, leaves the leading automatic.
    h.state_mut().app.run("type.char", serde_json::json!({"attrs": {"leading": {"kind": "auto"}}})).unwrap();
    h.run_steps(3);
    let auto = h.state().app.session.doc().unwrap().doc.story(sid).unwrap().format_after(0).over.leading;
    let lead = field(&h, "(87.6 pt)");
    press(&mut h, lead.center());
    h.run_steps(2);
    h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
    h.event(egui::Event::Text("87.6".into()));
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert_eq!(h.state().app.session.doc().unwrap().doc.story(sid).unwrap().format_after(0).over.leading, auto, "Escape leaves it automatic");
    // Escape also reverts a different number, in the leading field and in a plain one. (It
    // leaves the text too, so each try selects it again.)
    for (shown, typed) in [("(87.6 pt)", "20"), ("73 pt", "50")] {
        h.state_mut().app.run("text.select", serde_json::json!({"story": f["story"], "anchor": 0, "focus": 8})).unwrap();
        h.state_mut().app.session.set_tool("type");
        h.run_steps(3);
        let r = field(&h, shown);
        press(&mut h, r.center());
        h.run_steps(2);
        h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.event(egui::Event::Text(typed.into()));
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
    }
    assert_eq!(h.state().app.session.doc().unwrap().doc.story(sid).unwrap().format_after(0).over.leading, auto, "still automatic");
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
    assert_eq!(pressed, Some(Some(egui::accesskit::Toggled::True)));
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
    let _ = h.get_by_label("Liquid Page Rule:");
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

#[test]
fn the_mac_app_bar_centres_and_clears_the_traffic_lights() {
    use super::{MAC_APP_BAR, MAC_CONTENT_LEFT, MAC_LIGHT, MAC_LIGHT_PITCH, MAC_LIGHTS_LEFT};
    // Content keeps the same gap after the lights as the lights keep from the top.
    let margin = (MAC_APP_BAR - MAC_LIGHT) / 2.0;
    assert_eq!(MAC_CONTENT_LEFT - (MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT), margin);
    // `egui::Margin` is in `i8`.
    assert!(MAC_CONTENT_LEFT <= f32::from(i8::MAX));
    let mut app = app();
    app.integrated_titlebar = true;
    let h = test_window::open(app, vec2(1400.0, 900.0));
    // The bar's controls share the lights' centre line and start after them (relative to the
    // bar, which the test window insets).
    let bar = test_window::panel_rect(&h, "app_bar").min.to_vec2();
    let home = rect(&h, "Home").translate(-bar);
    assert_eq!(home.center().y, MAC_APP_BAR / 2.0, "{home:?}");
    assert!(home.min.y >= 2.0 && home.min.x >= MAC_CONTENT_LEFT, "{home:?}");
    for label in ["File", "Help", "Share"] {
        let r = rect(&h, label).translate(-bar);
        assert!((r.center().y - MAC_APP_BAR / 2.0).abs() <= 0.5 && r.max.y <= MAC_APP_BAR, "{label} at {r:?}");
    }
}

fn press(h: &egui_kittest::Harness<'static, test_window::Window>, pos: egui::Pos2, pressed: bool) {
    h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
}

/// The window commands the app sends over the next `steps` frames.
fn window_commands(h: &mut egui_kittest::Harness<'static, test_window::Window>, steps: usize) -> Vec<egui::ViewportCommand> {
    let mut out = Vec::new();
    for _ in 0..steps {
        h.step();
        if let Some(v) = h.output().viewport_output.get(&egui::ViewportId::ROOT) {
            out.extend(v.commands.iter().cloned());
        }
    }
    out
}

fn double_click(h: &mut egui_kittest::Harness<'static, test_window::Window>, pos: egui::Pos2) -> Vec<egui::ViewportCommand> {
    h.hover_at(pos);
    h.step();
    // One frame: the harness gives each queued event a frame of its own, longer apart than a
    // double-click allows.
    for pressed in [true, false, true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    window_commands(h, 3)
}

fn drag(h: &mut egui_kittest::Harness<'static, test_window::Window>, from: egui::Pos2) -> Vec<egui::ViewportCommand> {
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

#[test]
fn the_empty_mac_title_bar_zooms_and_moves_the_window() {
    use super::{MAC_APP_BAR, MAC_CONTENT_LEFT};
    use egui::ViewportCommand::{Maximized, Minimized, StartDrag};
    let mut app = app();
    app.integrated_titlebar = true;
    let mut h = test_window::open(app, vec2(1400.0, 900.0));
    let bar = test_window::panel_rect(&h, "app_bar");
    // Under the centred title, and between the traffic lights and the Home button.
    let empty = [pos2(bar.center().x, bar.min.y + MAC_APP_BAR / 2.0), pos2(bar.min.x + MAC_CONTENT_LEFT - 4.0, bar.min.y + 4.0)];
    for at in empty {
        // Tests read the setting as its default, zoom.
        let cmds = double_click(&mut h, at);
        assert_eq!(cmds, vec![Maximized(true)], "double-click at {at:?}");
        let cmds = drag(&mut h, at);
        assert_eq!(cmds, vec![StartDrag], "drag from {at:?}");
    }
    // The bar's controls keep their clicks and drags.
    for label in ["Home", "Share", "File"] {
        let at = rect(&h, label).center();
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
    use super::{MAC_APP_BAR, MAC_CONTENT_LEFT};
    let mut app = app();
    app.integrated_titlebar = true;
    let mut h = test_window::open(app, vec2(1400.0, 900.0));
    let bar = test_window::panel_rect(&h, "app_bar");
    let mid = bar.min.y + MAC_APP_BAR / 2.0;
    // The search field is the bar's last control: 125 wide, 4 from the 10-wide right margin.
    let search = pos2(bar.max.x - 10.0 - 4.0 - 60.0, mid);
    let controls = [rect(&h, "Home").center(), rect(&h, "Share").center(), rect(&h, "File").center(), search];
    let mut movable_at = |at: egui::Pos2| {
        h.hover_at(at);
        h.run_steps(2);
        craft_window::title_bar::window_movable(&h.ctx)
    };
    for at in [pos2(bar.center().x, mid), pos2(bar.min.x + MAC_CONTENT_LEFT - 4.0, bar.min.y + 4.0), pos2(bar.center().x, 500.0)] {
        assert!(movable_at(at), "empty title bar or the page at {at:?}");
    }
    for at in controls {
        assert!(!movable_at(at), "control at {at:?}");
    }
}

#[test]
fn a_narrow_window_scrolls_the_app_bar() {
    let w = 640.0;
    let mut h = test_window::open(app(), vec2(w, 600.0));
    let (help, share) = (rect(&h, "Help"), rect(&h, "Share"));
    assert!(help.max.x <= share.min.x, "the menus ({help:?}) and the workspace controls ({share:?}) overlap");
    wheel(&mut h, help.center(), vec2(0.0, -3000.0));
    let share = rect(&h, "Share");
    let window = egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(w, 600.0));
    assert!(window.contains_rect(share), "{share:?} outside the window");
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

struct WindowHarness {
    ctx: egui::Context,
    app: DesignApp,
    time: f64,
    maximized: bool,
    size: egui::Vec2,
}

impl WindowHarness {
    fn new() -> Self {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.custom_titlebar = true;
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        let mut harness = Self { ctx, app, time: 0.0, maximized: false, size: vec2(1440.0, 900.0) };
        harness.frame(vec![]);
        harness.frame(vec![]);
        harness
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> Vec<egui::ViewportCommand> {
        self.time += 0.1;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().maximized = Some(self.maximized);
        let mut output = self.ctx.run_ui(input, |ui| {
            app_bar(&mut self.app, ui);
            resize_borders(&mut self.app, ui);
        });
        output.textures_delta.clear();
        output.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
    }

    fn press(&mut self, pos: egui::Pos2) -> Vec<egui::ViewportCommand> {
        self.frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
        ])
    }

    fn click(&mut self, pos: egui::Pos2) -> Vec<egui::ViewportCommand> {
        let mut commands = self.press(pos);
        commands.extend(self.frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]));
        commands
    }

    fn caption_center(&self, id: &str) -> egui::Pos2 {
        self.ctx.read_response(egui::Id::unique(id)).unwrap().rect.center()
    }
}

#[test]
fn caption_buttons_minimize_maximize_restore_and_close() {
    let mut h = WindowHarness::new();
    for (id, command) in [("window.minimize", egui::ViewportCommand::Minimized(true)), ("window.maximize", egui::ViewportCommand::Maximized(true))] {
        let p = h.caption_center(id);
        assert!(h.click(p).contains(&command), "{id}");
    }
    h.maximized = true;
    h.frame(vec![]);
    let p = h.caption_center("window.maximize");
    assert!(h.click(p).contains(&egui::ViewportCommand::Maximized(false)));
    let p = h.caption_center("window.close");
    assert!(h.click(p).contains(&egui::ViewportCommand::Close));
}

#[test]
fn title_area_drags_and_double_clicks_but_menus_do_not_drag() {
    let mut h = WindowHarness::new();
    let title = h.ctx.read_response(egui::Id::unique("window_title_drag")).unwrap().rect;
    assert!(title.width() > 0.0, "there is a drag region between menus and tools");
    let p = title.center();
    let mut commands = h.press(p);
    commands.extend(h.frame(vec![egui::Event::PointerMoved(p + vec2(20.0, 0.0))]));
    assert!(commands.contains(&egui::ViewportCommand::StartDrag));

    let mut h = WindowHarness::new();
    h.click(p);
    assert!(h.click(p).contains(&egui::ViewportCommand::Maximized(true)));

    let mut h = WindowHarness::new();
    let p = egui::pos2(70.0, 18.0); // File menu, outside the title's drag region.
    let mut commands = h.press(p);
    commands.extend(h.frame(vec![egui::Event::PointerMoved(title.center())]));
    assert!(!commands.contains(&egui::ViewportCommand::StartDrag));
}

#[test]
fn edges_resize_only_when_the_window_is_restored() {
    let mut h = WindowHarness::new();
    let p = egui::pos2(1438.0, 400.0);
    let mut commands = h.press(p);
    commands.extend(h.frame(vec![egui::Event::PointerMoved(p + vec2(-20.0, 0.0))]));
    assert!(commands.contains(&egui::ViewportCommand::BeginResize(egui::ResizeDirection::East)));
    let mut h = WindowHarness::new();
    h.maximized = true;
    h.frame(vec![]);
    let mut commands = h.press(p);
    commands.extend(h.frame(vec![egui::Event::PointerMoved(p + vec2(-20.0, 0.0))]));
    assert!(!commands.iter().any(|c| matches!(c, egui::ViewportCommand::BeginResize(_))));
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
    let mut h = WindowHarness::new();
    h.size = vec2(640.0, 560.0);
    h.frame(vec![]);
    h.frame(vec![]);
    let rect = |h: &WindowHarness, id: &str| h.ctx.read_response(egui::Id::unique(id)).unwrap().rect;
    let menus_end = |h: &WindowHarness| h.ctx.data(|d| d.get_temp::<f32>(egui::Id::unique("app_bar_menus_end"))).unwrap();
    let (close, menus0) = (rect(&h, "window.close"), menus_end(&h));
    assert!(close.left() >= 0.0 && close.right() <= h.size.x, "{close:?}");
    let p = egui::pos2(200.0, 18.0);
    h.frame(vec![
        egui::Event::PointerMoved(p),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -3000.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
            source: egui::MouseWheelSource::Unknown,
        },
    ]);
    for _ in 0..10 {
        h.frame(vec![]);
    }
    assert!(menus_end(&h) < menus0, "the menus scrolled");
    assert_eq!(rect(&h, "window.close"), close, "the caption buttons did not");
    assert!(h.click(close.center()).contains(&egui::ViewportCommand::Close));
}

#[test]
fn the_window_drags_from_its_lower_band_at_the_minimum_width() {
    // At 900 px the menus and tools fill the bar's row; its lower band still moves the window,
    // and it isn't drawn over the caption buttons.
    let mut h = WindowHarness::new();
    h.size = vec2(900.0, 560.0);
    h.frame(vec![]);
    h.frame(vec![]);
    let drag = h.ctx.read_response(egui::Id::unique("window_title_drag")).unwrap().rect;
    let close = h.ctx.read_response(egui::Id::unique("window.close")).unwrap().rect;
    assert!(drag.right() <= close.left() + 0.5, "{drag:?} {close:?}");
    let p = egui::pos2(450.0, APP_BAR + APP_BAR_BAND / 2.0);
    assert!(drag.contains(p), "{drag:?}");
    let mut commands = h.press(p);
    commands.extend(h.frame(vec![egui::Event::PointerMoved(p + vec2(20.0, 0.0))]));
    assert!(commands.contains(&egui::ViewportCommand::StartDrag));
}

#[test]
fn caption_buttons_fit_the_minimum_window_width() {
    let mut h = WindowHarness::new();
    h.size = vec2(900.0, 560.0);
    h.frame(vec![]);
    h.frame(vec![]);
    for id in ["window.minimize", "window.maximize", "window.close"] {
        let rect = h.ctx.read_response(egui::Id::unique(id)).unwrap().rect;
        assert!(rect.left() >= 0.0 && rect.right() <= h.size.x, "{id}: {rect:?}");
        let p = rect.center();
        assert!(!h.click(p).is_empty(), "{id} remains clickable");
    }
}
