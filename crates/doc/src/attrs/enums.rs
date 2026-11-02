use super::{Deserialize, Serialize};

// ---------- value types ----------

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum Leading {
    /// Auto leading (percentage of the largest size on the line, Justification → Auto Leading).
    #[default]
    Auto,
    Points(f64),
}

/// Paragraph direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextDirection {
    #[default]
    LeftToRight,
    RightToLeft,
}

/// Digits (World-Ready): the figures 0–9 are drawn as these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Digits {
    /// Contextual digits; composition selects the preceding strong script/language.
    #[default]
    Default,
    /// European (0123).
    Arabic,
    /// Arabic-Indic (٠١٢٣).
    Hindi,
    /// Extended Arabic-Indic, as used for Persian and Urdu (۰۱۲۳).
    Farsi,
    /// Those of the text's language (Thai, Devanagari, Bengali, Arabic, Persian…); else as typed.
    Native,
}

impl Digits {
    /// The digit `c` (0–9) is drawn as, for text in `language`.
    pub fn map(self, c: char, language: &str) -> char {
        let d = match c {
            '0'..='9' => c as u32 - '0' as u32,
            '\u{0660}'..='\u{0669}' => c as u32 - 0x0660,
            '\u{06F0}'..='\u{06F9}' => c as u32 - 0x06F0,
            _ => return c,
        };
        let zero = match self {
            Digits::Default => return c,
            Digits::Arabic => 0x0030,
            Digits::Hindi => 0x0660,
            Digits::Farsi => 0x06F0,
            Digits::Native => {
                let l = language.to_ascii_lowercase();
                match () {
                    _ if l.starts_with("arabic") || l == "ar" || l.starts_with("ar-") => 0x0660,
                    _ if l.starts_with("persian")
                        || l.starts_with("farsi")
                        || l.starts_with("urdu")
                        || l == "fa"
                        || l.starts_with("fa-")
                        || l == "ur"
                        || l.starts_with("ur-") =>
                    {
                        0x06F0
                    }
                    _ if l.starts_with("hindi") || l.starts_with("marathi") || l.starts_with("nepali") || l.starts_with("sanskrit") => 0x0966,
                    _ if l.starts_with("bengali") => 0x09E6,
                    _ if l.starts_with("gujarati") => 0x0AE6,
                    _ if l.starts_with("punjabi") => 0x0A66,
                    _ if l.starts_with("tamil") => 0x0BE6,
                    _ if l.starts_with("telugu") => 0x0C66,
                    _ if l.starts_with("kannada") => 0x0CE6,
                    _ if l.starts_with("malayalam") => 0x0D66,
                    _ if l.starts_with("thai") => 0x0E50,
                    _ if l.starts_with("lao") => 0x0ED0,
                    _ if l.starts_with("tibetan") => 0x0F20,
                    _ if l.starts_with("burmese") => 0x1040,
                    _ if l.starts_with("khmer") => 0x17E0,
                    _ => return c,
                }
            }
        };
        char::from_u32(zero + d).unwrap_or(c)
    }
}

/// A tracked change on text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeMark {
    #[default]
    None,
    Inserted,
    Deleted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum Kerning {
    #[default]
    Metrics,
    Optical,
    /// Zero ("0" in the kerning menu).
    None,
    /// Manual pair kerning in 1/1000 em (applies after this character).
    Manual(f64),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capitalization {
    #[default]
    Normal,
    AllCaps,
    SmallCaps,
    OpenTypeAllSmallCaps,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Position {
    #[default]
    Normal,
    Superscript,
    Subscript,
    OtSuperscript,
    OtSubscript,
    OtNumerator,
    OtDenominator,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    LeftJustified,
    CenterJustified,
    RightJustified,
    FullyJustified,
    TowardsSpine,
    AwayFromSpine,
}

impl Align {
    pub fn is_justified(self) -> bool {
        matches!(self, Align::LeftJustified | Align::CenterJustified | Align::RightJustified | Align::FullyJustified)
    }
    pub const ALL: [Align; 9] = [
        Align::Left,
        Align::Center,
        Align::Right,
        Align::LeftJustified,
        Align::CenterJustified,
        Align::RightJustified,
        Align::FullyJustified,
        Align::TowardsSpine,
        Align::AwayFromSpine,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Align::Left => "Align left",
            Align::Center => "Align center",
            Align::Right => "Align right",
            Align::LeftJustified => "Justify with last line aligned left",
            Align::CenterJustified => "Justify with last line aligned center",
            Align::RightJustified => "Justify with last line aligned right",
            Align::FullyJustified => "Justify all lines",
            Align::TowardsSpine => "Align towards spine",
            Align::AwayFromSpine => "Align away from spine",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Composer {
    /// Total-fit (Knuth–Plass) over the whole paragraph.
    #[default]
    Paragraph,
    /// Greedy, line by line.
    SingleLine,
    /// The Japanese paragraph composer: total fit, with the CJK leading model and character
    /// alignment.
    Japanese,
    /// The Japanese single-line composer.
    JapaneseSingleLine,
}

impl Composer {
    /// Breaks lines greedily, one at a time.
    pub fn single_line(self) -> bool {
        matches!(self, Composer::SingleLine | Composer::JapaneseSingleLine)
    }

    /// A Japanese composer: lines follow the leading model ([`crate::cjk::LeadingModel`]) and
    /// characters their [`crate::cjk::CharacterAlignment`]. The others set lines baseline to
    /// baseline with every character on the Roman baseline.
    pub fn japanese(self) -> bool {
        matches!(self, Composer::Japanese | Composer::JapaneseSingleLine)
    }
}

/// What a nested style runs to (Drop Caps and Nested Styles).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "chars")]
pub enum NestedUntil {
    Sentences,
    #[default]
    Words,
    Characters,
    Letters,
    Digits,
    Tab,
    ForcedLineBreak,
    EmSpace,
    EnSpace,
    /// The paragraph's drop cap characters.
    Dropcap,
    /// Any of these characters.
    Chars(String),
}

/// A character style applied from the start of the paragraph through (or up to) the `count`-th
/// delimiter; nested styles follow each other.
/// A nested line style: `style` for the next `lines` lines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NestedLineStyle {
    pub style: String,
    pub lines: u32,
}

impl Default for NestedLineStyle {
    fn default() -> Self {
        NestedLineStyle { style: String::new(), lines: 1 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NestedStyle {
    pub style: String,
    /// Through (include the delimiter) or up to (stop before it).
    pub through: bool,
    pub count: u32,
    pub until: NestedUntil,
}

impl Default for NestedStyle {
    fn default() -> Self {
        NestedStyle { style: String::new(), through: true, count: 1, until: NestedUntil::Words }
    }
}

impl NestedStyle {
    /// The drop cap's character style as a nested style: `style` through 1 drop cap.
    pub fn drop_cap(style: &str) -> Self {
        NestedStyle { style: style.into(), through: true, count: 1, until: NestedUntil::Dropcap }
    }

    /// Is this the form a drop cap's character style takes at the start of a nested style list?
    pub fn is_drop_cap(&self) -> bool {
        self.until == NestedUntil::Dropcap && self.through && self.count == 1
    }
}

/// A character style applied to every match of a regular expression in the paragraph.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GrepStyle {
    pub style: String,
    pub pattern: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridAlign {
    #[default]
    None,
    AllLines,
    FirstLineOnly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartParagraph {
    #[default]
    Anywhere,
    NextColumn,
    NextFrame,
    NextPage,
    NextOddPage,
    NextEvenPage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "columns")]
pub enum SpanColumns {
    #[default]
    Single,
    /// Span `n` columns (0 = all).
    Span(u32),
    /// Split into `n` sub-columns.
    Split(u32),
}
