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

/// Insert Kashidas (World-Ready): how far a justified Arabic line may stretch each join.
/// `None` is the same as the paragraph's Insert Kashidas switch being off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KashidaLevel {
    None,
    Low,
    /// What kashidas did before there were levels: up to one and a half ems per join.
    #[default]
    Medium,
    High,
}

impl KashidaLevel {
    pub const ALL: [KashidaLevel; 4] = [Self::None, Self::Low, Self::Medium, Self::High];

    /// The longest kashida one join takes, in ems of the glyph before it.
    pub fn max_em(self) -> f64 {
        match self {
            Self::None => 0.0,
            Self::Low => 0.75,
            Self::Medium => 1.5,
            Self::High => 3.0,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}
