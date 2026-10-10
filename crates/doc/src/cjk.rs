//! Portable CJK typography data. No platform fonts or UI dependencies.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CompositeFont {
    pub name: String,
    pub entries: Vec<CompositeFontEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CompositeFontEntry {
    pub name: String,
    /// Empty character set is the base entry, independent of its localized name.
    pub characters: String,
    pub family: String,
    pub style: String,
    pub relative_size: f64,
    pub horizontal_scale: f64,
    pub vertical_scale: f64,
    /// Percentage of the original em, as a fraction.
    pub baseline_shift: f64,
    pub scale_option: bool,
}
impl Default for CompositeFontEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            characters: String::new(),
            family: String::new(),
            style: "Regular".into(),
            relative_size: 1.0,
            horizontal_scale: 1.0,
            vertical_scale: 1.0,
            baseline_shift: 0.0,
            scale_option: true,
        }
    }
}
impl CompositeFont {
    pub fn entry(&self, c: char) -> Option<&CompositeFontEntry> {
        self.entries
            .iter()
            .rev()
            .find(|e| !e.characters.is_empty() && e.characters.contains(c))
            .or_else(|| self.entries.iter().find(|e| e.characters.is_empty()))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Kinsoku {
    pub name: String,
    pub no_start: String,
    pub no_end: String,
    pub inseparable: String,
    pub hanging: String,
}
impl Kinsoku {
    /// Explicitly supplied character sets take precedence over the fallback table.
    pub fn allows(&self, a: char, b: char) -> bool {
        !(self.no_end.contains(a) || self.no_start.contains(b) || (a == b && self.inseparable.contains(a)))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LeadingModel {
    #[default]
    Roman,
    AkiBelow,
    AkiAbove,
    Center,
    CenterDown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CharacterAlignment {
    #[default]
    Baseline,
    EmTop,
    EmCenter,
    EmBottom,
    IcfTop,
    IcfBottom,
}

impl Kinsoku {
    /// Clean-room Unicode rules for the named CJK sets. Explicit document tables
    /// are imported separately and always take precedence over these defaults.
    pub fn named(name: &str) -> Option<Self> {
        let name = name.trim_start_matches("KinsokuTable/").trim_start_matches("$ID/");
        let hard = match name {
            "HardKinsoku" | "kHardKinsokuName" => true,
            "SoftKinsoku"
            | "kSoftKinsokuName"
            | "SimplifiedChineseKinsoku"
            | "kSimpChineseKinsokuName"
            | "TraditionalChineseKinsoku"
            | "kTradChineseKinsokuName"
            | "KoreanKinsoku"
            | "kKoreanKinsokuName" => false,
            _ => return None,
        };
        let mut no_start = "、。，．・：；？！）」』】〕〉》｝］〙〗’”!),.:;?]}".to_string();
        if hard {
            no_start.push_str("ーぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ々〻ゝゞヽヾ");
        }
        Some(Self {
            name: name.into(),
            no_start,
            no_end: "（「『【〔〈《｛［〘〖‘“([{".into(),
            hanging: "、。，．,.".into(),
            inseparable: String::new(),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KinsokuHang {
    #[default]
    None,
    Regular,
    Force,
}

/// IDML spacing tables resolved by `mojikumi::Rules` for composition and diagnostics.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MojikumiTable {
    pub name: String,
    pub based_on: String,
    pub overrides: Vec<MojikumiAki>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MojikumiAki {
    pub target_class: i16,
    pub side_class: i16,
    pub after: bool,
    pub minimum: f64,
    pub desired: f64,
    pub maximum: f64,
    pub priority: i16,
    pub does_not_float: bool,
}
