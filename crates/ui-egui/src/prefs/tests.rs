use super::*;
use crate::DesignApp;
use craft_ui::docking::{Column, Node, Side};
use serde_json::json;

fn populated() -> UiState {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
    app.run("window.panel.move", json!({"panel":"swatches", "anchor":"layers", "zone":"top"})).unwrap();
    app.run("window.panel.close", json!({"panel":"properties"})).unwrap();
    app.ui.ui_scale = 1.5;
    app.ui.language = "ja".into();
    app.ui.custom_workspaces = vec![
        SavedWorkspace { name: "Print".into(), docking: app.ui.docking.clone(), docking_hidden: app.ui.docking_hidden.clone(), ..Default::default() },
        SavedWorkspace { name: "Proof".into(), docking: app.ui.docking.clone(), ..Default::default() },
    ];
    app.ui
}

#[test]
fn saved_preferences_preserve_valid_layouts_and_hidden_origins() {
    let original = serde_json::to_value(populated()).unwrap();
    let loaded = UiState::from_preferences_json(&serde_json::to_vec(&original).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(loaded).unwrap(), original);
}

#[test]
fn future_layouts_recover_independently_without_resetting_preferences_or_workspaces() {
    let original = populated();
    for field in ["docking", "dockingHidden"] {
        for invalid in [json!({"root":{"FutureLayout":{}}}), json!(false)] {
            let mut saved = serde_json::to_value(&original).unwrap();
            saved[field] = invalid.clone();
            saved["customWorkspaces"][0][field] = invalid;
            assert!(serde_json::from_value::<UiState>(saved.clone()).is_err(), "command parsing remains strict");
            let loaded = UiState::from_preferences_json(&serde_json::to_vec(&saved).unwrap()).unwrap();
            assert_eq!(loaded.ui_scale, 1.5);
            assert_eq!(loaded.language, "ja");
            assert_eq!(loaded.custom_workspaces.len(), 2);
            assert_eq!(loaded.custom_workspaces[0].name, "Print");
            assert_eq!(loaded.custom_workspaces[1].name, "Proof");
            assert_eq!(loaded.custom_workspaces[1].docking, original.custom_workspaces[1].docking);
            if field == "docking" {
                assert!(loaded.docking.is_none());
                assert!(loaded.custom_workspaces[0].docking.is_none());
                assert_eq!(loaded.docking_hidden, original.docking_hidden);
                assert_eq!(loaded.custom_workspaces[0].docking_hidden, original.custom_workspaces[0].docking_hidden);
            } else {
                assert!(loaded.docking_hidden.is_empty());
                assert!(loaded.custom_workspaces[0].docking_hidden.is_empty());
                assert_eq!(loaded.docking, original.docking);
                assert_eq!(loaded.custom_workspaces[0].docking, original.custom_workspaces[0].docking);
            }
        }
    }
}

#[test]
fn unreadable_hidden_origin_does_not_discard_other_origins() {
    let original = populated();
    let mut saved = serde_json::to_value(&original).unwrap();
    saved["dockingHidden"].as_array_mut().unwrap().push(json!(["futurePanel", {"FutureLocation":{}}]));
    saved["customWorkspaces"][0]["dockingHidden"].as_array_mut().unwrap().push(json!(false));
    let loaded = UiState::from_preferences_json(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(!original.docking_hidden.is_empty());
    assert_eq!(loaded.docking_hidden, original.docking_hidden);
    assert_eq!(loaded.custom_workspaces[0].docking_hidden, original.custom_workspaces[0].docking_hidden);
}

#[test]
fn unreadable_shortcut_sets_are_reported_so_the_file_is_kept() {
    let mut saved = serde_json::to_value(populated()).unwrap();
    assert!(UiState::read_preferences(&serde_json::to_vec(&saved).unwrap()).unwrap().intact);
    saved.as_object_mut().unwrap().remove("shortcutSets");
    assert!(UiState::read_preferences(&serde_json::to_vec(&saved).unwrap()).unwrap().intact, "no sets is no loss");
    saved["shortcutSets"] = json!("a later format");
    let read = UiState::read_preferences(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(!read.intact);
    assert!(read.ui.shortcut_sets.sets().is_empty(), "the rest of ui.json still loads");
}

#[test]
fn unreadable_docking_is_reported_so_the_file_is_kept() {
    let mut saved = serde_json::to_value(populated()).unwrap();
    assert!(UiState::read_preferences(&serde_json::to_vec(&saved).unwrap()).unwrap().intact);
    saved["customWorkspaces"][1]["docking"] = json!({"root":{"FutureLayout":{}}});
    assert!(!UiState::read_preferences(&serde_json::to_vec(&saved).unwrap()).unwrap().intact);
}

/// The dock of a layout in the current shape: the canvas beside a dock at the end edge.
fn dock(layout: &Layout<String>) -> &[Column<String>] {
    assert!(crate::panel_docking::valid(layout), "{layout:?}");
    let Some(Node::Split { first, second, .. }) = &layout.root else { panic!("expected the canvas split: {layout:?}") };
    assert!(matches!(&**first, Node::Tabs { panels, .. } if panels == &["canvas"]));
    let Node::Dock { side: Side::End, columns } = &**second else { panic!("expected an end dock: {second:?}") };
    columns
}

fn panels(column: &Column<String>) -> Vec<Vec<&str>> {
    column.groups.iter().map(|g| g.panels.iter().map(String::as_str).collect()).collect()
}

/// Hand-written pre-trial `ui.json` files: each loads through the app's path with nothing lost.
fn load(saved: serde_json::Value) -> UiState {
    UiState::read_preferences(&serde_json::to_vec(&saved).unwrap()).map(|read| read.ui).unwrap()
}

#[test]
fn classic_dock_file_loads_as_its_workspace_default_layout() {
    let ui = load(json!({
        "workspace": "Printing and Proofing", "dockTab": "layers", "dockExpanded": true, "openPanel": "swatches",
        "floating": [["glyphs", [100.0, 120.0]]],
    }));
    assert!(ui.docking.is_none());
    let layout = crate::panel_docking::default_layout(&ui);
    let columns = dock(&layout);
    assert_eq!(panels(&columns[0]), [vec!["properties", "pages", "layers"], vec!["swatches"]]);
    assert_eq!(columns[0].groups[0].active, 2, "Layers stays in front");
    assert!(!columns[0].iconic && columns[1].iconic);
    assert!(columns[1].groups.iter().all(|g| !g.panels.iter().any(|p| p == "glyphs" || p == "swatches")));
    assert_eq!(layout.floating.len(), 1);
    assert_eq!(crate::panel_docking::window_groups(&layout.floating[0])[0].panels, ["glyphs"]);
    assert_eq!(&layout.floating[0].rect[..2], &[100.0, 120.0]);
}

/// An `openPanel` that was the flyout open at quit is not docked below Properties.
#[test]
fn a_flyout_open_at_quit_is_not_docked() {
    let ui = load(json!({
        "workspace": "Essentials", "dockExpanded": true, "openPanel": "stroke",
        "customWorkspaces": [
            {"name": "Mine", "dockExpanded": true, "openPanel": "effects"},
            {"name": "Proof", "dockExpanded": true, "openPanel": "swatches"},
        ],
    }));
    assert_eq!(ui.open_panel, None);
    let layout = crate::panel_docking::default_layout(&ui);
    assert_eq!(panels(&dock(&layout)[0]), [vec!["properties", "pages", "layers"]], "Stroke stays in its icon group");
    assert_eq!(ui.custom_workspaces[0].open_panel, None);
    assert_eq!(ui.custom_workspaces[1].open_panel.as_deref(), Some("swatches"), "saved from Printing and Proofing");
    let ui = load(json!({"workspace": "Proof", "openPanel": "swatches", "customWorkspaces": [{"name": "Proof", "openPanel": "swatches"}]}));
    assert_eq!(ui.open_panel.as_deref(), Some("swatches"), "the saved workspace's own panel");
}

#[test]
fn dock_only_layout_becomes_the_main_column() {
    let ui = load(json!({
        "dockExpanded": true,
        "docking": {"root": {"Tabs": {"panels": ["properties", "pages", "layers", "swatches"], "active": 3}}, "floating": []},
        "customWorkspaces": [{
            "name": "Split", "dockExpanded": true,
            "docking": {"root": {"Split": {"axis": "Horizontal", "size": {"Ratio": 0.5},
                "first": {"Tabs": {"panels": ["links"], "active": 0}},
                "second": {"Split": {"axis": "Vertical", "size": {"Ratio": 0.5},
                    "first": {"Tabs": {"panels": ["properties", "pages"], "active": 1}},
                    "second": {"Stack": {"entries": [
                        {"panel": "layers", "open": true, "height": null},
                        {"panel": "swatches", "open": false, "height": 120.0}]}}}}}},
                "floating": []},
        }],
    }));
    let columns = dock(ui.docking.as_ref().unwrap());
    assert_eq!(panels(&columns[0]), [vec!["properties", "pages", "layers", "swatches"]]);
    assert_eq!(columns[0].groups[0].active, 3);
    assert!(!columns[0].iconic);
    let icons = &columns[1];
    assert!(icons.iconic && icons.groups.iter().any(|g| g.panels == ["stroke", "color", "gradient"]));
    assert!(icons.groups.iter().all(|g| !g.panels.iter().any(|p| p == "swatches")), "each panel appears once");

    // Splits: a vertical split stacks groups in one column, a horizontal one adds a column
    // (the dock lists columns from the window edge inwards).
    let columns = dock(ui.custom_workspaces[0].docking.as_ref().unwrap());
    assert_eq!(panels(&columns[0]), [vec!["properties", "pages"], vec!["layers"], vec!["swatches"]]);
    assert_eq!(columns[0].groups[0].active, 1);
    assert!(columns[0].groups[2].minimized && columns[0].groups[2].height == Some(120.0), "a closed accordion entry stays minimized");
    assert_eq!(panels(&columns[1]), [vec!["links"]]);
    assert!(columns[2].iconic);
}

#[test]
fn icon_groups_become_the_iconic_column_in_order() {
    let ui = load(json!({
        "dockExpanded": true,
        "iconGroups": [["pages", "layers"], ["properties", "swatches", "color"]],
        "docking": {"root": {"Tabs": {"panels": ["properties"], "active": 0}}, "floating": []},
    }));
    let columns = dock(ui.docking.as_ref().unwrap());
    assert_eq!(columns.len(), 2);
    assert_eq!(panels(&columns[0]), [vec!["properties"]]);
    assert!(columns[1].iconic);
    assert_eq!(panels(&columns[1]), [vec!["pages", "layers"], vec!["swatches", "color"]]);
}

#[test]
fn collapsed_dock_makes_the_main_column_iconic() {
    let ui = load(json!({
        "dockExpanded": false,
        "docking": {"root": {"Tabs": {"panels": ["properties", "pages", "layers"], "active": 0}}, "floating": []},
    }));
    let columns = dock(ui.docking.as_ref().unwrap());
    assert_eq!(panels(&columns[0]), [vec!["properties", "pages", "layers"]]);
    assert!(columns.iter().all(|c| c.iconic));
}

#[test]
fn floating_windows_and_hidden_places_survive_the_migration() {
    let saved = json!({
        "dockExpanded": true,
        "floating": [["glyphs", [40.0, 60.0]], ["character", [40.0, 60.0]], ["tabs", [10.0, 20.0]]],
        "docking": {
            "root": {"Tabs": {"panels": ["properties", "pages", "layers"], "active": 0}},
            "floating": [{"panels": ["glyphs", "character"], "active": 1, "rect": [40.0, 60.0, 300.0, 400.0]}],
        },
        "dockingHidden": {"color": {"floating": null, "anchor": "layers", "placement": {"Split": "Bottom"},
            "size": {"Ratio": 0.5}, "stack": null}},
    });
    let read = UiState::read_preferences(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(read.intact, "converting is not a loss");
    let ui = read.ui;
    let layout = ui.docking.clone().unwrap();
    let columns = dock(&layout);
    assert_eq!(layout.floating.len(), 1);
    assert_eq!(crate::panel_docking::window_groups(&layout.floating[0])[0].panels, ["glyphs", "character"]);
    assert_eq!(crate::panel_docking::window_groups(&layout.floating[0])[0].active, 1);
    assert_eq!(layout.floating[0].rect, [40.0, 60.0, 300.0, 400.0]);
    assert_eq!(ui.floating, [("tabs".to_string(), [10.0, 20.0])], "the layout holds the floating dock panels");
    assert!(ui.docking_hidden.contains_key("color") && !layout.contains(&"color".to_string()), "a closed panel stays closed");
    assert!(columns[1].groups.iter().all(|g| g.panels != ["glyphs"]));
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
    app.ui = ui;
    app.run("window.panel", json!({"panel": "color"})).unwrap();
    assert!(app.ui.docking.as_ref().unwrap().contains(&"color".to_string()));
    assert!(!app.ui.docking_hidden.contains_key("color"));
    // Saved again, the file is in the current shape and loads unchanged.
    let again = UiState::from_preferences_json(&serde_json::to_vec(&app.ui).unwrap()).unwrap();
    assert_eq!(again.docking, app.ui.docking);
}

/// Floating windows saved as a stack of groups (`groups`, before floating windows held split
/// trees) load as a vertical split of the groups in order, sized by their heights; a window
/// whose groups were all minimized loads minimized.
#[test]
fn floating_stacks_of_groups_load_as_split_windows() {
    let mut saved = serde_json::to_value(UiState::default()).unwrap();
    let mut docking = serde_json::to_value(crate::panel_docking::default_layout(&UiState::default())).unwrap();
    docking["floating"] = json!([
        {"groups": [
            {"panels": ["swatches", "color"], "active": 1, "minimized": false, "height": 300.0},
            {"panels": ["stroke"], "active": 0, "minimized": false, "height": 100.0},
        ], "rect": [400.0, 100.0, 280.0, 440.0]},
        {"groups": [{"panels": ["glyphs"], "active": 0, "minimized": true, "height": null}], "rect": [40.0, 60.0, 300.0, 400.0]},
    ]);
    docking["root"]["Split"]["second"]["Dock"]["columns"][1]["groups"] = json!([{"panels": ["effects"], "active": 0}]);
    saved["docking"] = docking;
    let read = UiState::read_preferences(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(read.intact);
    let layout = read.ui.docking.clone().unwrap();
    dock(&layout);
    let Node::Split { axis: craft_ui::layout::SplitAxis::Vertical, size: craft_ui::layout::SplitSize::Ratio(ratio), first, second } =
        &layout.floating[0].root
    else {
        panic!("expected a vertical split: {:?}", layout.floating[0]);
    };
    assert!((ratio - 0.75).abs() < 1e-4, "{ratio}");
    assert_eq!(**first, Node::Tabs { panels: vec!["swatches".into(), "color".into()], active: 1 });
    assert_eq!(**second, Node::Tabs { panels: vec!["stroke".into()], active: 0 });
    assert_eq!(layout.floating[0].rect, [400.0, 100.0, 280.0, 440.0]);
    assert!(!layout.floating[0].minimized && layout.floating[1].minimized);
    let again = UiState::from_preferences_json(&serde_json::to_vec(&read.ui).unwrap()).unwrap();
    assert_eq!(again.docking, read.ui.docking);
}

/// An old layout that cannot be converted gives way to the workspace's default and is
/// reported, so the file it came from is kept.
#[test]
fn an_old_layout_that_cannot_be_converted_is_reported() {
    let saved = json!({"docking": {"root": {"Tabs": {"panels": ["properties", "properties"], "active": 0}}, "floating": []}});
    let read = UiState::read_preferences(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(!read.intact);
    assert_eq!(read.ui.docking, None);
}

/// `dockingHidden` as an object keyed by panel (files before craft-ui's `Workspace::hidden`
/// list) loads as the list, drops only its unreadable entries and saves as the list.
#[test]
fn hidden_panels_keyed_by_panel_load_as_the_workspace_list() {
    let place = json!({"floating": null, "anchor": "layers", "placement": {"Split": "Bottom"}, "size": {"Ratio": 0.5}, "stack": null});
    let saved = json!({
        "dockingHidden": {"color": place, "swatches": place, "futurePanel": {"FutureLocation": {}}},
        "customWorkspaces": [{"name": "Old", "dockingHidden": {"stroke": place}}],
    });
    let read = UiState::read_preferences(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(!read.intact, "the unreadable entry is reported so the file is kept");
    let ui = read.ui;
    assert_eq!(ui.docking_hidden.keys().collect::<Vec<_>>(), ["color", "swatches"]);
    assert_eq!(ui.custom_workspaces[0].docking_hidden.keys().collect::<Vec<_>>(), ["stroke"]);
    let written = serde_json::to_value(&ui).unwrap();
    assert_eq!(written["dockingHidden"][0][0], "color");
    let workspace: craft_ui::docking::Workspace<String> =
        serde_json::from_value(json!({"layout": {"root": null, "floating": []}, "hidden": written["dockingHidden"]})).unwrap();
    assert_eq!(workspace.hidden, ui.docking_hidden.0, "the same shape as craft-ui's Workspace::hidden");
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
    app.ui = ui;
    app.run("window.panel", json!({"panel": "color"})).unwrap();
    assert!(app.ui.docking.as_ref().unwrap().contains(&"color".to_string()));
    assert!(!app.ui.docking_hidden.contains_key("color"));
}

#[test]
fn unrelated_malformed_preferences_are_still_rejected() {
    assert!(UiState::from_preferences_json(br#"{"uiScale":"invalid","docking":false}"#).is_err());
    assert!(UiState::from_preferences_json(br#"{"customWorkspaces":false}"#).is_err());
}
