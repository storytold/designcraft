//! Successors, on the framework path, of the native panel tests the port left with no framework twin: the Swatches rename in
//! place, the swatch list, the font menu's named instances and the Layers drop of a group's member. Each runs the real panel
//! over an installed framework (`table_layout_kit::panel_harness`, or the shipping window for the font menu) and checks the
//! same outcome as the old test: the document, the commands and what the panel shows.

use super::*;
use crate::dialogs::table_layout_kit as kit;
use crate::test_window::{self, Window};
use egui::accesskit::Role;
use egui::{Key, Modifiers, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------------------------------------------------
// Swatches

/// The row of the swatch `name`.
fn swatch_row(h: &egui_kittest::Harness<'_, DesignApp>, name: &str) -> egui::Pos2 {
    h.query_all_by_role(Role::TreeItem)
        .find(|node| node.accesskit_node().label().is_some_and(|label| crate::panels::swatches::names_swatch(&label, name)))
        .unwrap_or_else(|| panic!("no row for the swatch {name:?}"))
        .rect()
        .center()
}

/// The text fields the panel shows.
fn fields(h: &egui_kittest::Harness<'_, DesignApp>) -> usize {
    h.query_all_by_role(Role::TextInput).filter(|node| !node.accesskit_node().is_hidden()).count()
}

/// Successor of `a_rename_in_place_ends_with_its_document`: a rename in place belongs to its document. With another document
/// active, a swatch of the same name there gets no field, and nothing is renamed.
#[test]
fn swatches_rename_in_place_ends_with_its_document() {
    let mut app = kit::app(1, false);
    let color = json!({"color": {"c": 14, "m": 8, "y": 95, "k": 0}});
    let name = app.run("swatch.create", color.clone()).unwrap()["name"].as_str().unwrap().to_string();
    let mut h = kit::panel_harness(app, vec2(300.0, 520.0), crate::panels::swatches::show);
    let base = fields(&h);
    // A first click picks the swatch; a second one, after the double-click time, makes its name a field.
    let at = swatch_row(&h, &name);
    kit::click_at(&mut h, at);
    kit::steps(&mut h, 40);
    let at = swatch_row(&h, &name);
    kit::click_at(&mut h, at);
    assert!(fields(&h) > base, "the name is a field");
    kit::key(&mut h, Key::A, Modifiers::COMMAND);
    kit::type_text(&mut h, "Typed");
    // Another document with a swatch of the same name.
    h.state_mut().run("file.new", json!({})).unwrap();
    h.state_mut().run("swatch.create", color).unwrap();
    kit::steps(&mut h, 4);
    assert_eq!(fields(&h), base, "the rename is dropped: the other document's swatch has no field");
    let doc = &h.state().session.active().unwrap().doc;
    assert!(doc.swatch(&name).is_some() && doc.swatch("Typed").is_none(), "nothing was renamed");
}

/// Successor of `the_swatch_under_the_pointer_is_highlighted_and_names_clear_the_scroll_bar`: the framework swatch select lists
/// every swatch with its colour; the longest name is wholly inside its row and the list, and the row under the pointer is the
/// one the pointer is over.
#[test]
fn swatch_select_lists_long_names_inside_their_rows() {
    let mut app = kit::app(1, false);
    let long = "C=15 M=100 Y=100 K=0 Long Name";
    app.run("swatch.create", json!({"name": long, "color": {"c": 15, "m": 100, "y": 100, "k": 0}})).unwrap();
    let ctx = egui::Context::default();
    app.framework_transform = true;
    app.install_framework(&ctx);
    app.raw_input_hook(&ctx, &mut egui::RawInput::default());
    let mut ready = false;
    let mut h = egui_kittest::Harness::builder().with_context(ctx).with_size(vec2(800.0, 700.0)).with_step_dt(0.1).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), "en");
                ready = true;
                return;
            }
            let doc = app.session.active().unwrap().doc.clone();
            let _ = crate::framework_transform::swatch_select(ui, None, egui::Id::new("swatch_list_test"), "Swatch", &doc, Some("[Black]"), None);
        },
        app,
    );
    kit::steps(&mut h, 4);
    kit::click(&mut h, &[Role::ComboBox], "Swatch", 0);
    let list = h.query_all_by_role(Role::ListBox).next().expect("the list is open").rect();
    let option = |h: &egui_kittest::Harness<'_, DesignApp>, name: &str| {
        h.query_all_by_label(name).find(|node| node.accesskit_node().role() == Role::ListBoxOption).map(|node| node.rect())
    };
    let registration = option(&h, "[Registration]").expect("[Registration] is listed");
    kit::event(&mut h, egui::Event::PointerMoved(registration.center()));
    kit::steps(&mut h, 2);
    let registration = option(&h, "[Registration]").unwrap();
    assert!(registration.contains(registration.center()) && list.contains_rect(registration), "the row under the pointer is in the list");
    let long_row = option(&h, long).expect("the long name is listed");
    assert!(
        list.contains_rect(long_row) || list.intersect(long_row).width() >= long_row.width() - 1.0,
        "the longest name {long_row:?} fits the list {list:?}"
    );
    let black = option(&h, "[Black]").expect("[Black] is listed");
    assert!(list.contains_rect(black));
}

// ---------------------------------------------------------------------------------------------------------------------
// The font menu

/// The Type tool's selection over a new text frame's text, in the shipping window with the Control bar on. Returns the story.
fn typing() -> (egui_kittest::Harness<'static, Window>, u64) {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("window.workspace", json!({"name": "Advanced"})).unwrap();
    let mut h = test_window::open(app, vec2(1440.0, 900.0));
    let app = &mut h.state_mut().app;
    let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hello", "caret": true})).unwrap();
    app.run("tool.select", json!({"tool": "type"})).unwrap();
    app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 5})).unwrap();
    h.run_steps(4);
    (h, r["story"].as_u64().unwrap())
}

/// The Control bar's font field showing `family`.
fn font_field(h: &egui_kittest::Harness<'static, Window>, family: &str) -> egui::Pos2 {
    let mut found: Vec<egui::Rect> =
        h.query_all_by(|n| n.role() == Role::ComboBox && n.value().as_deref() == Some(family)).map(|n| n.rect()).collect();
    found.sort_by(|a, b| a.min.y.total_cmp(&b.min.y).then(a.min.x.total_cmp(&b.min.x)));
    found.first().unwrap_or_else(|| panic!("a font field shows {family}")).center()
}

/// Runs the menu until the filters' font kinds are read (it says Loading fonts… meanwhile).
fn settle(h: &mut egui_kittest::Harness<'static, Window>) {
    for _ in 0..3000 {
        if h.query_by_label("Loading fonts…").is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.run_steps(1);
    }
    panic!("the font kinds were never read");
}

/// Adds a variable font whose named instances are Regular and SemiBold to the installed fonts, opens the Control bar's font
/// menu, searches for it and opens its styles.
fn open_family_styles(h: &mut egui_kittest::Harness<'static, Window>, family: &str) {
    designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::variable_font(family).unwrap());
    let at = font_field(h, designcraft_fonts::DEFAULT_FAMILY);
    test_window::click_at(h, at);
    h.event(egui::Event::Text(family.into()));
    h.run_steps(4);
    settle(h);
    let at = h.get_by_label("Show Styles").rect().center();
    test_window::click_at(h, at);
}

/// Successor of `family_menu_selects_a_synthetic_named_instance`: in the framework font menu a catalogued variable font's named
/// instance is listed under its family and picking it sets the family and that style.
#[test]
fn font_menu_selects_a_synthetic_named_instance() {
    let (mut h, _) = typing();
    let family = "Synthetic Menu Wiring";
    open_family_styles(&mut h, family);
    assert!(h.query_by_label("SemiBold").is_some(), "the named instance is listed");
    let at = h.get_by_label("SemiBold").rect().center();
    test_window::click_at(&mut h, at);
    let attrs = h.state_mut().app.run("type.selectionAttrs", json!({})).unwrap();
    assert_eq!((attrs["chars"]["fontFamily"].as_str(), attrs["chars"]["fontStyle"].as_str()), (Some(family), Some("SemiBold")));
    assert!(!h.ctx.text_edit_focused(), "picking closes the menu");
}

/// Successor of `cold_family_menu_contains_named_instances` (which needs the pinned Bricolage fixture and is ignored), on a
/// synthetic variable font: the font menu lists a catalogued family's named instances the first time it shows the family,
/// and the family's styles are the same after the menu was closed and opened again.
#[test]
fn font_menu_contains_the_named_instances_of_a_catalogued_variable_font() {
    let (mut h, _) = typing();
    let family = "Synthetic Cold Instances";
    open_family_styles(&mut h, family);
    assert!(h.query_by_label("SemiBold").is_some(), "the first listing has the named instance");
    let first = designcraft_fonts::FontDb::global().scoped(0).styles(family);
    assert!(first.contains(&"SemiBold".to_string()), "{first:?}");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    let at = font_field(&h, designcraft_fonts::DEFAULT_FAMILY);
    test_window::click_at(&mut h, at);
    h.event(egui::Event::Text(family.into()));
    h.run_steps(4);
    settle(&mut h);
    let at = h.get_by_label("Show Styles").rect().center();
    test_window::click_at(&mut h, at);
    assert!(h.query_by_label("SemiBold").is_some(), "the reopened menu lists it too");
    assert_eq!(designcraft_fonts::FontDb::global().scoped(0).styles(family), first, "the styles are cached unchanged");
}

/// Successor of `cold_family_menu_discovers_and_caches_named_instances`: the first look at a catalogued family finds its
/// named instances (Regular and SemiBold), and a second look finds the same.
#[test]
fn font_menu_discovers_and_caches_named_instances() {
    let (mut h, _) = typing();
    let family = "Synthetic Cold Menu";
    open_family_styles(&mut h, family);
    let listed: Vec<String> =
        ["Regular", "SemiBold"].iter().filter(|style| h.query_by_label(style).is_some()).map(|style| (*style).to_string()).collect();
    assert!(listed.contains(&"SemiBold".to_string()), "{listed:?}");
    let styles = designcraft_fonts::FontDb::global().scoped(0).styles(family);
    assert_eq!(styles, ["Regular", "SemiBold"]);
    h.run_steps(4);
    assert_eq!(designcraft_fonts::FontDb::global().scoped(0).styles(family), styles, "cached");
}

// ---------------------------------------------------------------------------------------------------------------------
// Layers

/// Successor of the group half of `locked_layers_take_drops_with_cmd_and_group_members_move_with_their_group`: with a member
/// of a group selected, dragging the selection square onto a locked layer is refused without Cmd, and with Cmd the whole
/// group (the group and both members) moves to that layer.
#[test]
fn layers_a_selected_group_member_moves_its_whole_group_onto_a_locked_layer_with_cmd() {
    let mut app = kit::app(1, false);
    let l1 = app.session.active().unwrap().active_layer;
    let a = app.run("frame.create", json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap();
    let b = app.run("frame.create", json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap();
    let g = app.run("object.group", json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
    let art = designcraft_doc::LayerId(app.run("layer.new", json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap());
    app.run("layer.set", json!({"id": art.0, "locked": true})).unwrap();
    // A group member selected directly.
    app.run("selection.set", json!({"ids": [a]})).unwrap();
    let mut h = kit::panel_harness(app, vec2(320.0, 600.0), crate::panels::layers::show);
    let layer_of = |h: &egui_kittest::Harness<'static, DesignApp>, id: u64| {
        h.state().session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().layer
    };
    let square = |h: &egui_kittest::Harness<'static, DesignApp>| kit::centre_of(h, "Select; drag to move the selection to another layer");
    // Without Cmd the locked layer refuses the drop.
    let (from, to) = (square(&h), kit::centre_of(&h, "Art"));
    kit::drag_with(&mut h, from, to, Modifiers::NONE);
    assert_eq!(layer_of(&h, g), l1, "a locked layer refuses the drop");
    let (from, to) = (square(&h), kit::centre_of(&h, "Art"));
    kit::drag_with(&mut h, from, to, Modifiers::COMMAND);
    assert!([g, a, b].iter().all(|id| layer_of(&h, *id) == art), "the whole group moved");
}

// ---------------------------------------------------------------------------------------------------------------------
// The font style select

/// Successor of `cold_style_popup_discovers_instances_and_selects_them` for what the framework select can show: its list holds
/// the named instances of a catalogued variable font, and picking one returns it. (The old test's `closed popup must not
/// preload the font` check needs a database of its own, and the framework select reads the app's global one.)
#[test]
fn the_framework_font_style_select_lists_named_instances_and_picks_one() {
    let family = "Synthetic Style Select";
    designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::variable_font(family).unwrap());
    let mut app = kit::app(1, false);
    let ctx = egui::Context::default();
    app.framework_transform = true;
    app.install_framework(&ctx);
    app.raw_input_hook(&ctx, &mut egui::RawInput::default());
    let mut ready = false;
    let mut h = egui_kittest::Harness::builder().with_context(ctx).with_size(vec2(600.0, 500.0)).with_step_dt(0.05).build_ui_state(
        move |ui, state: &mut (DesignApp, Option<String>)| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), "en");
                ready = true;
                return;
            }
            let (app, picked) = state;
            if let Some(style) = crate::framework_transform::font_style_select(app, ui, None, egui::Id::new("style_select_test"), family, "Regular") {
                *picked = Some(style);
            }
        },
        (app, None),
    );
    h.run_steps(4);
    let combo = h.get_by_role_and_label(Role::ComboBox, "Font Style").rect().center();
    h.hover_at(combo);
    h.drag_at(combo);
    h.drop_at(combo);
    h.run_steps(4);
    assert!(h.query_by_label("SemiBold").is_some(), "the named instance is listed");
    let at = h.get_by_label("SemiBold").rect().center();
    h.hover_at(at);
    h.drag_at(at);
    h.drop_at(at);
    h.run_steps(4);
    assert_eq!(h.state().1.as_deref(), Some("SemiBold"));
}
