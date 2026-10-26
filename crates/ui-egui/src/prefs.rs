//! Compatibility recovery at the saved-preferences boundary. Command deserialization stays strict.

use craft_ui::docking::{Layout, Location};
use serde_json::Value;

use crate::{HiddenPanels, SavedWorkspace, UiState, panel_docking};

/// Saved preferences, and whether reading them discarded anything.
pub struct ReadPreferences {
    pub ui: UiState,
    /// False when unreadable docking data, or an old layout that could not be converted, was
    /// dropped. The caller keeps the file it read instead of overwriting it.
    pub intact: bool,
}

impl UiState {
    /// Read saved preferences, discarding only unreadable optional docking metadata.
    /// A layout from a newer craft-ui version must not reset unrelated settings or named workspaces.
    pub fn from_preferences_json(bytes: &[u8]) -> serde_json::Result<Self> {
        Self::read_preferences(bytes).map(|read| read.ui)
    }

    /// [`Self::from_preferences_json`], also reporting whether anything was discarded. Layouts
    /// saved before the canvas joined the panel layout are converted (see
    /// [`panel_docking::from_dock_only`]); that is not a loss, unless one cannot be converted.
    pub fn read_preferences(bytes: &[u8]) -> serde_json::Result<ReadPreferences> {
        let mut saved: Value = serde_json::from_slice(bytes)?;
        let mut intact = recover_docking(&mut saved);
        // Shortcut sets this version cannot read load as none (`lenient_sets`): keep the file.
        intact &= saved.get("shortcutSets").is_none_or(|v| v.is_null() || serde_json::from_value::<craft_keymap::Profiles>(v.clone()).is_ok());
        if let Some(workspaces) = saved.get_mut("customWorkspaces").and_then(Value::as_array_mut) {
            for workspace in workspaces {
                intact &= recover_docking(workspace);
            }
        }
        let mut ui: UiState = serde_json::from_value(saved)?;
        clear_stale_open_panels(&mut ui);
        intact &= migrate(&mut ui.docking, &mut ui.floating, &ui.docking_hidden, ui.dock_expanded, ui.icon_groups.as_deref());
        for workspace in &mut ui.custom_workspaces {
            let SavedWorkspace { docking, floating, docking_hidden, dock_expanded, icon_groups, .. } = workspace;
            intact &= migrate(docking, floating, docking_hidden, *dock_expanded, icon_groups.as_deref());
        }
        Ok(ReadPreferences { ui, intact })
    }
}

/// `UiState::shortcuts` in ui.json. `shortcuts` maps each changed command to its first
/// shortcut (`""` for none): the map of one text per command that earlier DesignCraft versions
/// read, and fail on with anything else, which drops the whole file. A command's further
/// shortcuts go in `shortcutsMore` (command → texts), which those versions ignore. Loading
/// appends them to the command's entry in `shortcuts`; extra shortcuts for a command missing
/// there are ignored, because an earlier version that saved the file left that command at its
/// defaults. `shortcuts` also loads in the shared keymap's array form, which the first builds
/// on it wrote.
pub(crate) mod saved_shortcuts {
    use std::collections::BTreeMap;

    use craft_keymap::{MAX_BINDINGS, Overrides};
    use serde::ser::SerializeMap;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(overrides: &Overrides, serializer: S) -> Result<S::Ok, S::Error> {
        let mut first = BTreeMap::new();
        let mut more = BTreeMap::new();
        for (command, texts) in overrides.iter() {
            // A blank text is no shortcut, as `""` is in the map.
            let mut texts = texts.iter().map(String::as_str).filter(|t| !t.trim().is_empty());
            first.insert(command, texts.next().unwrap_or_default());
            let rest: Vec<&str> = texts.collect();
            if !rest.is_empty() {
                more.insert(command, rest);
            }
        }
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("shortcuts", &first)?;
        if !more.is_empty() {
            map.serialize_entry("shortcutsMore", &more)?;
        }
        map.end()
    }

    // `Overrides` loads leniently (never fails), so neither field can make ui.json unreadable.
    #[derive(Default, Deserialize)]
    #[serde(default, rename_all = "camelCase")]
    struct Saved {
        shortcuts: Overrides,
        shortcuts_more: Overrides,
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Overrides, D::Error> {
        let Saved { mut shortcuts, shortcuts_more } = Saved::deserialize(deserializer)?;
        for (command, rest) in shortcuts_more.iter() {
            let Some(first) = shortcuts.get(command) else { continue };
            let texts: Vec<String> = first.iter().chain(rest).take(MAX_BINDINGS).cloned().collect();
            shortcuts.insert(command, texts);
        }
        Ok(shortcuts)
    }
}

/// `UiState::shortcut_sets`: anything that does not read as shortcut sets (a later format, a
/// hand edit) loads as none, keeping the rest of ui.json and the `shortcuts` the keymap uses.
pub(crate) fn lenient_sets<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<craft_keymap::Profiles, D::Error> {
    let value = <Value as serde::Deserialize>::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// Files from before the canvas joined the panel layout saved the flyout open at quit as
/// `openPanel`, which is the panel a workspace opens below Properties · Pages · Layers. Only a
/// workspace's own panel stays: a built-in workspace's (Printing and Proofing: Swatches), and in a
/// saved workspace one a built-in workspace opens (saving copies it). The current workspace keeps
/// a saved workspace's panel.
fn clear_stale_open_panels(ui: &mut UiState) {
    let built_in = |panel: &str| crate::menus::WORKSPACE_OPEN_PANELS.iter().any(|(_, p)| *p == panel);
    for workspace in &mut ui.custom_workspaces {
        if workspace.open_panel.as_deref().is_some_and(|panel| !built_in(panel)) {
            workspace.open_panel = None;
        }
    }
    let own = match ui.custom_workspaces.iter().find(|w| w.name == ui.workspace) {
        Some(saved) => saved.open_panel.as_deref(),
        None => crate::menus::workspace_open_panel(&ui.workspace),
    };
    if ui.open_panel.as_deref() != own {
        ui.open_panel = None;
    }
}

/// Drop docking data this version cannot read. True when nothing was dropped.
fn recover_docking(saved: &mut Value) -> bool {
    let Some(fields) = saved.as_object_mut() else { return true };
    let mut intact = true;
    if fields.get("docking").is_some_and(|value| serde_json::from_value::<Option<Layout<String>>>(value.clone()).is_err()) {
        fields.remove("docking");
        intact = false;
    }
    if let Some(hidden) = fields.get_mut("dockingHidden") {
        // `[[panel, location], …]`, or an object keyed by panel from older files.
        if let Some(entries) = hidden.as_array_mut() {
            let before = entries.len();
            entries.retain(|entry| serde_json::from_value::<(String, Location<String>)>(entry.clone()).is_ok());
            intact &= entries.len() == before;
        } else if let Some(entries) = hidden.as_object_mut() {
            let before = entries.len();
            entries.retain(|_, value| serde_json::from_value::<Location<String>>(value.clone()).is_ok());
            intact &= entries.len() == before;
        } else {
            fields.remove("dockingHidden");
            intact = false;
        }
    }
    intact
}

/// Convert a layout saved before the canvas joined the panel layout. One that cannot be
/// converted gives way to the workspace's default layout, which the workspace fields
/// (`dockTab`, `openPanel`, `floating`, `dockExpanded`, `iconGroups`) still describe. False when
/// a layout was dropped that way.
fn migrate(
    docking: &mut Option<Layout<String>>,
    floating: &mut Vec<(String, [f32; 2])>,
    hidden: &HiddenPanels,
    dock_expanded: bool,
    icon_groups: Option<&[Vec<String>]>,
) -> bool {
    let Some(old) = docking.as_ref().filter(|layout| panel_docking::dock_only(layout)) else { return true };
    *docking = panel_docking::from_dock_only(old, hidden.keys(), dock_expanded, icon_groups);
    if docking.is_some() {
        // The converted layout holds the floating dock panels.
        floating.retain(|(id, _)| crate::dock::float_only(id));
    }
    docking.is_some()
}

#[cfg(test)]
mod tests {
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
            SavedWorkspace {
                name: "Print".into(),
                docking: app.ui.docking.clone(),
                docking_hidden: app.ui.docking_hidden.clone(),
                ..Default::default()
            },
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
}
