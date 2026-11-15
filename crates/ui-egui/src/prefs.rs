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
mod tests;
