//! Compatibility recovery at the saved-preferences boundary. Command deserialization stays strict.

use craft_ui::docking::{Layout, Location};
use serde_json::Value;

use crate::UiState;

impl UiState {
    /// Read saved preferences, discarding only unreadable optional docking metadata.
    /// A layout from a newer craft-ui version must not reset unrelated settings or named workspaces.
    pub fn from_preferences_json(bytes: &[u8]) -> serde_json::Result<Self> {
        let mut saved: Value = serde_json::from_slice(bytes)?;
        recover_docking(&mut saved);
        if let Some(workspaces) = saved.get_mut("customWorkspaces").and_then(Value::as_array_mut) {
            for workspace in workspaces {
                recover_docking(workspace);
            }
        }
        serde_json::from_value(saved)
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

fn recover_docking(saved: &mut Value) {
    let Some(fields) = saved.as_object_mut() else { return };
    if fields.get("docking").is_some_and(|value| serde_json::from_value::<Option<Layout<String>>>(value.clone()).is_err()) {
        fields.remove("docking");
    }
    if let Some(hidden) = fields.get_mut("dockingHidden") {
        if let Some(entries) = hidden.as_object_mut() {
            entries.retain(|_, value| serde_json::from_value::<Location<String>>(value.clone()).is_ok());
        } else {
            fields.remove("dockingHidden");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DesignApp, SavedWorkspace};
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
        saved["dockingHidden"]["futurePanel"] = json!({"FutureLocation":{}});
        saved["customWorkspaces"][0]["dockingHidden"]["futurePanel"] = json!(false);
        let loaded = UiState::from_preferences_json(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert!(!original.docking_hidden.is_empty());
        assert_eq!(loaded.docking_hidden, original.docking_hidden);
        assert_eq!(loaded.custom_workspaces[0].docking_hidden, original.custom_workspaces[0].docking_hidden);
    }

    #[test]
    fn unrelated_malformed_preferences_are_still_rejected() {
        assert!(UiState::from_preferences_json(br#"{"uiScale":"invalid","docking":false}"#).is_err());
        assert!(UiState::from_preferences_json(br#"{"customWorkspaces":false}"#).is_err());
    }
}
