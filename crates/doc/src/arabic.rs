//! Portable character controls for bidirectional and Arabic text.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CharacterDirection {
    #[default]
    Default,
    LeftToRight,
    RightToLeft,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiacriticPosition {
    #[default]
    Default,
    OpenType,
    OpenTypeFromBaseline,
    Loose,
    Medium,
    Tight,
}
