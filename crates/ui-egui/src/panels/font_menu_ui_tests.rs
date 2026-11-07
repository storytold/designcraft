use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::test_window::{self, Window, click_at};

/// The Type tool's selection over a new text frame's text, with the Control bar on. Returns the
/// story.
fn typing() -> (Harness<'static, Window>, u64) {
    typing_in("Advanced")
}

/// [`typing`] in the workspace `name`.
fn typing_in(name: &str) -> (Harness<'static, Window>, u64) {
    typing_on(name, vec2(1440.0, 900.0), false)
}

/// [`typing_in`] a window of `size`, with the macOS window chrome (the native menu bar and the
/// title bar inside the window) when `mac`.
fn typing_on(name: &str, size: egui::Vec2, mac: bool) -> (Harness<'static, Window>, u64) {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("window.workspace", json!({ "name": name })).unwrap();
    app.native_menu = mac;
    app.integrated_titlebar = mac;
    let mut h = test_window::open(app, size);
    let app = &mut h.state_mut().app;
    let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hello", "caret": true})).unwrap();
    app.run("tool.select", json!({"tool": "type"})).unwrap();
    app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 5})).unwrap();
    h.run_steps(4);
    (h, r["story"].as_u64().unwrap())
}

fn story_text(h: &mut Harness<'static, Window>, story: u64) -> String {
    h.state_mut().app.run("story.get", json!({"story": story})).unwrap()["text"].as_str().unwrap().to_string()
}

/// The font fields showing `family`, top to bottom.
fn fields(h: &Harness<'static, Window>, family: &str) -> Vec<egui::Rect> {
    let mut r: Vec<egui::Rect> =
        h.query_all_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.value().as_deref() == Some(family)).map(|n| n.rect()).collect();
    r.sort_by(|a, b| a.min.y.total_cmp(&b.min.y).then(a.min.x.total_cmp(&b.min.x)));
    r
}

/// The Control bar's font field (the topmost), showing `family`.
fn field(h: &Harness<'static, Window>, family: &str) -> egui::Rect {
    *fields(h, family).first().unwrap_or_else(|| panic!("a font field shows {family}"))
}

/// The text in the focused search field.
fn search(h: &Harness<'static, Window>) -> String {
    h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.is_focused()).value().unwrap_or_default()
}

fn family(h: &mut Harness<'static, Window>) -> String {
    h.state_mut().app.run("type.selectionAttrs", json!({})).unwrap()["chars"]["fontFamily"].as_str().unwrap().to_string()
}

/// The first matching family in this machine's font catalog. Installed variable fonts
/// can add more matches beside the bundled families.
fn first_match(h: &Harness<'static, Window>, query: &str) -> String {
    super::font_menu(&h.state().app).into_iter().find(|f| f.matches(query)).expect("a matching family").family
}

/// Run the menu until the filters' font kinds are read (it says Loading fonts… meanwhile).
fn settle(h: &mut Harness<'static, Window>) {
    for _ in 0..3000 {
        if h.query_by_label("Loading fonts…").is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.run_steps(1);
    }
    panic!("the font kinds were never read");
}

/// Open the Control bar's font menu (showing `family`) and search for `query`.
fn open_menu(h: &mut Harness<'static, Window>, family: &str, query: &str) {
    let at = field(h, family);
    click_at(h, at.center());
    if !query.is_empty() {
        h.event(egui::Event::Text(query.into()));
    }
    h.run_steps(4);
}

#[test]
fn a_family_opens_to_its_styles_and_a_style_is_picked() {
    let (mut h, _) = typing();
    let serif = designcraft_fonts::DEFAULT_FAMILY;
    open_menu(&mut h, serif, "serif 4");
    // The arrow before the name opens the family; the menu stays open.
    {
        let at = h.get_by_label("Show Styles").rect().center();
        click_at(&mut h, at);
    }
    assert!(h.query_by_label("Bold Italic").is_some(), "its styles are listed");
    {
        let at = h.get_by_label("Bold Italic").rect().center();
        click_at(&mut h, at);
    }
    let attrs = h.state_mut().app.run("type.selectionAttrs", json!({})).unwrap();
    assert_eq!((attrs["chars"]["fontFamily"].as_str(), attrs["chars"]["fontStyle"].as_str()), (Some(serif), Some("Bold Italic")));
    assert!(!h.ctx.text_edit_focused(), "picking closes the menu");
}

#[test]
fn picked_fonts_come_first_as_recently_used() {
    let (mut h, _) = typing();
    open_menu(&mut h, designcraft_fonts::DEFAULT_FAMILY, "source sans");
    {
        let at = h.get_by_label("Source Sans 3").rect().center();
        click_at(&mut h, at);
    }
    assert_eq!(h.state().app.session.prefs.recent_fonts, ["Source Sans 3"]);
    // Reopened without a search: Source Sans 3 is listed at the top, and again in its place (on
    // screen too when few fonts are installed).
    open_menu(&mut h, "Source Sans 3", "");
    let recent = h.query_all_by_label("Source Sans 3").map(|n| n.rect()).min_by(|a, b| a.min.y.total_cmp(&b.min.y)).expect("Source Sans 3 is listed");
    let search = h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.is_focused()).rect();
    assert!(recent.min.y > search.max.y && recent.min.y - search.max.y < 80.0, "the first row, under the filters: {recent:?} vs {search:?}");
}

#[test]
fn the_class_filter_and_similar_fonts_narrow_the_list() {
    let (mut h, _) = typing();
    open_menu(&mut h, designcraft_fonts::DEFAULT_FAMILY, "");
    {
        let at = h.get_by(|n| n.label().is_some_and(|l| l.starts_with("All Classes"))).rect().center();
        click_at(&mut h, at);
    }
    {
        let at = h.get_by_label("Monospaced").rect().center();
        click_at(&mut h, at);
    }
    settle(&mut h);
    assert!(h.query_by_label("JetBrains Mono").is_some(), "a monospaced family is listed");
    assert!(h.query_by_label(designcraft_fonts::DEFAULT_FAMILY).is_none(), "a serif isn't");
    // Back to all classes, then fonts like the serif: it leads, other serifs follow, no sans.
    {
        let at = h.get_by(|n| n.label().is_some_and(|l| l.starts_with("Monospaced ▾"))).rect().center();
        click_at(&mut h, at);
    }
    {
        let at = h.get_by_label("All Classes").rect().center();
        click_at(&mut h, at);
    }
    h.event(egui::Event::Text("serif 4".into()));
    h.run_steps(4);
    let row = h.get_by_label(designcraft_fonts::DEFAULT_FAMILY).rect();
    h.hover_at(row.center());
    h.run_steps(2);
    {
        let at = h.get_by_label("Show Similar Fonts").rect().center();
        click_at(&mut h, at);
    }
    assert!(h.query_by_label(&format!("Similar to {}", designcraft_fonts::DEFAULT_FAMILY)).is_some());
    assert!(h.query_by_label("Source Sans 3").is_none() && h.query_by_label("JetBrains Mono").is_none(), "other classes aren't similar");
    assert!(h.query_by_label(designcraft_fonts::DEFAULT_FAMILY).is_some());
}

#[test]
fn every_font_field_reports_as_a_combo_box_showing_the_family() {
    let (h, _) = typing();
    let shown = fields(&h, designcraft_fonts::DEFAULT_FAMILY);
    // The Control bar's, and the hand-drawn ones (Properties, the Contextual Task Bar).
    assert!(shown.len() >= 2, "{shown:?}");
}

#[test]
fn typing_in_the_font_menu_searches_and_never_reaches_the_story() {
    let (mut h, story) = typing();
    let before = story_text(&mut h, story);
    let at = field(&h, designcraft_fonts::DEFAULT_FAMILY);
    click_at(&mut h, at.center());
    assert!(h.ctx.text_edit_focused(), "the search field takes the keyboard when the menu opens");
    h.event(egui::Event::Text("source".into()));
    h.run_steps(4);
    assert_eq!(search(&h), "source");
    assert_eq!(story_text(&mut h, story), before, "the typing went to the search, not the story");
    assert!(h.query_by_label("Source Sans 3").is_some(), "a match is listed");
    assert!(h.query_by_label(designcraft_fonts::DEFAULT_FAMILY).is_some(), "both bundled Source families match");
    assert!(h.query_by_label("JetBrains Mono").is_none(), "families that don't match aren't");
    // Typing highlights the first match. ↓ then ↑ exercise navigation and return to it.
    let expected = first_match(&h, "source");
    assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(0));
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(1), "Down highlights the next family");
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(2);
    assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(0), "Up restores the first family");
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(family(&mut h), expected);
    assert_eq!(story_text(&mut h, story), before);
    assert!(!h.ctx.text_edit_focused(), "the menu closed");
    // Reopened, the search starts empty.
    let at = field(&h, &expected);
    click_at(&mut h, at.center());
    assert_eq!(search(&h), "", "the search starts empty");
    // A click on a family picks it.
    h.event(egui::Event::Text("serif 4".into()));
    h.run_steps(4);
    let row = h.get_by_label(designcraft_fonts::DEFAULT_FAMILY).rect();
    click_at(&mut h, row.center());
    assert_eq!(family(&mut h), designcraft_fonts::DEFAULT_FAMILY);
    // Escape closes the menu and leaves the text as it was.
    let at = field(&h, designcraft_fonts::DEFAULT_FAMILY);
    click_at(&mut h, at.center());
    // A star marks a favourite and keeps the menu open.
    h.event(egui::Event::Text("serif 4".into()));
    h.run_steps(4);
    let row = h.get_by_label(designcraft_fonts::DEFAULT_FAMILY).rect();
    click_at(&mut h, row.min + vec2(10.0, 12.0));
    assert!(h.state().app.session.prefs.favorite_fonts.iter().any(|f| f == designcraft_fonts::DEFAULT_FAMILY));
    assert_eq!(search(&h), "serif 4", "the menu stays open");
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!h.ctx.text_edit_focused(), "the menu closed");
    assert_eq!(story_text(&mut h, story), before);
    assert!(h.state().app.session.active().unwrap().selection.text.is_some(), "the text is still selected");
}

/// Opens the font menu of the field at `at`, clicks its search field, types, checks the menu
/// stays open and lists only the matches, then picks the first with ↓, ↑ and Return.
fn search_in_menu(h: &mut Harness<'static, Window>, at: egui::Rect, what: &str) -> String {
    click_at(h, at.center());
    let field = h.query_by(|n| n.role() == egui::accesskit::Role::TextInput && n.is_focused()).map(|n| n.rect());
    let field = field.unwrap_or_else(|| panic!("{what}: the menu opens with the search field focused"));
    click_at(h, field.center());
    assert!(h.ctx.text_edit_focused(), "{what}: a click on the search field keeps the menu open");
    h.event(egui::Event::Text("SOURCE".into()));
    h.run_steps(4);
    assert_eq!(search(h), "SOURCE", "{what}: the typing went to the search");
    assert!(h.query_by_label("Source Sans 3").is_some(), "{what}: a match is listed");
    assert!(h.query_by_label(designcraft_fonts::DEFAULT_FAMILY).is_some(), "{what}: both bundled Source families match");
    assert!(h.query_by_label("JetBrains Mono").is_none(), "{what}: families that don't match aren't");
    let expected = first_match(h, "source");
    assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(0), "{what}: typing highlights the first family");
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(1), "{what}: Down highlights the next family");
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(2);
    assert_eq!(super::font_menu::keyboard_highlight(&h.ctx), Some(0), "{what}: Up restores the first family");
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert!(!h.ctx.text_edit_focused(), "{what}: the menu closed");
    expected
}

/// Every font field over selected text (Control bar, Contextual Task Bar, Properties ›
/// Character, Character panel) searches, in the workspaces that show them and in a small
/// window with the macOS window chrome.
#[test]
fn every_font_field_searches_in_every_workspace() {
    for (name, size, mac, panel) in [
        ("Advanced", vec2(1440.0, 900.0), false, None),
        ("Printing and Proofing", vec2(1440.0, 900.0), false, None),
        ("Essentials", vec2(1440.0, 900.0), false, Some("character")),
        ("Printing and Proofing", vec2(900.0, 560.0), true, None),
    ] {
        let (mut h, story) = typing_on(name, size, mac);
        if panel.is_some() {
            h.state_mut().app.ui.open_panel = panel.map(str::to_string);
            h.run_steps(4);
        }
        let name = &format!("{name} {size:?} {panel:?}");
        let before = story_text(&mut h, story);
        let count = fields(&h, designcraft_fonts::DEFAULT_FAMILY).len();
        assert_eq!(count, 3, "{name}: the font fields are shown");
        for k in 0..count {
            h.state_mut().app.run("type.char", json!({"attrs": {"fontFamily": designcraft_fonts::DEFAULT_FAMILY}})).unwrap();
            h.run_steps(4);
            // A short Properties group (below it, the workspace's open panel) scrolls to the field.
            let mut nodes: Vec<_> = h
                .query_all_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.value().as_deref() == Some(designcraft_fonts::DEFAULT_FAMILY))
                .collect();
            nodes.sort_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y).then(a.rect().min.x.total_cmp(&b.rect().min.x)));
            if let Some(node) = nodes.get(k) {
                node.scroll_to_me();
            }
            h.run_steps(4);
            let at = fields(&h, designcraft_fonts::DEFAULT_FAMILY)[k];
            let expected = search_in_menu(&mut h, at, &format!("{name}, field {k}"));
            assert_eq!(family(&mut h), expected, "{name}, field {k}");
            assert_eq!(story_text(&mut h, story), before, "{name}, field {k}");
        }
    }
}

/// The font fields away from the text: the Glyphs panel, Paragraph Style Options › Basic
/// Character Formats and Find/Replace Font's Replace With.
#[test]
fn font_fields_in_panels_and_dialogs_search() {
    let paragraph = json!({"name": designcraft_doc::BASIC_PARAGRAPH, "section": "chars"});
    for (what, panel, dialog, shown) in [
        ("Glyphs", Some("glyphs"), None, designcraft_fonts::DEFAULT_FAMILY),
        ("Paragraph Style Options", None, Some(("paragraphStyleOptions", paragraph)), designcraft_fonts::DEFAULT_FAMILY),
        ("Find/Replace Font", None, Some(("findFont", json!({"toFamily": designcraft_fonts::DEFAULT_FAMILY}))), designcraft_fonts::DEFAULT_FAMILY),
    ] {
        let app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let mut h = test_window::open(app, vec2(1440.0, 900.0));
        let dialog_id = dialog.as_ref().map(|(id, _)| id.to_string());
        h.state_mut().app.ui.open_panel = panel.map(str::to_string);
        h.state_mut().app.ui.dialog = dialog.map(|(id, fields)| crate::dialogs::Dialog::new(id, fields));
        h.run_steps(4);
        let at = *fields(&h, shown).first().unwrap_or_else(|| panic!("{what}: the font field is shown"));
        let expected = search_in_menu(&mut h, at, what);
        assert_eq!(h.state().app.ui.dialog.as_ref().map(|d| d.id.clone()), dialog_id, "{what}: Return in the menu leaves the dialog open");
        assert!(!fields(&h, &expected).is_empty(), "{what}: the field shows the family picked");
        // Escape closes the menu only.
        let at = fields(&h, &expected)[0];
        click_at(&mut h, at.center());
        assert!(h.ctx.text_edit_focused(), "{what}: the menu opened");
        h.key_press(egui::Key::Escape);
        h.run_steps(4);
        assert!(!h.ctx.text_edit_focused(), "{what}: Escape closed the menu");
        assert_eq!(h.state().app.ui.dialog.as_ref().map(|d| d.id.clone()), dialog_id, "{what}: Escape in the menu leaves the dialog open");
    }
}

/// The font rows the open menu shows under its search field `search`: how many, and the bottom
/// of the lowest.
fn rows_shown(h: &Harness<'static, Window>, search: egui::Rect) -> (usize, f32) {
    let families: Vec<String> = crate::panels::font_menu(&h.state().app).iter().map(|f| f.label(true).to_string()).collect();
    let rows: Vec<egui::Rect> = h
        .query_all_by(|n| n.role() == egui::accesskit::Role::Button && n.label().is_some_and(|l| families.contains(&l)))
        .map(|n| n.rect())
        .filter(|r| r.min.y > search.max.y)
        .collect();
    (rows.len(), rows.iter().map(|r| r.max.y).fold(f32::MIN, f32::max))
}

/// Opens the font menu of the field at `at`, searches for a font that doesn't exist and clears
/// the search: the menu grows back to the rows it showed, with the search field where it was.
/// The menu lists every family by its English name: the test UI has no CJK font for a system
/// CJK family's native name.
fn menu_regrows_after_a_search_with_no_matches(h: &mut Harness<'static, Window>, at: egui::Rect, what: &str) {
    h.state_mut().app.session.prefs.show_font_names_in_english = true;
    click_at(h, at.center());
    h.run_steps(4);
    let focused = |h: &Harness<'static, Window>| h.query_by(|n| n.role() == egui::accesskit::Role::TextInput && n.is_focused()).map(|n| n.rect());
    let field = focused(h).unwrap_or_else(|| panic!("{what}: the menu opens with the search field focused"));
    let (count, bottom) = rows_shown(h, field);
    assert!(count > 1, "{what}: the menu lists the fonts: {count}");

    let query = "zzqqxxnomatch";
    h.event(egui::Event::Text(query.into()));
    h.run_steps(4);
    assert!(h.query_by_label("No fonts match").is_some(), "{what}: nothing matches");
    assert_eq!(rows_shown(h, field).0, 0, "{what}: no rows while nothing matches");

    for _ in query.chars() {
        h.key_press(egui::Key::Backspace);
    }
    h.run_steps(4);
    assert_eq!(search(h), "", "{what}: the search is cleared");
    let again = focused(h).unwrap_or_else(|| panic!("{what}: the menu is still open"));
    assert!((again.min.y - field.min.y).abs() < 0.5, "{what}: the search field stays at {}, not {}", field.min.y, again.min.y);
    let (count_again, bottom_again) = rows_shown(h, again);
    assert_eq!(count_again, count, "{what}: the menu shows as many rows as before the search");
    assert!((bottom_again - bottom).abs() < 1.0, "{what}: the menu grows back to {bottom}, not {bottom_again}");
}

#[test]
fn the_font_field_menu_grows_back_after_a_search_with_no_matches() {
    let (mut h, _) = typing();
    let at = field(&h, designcraft_fonts::DEFAULT_FAMILY);
    menu_regrows_after_a_search_with_no_matches(&mut h, at, "Control bar");
}

/// Paragraph Style Options › Basic Character Formats' Font Family combo box. The CJK style
/// fields' combo box hosts the same menu the same way.
#[test]
fn the_style_dialog_font_menu_grows_back_after_a_search_with_no_matches() {
    let app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let mut h = test_window::open(app, vec2(1440.0, 900.0));
    let section = json!({"name": designcraft_doc::BASIC_PARAGRAPH, "section": "chars"});
    h.state_mut().app.ui.dialog = Some(crate::dialogs::Dialog::new("paragraphStyleOptions", section));
    h.run_steps(4);
    let at = *fields(&h, designcraft_fonts::DEFAULT_FAMILY).first().expect("the dialog's font field is shown");
    menu_regrows_after_a_search_with_no_matches(&mut h, at, "Paragraph Style Options");
}
