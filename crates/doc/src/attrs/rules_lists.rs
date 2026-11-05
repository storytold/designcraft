use super::{Deserialize, ParaProps, Serialize};

/// The colour of a paragraph rule that follows the paragraph's text colour.
pub const TEXT_COLOR: &str = "Text Color";

/// A paragraph rule (Rule Above / Rule Below).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Rule {
    pub on: bool,
    pub weight: f64,
    /// A swatch, or [`TEXT_COLOR`].
    pub color: String,
    pub tint: f32,
    /// Width: column (true) or text (false).
    pub column_width: bool,
    pub offset: f64,
    pub left_indent: f64,
    pub right_indent: f64,
    /// Type: the rule's stroke type (solid, dashed, wavy…, or a stroke style of the document).
    #[serde(rename = "type", skip_serializing_if = "rule_solid")]
    pub kind: crate::item::StrokeType,
    /// Gap Color (a swatch; [None]: none) and Gap Tint: under the gaps of a type that has them.
    #[serde(skip_serializing_if = "rule_no_gap")]
    pub gap_color: String,
    #[serde(skip_serializing_if = "rule_full_tint")]
    pub gap_tint: f32,
    /// Keep In Frame: a rule above the first line of a frame stays inside the frame.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub keep_in_frame: bool,
}

// A rule's settings at their defaults are left out of the document (and of the attributes).
fn rule_solid(k: &crate::item::StrokeType) -> bool {
    *k == crate::item::StrokeType::Solid
}
fn rule_no_gap(c: &str) -> bool {
    c == designcraft_color::swatch::NONE
}
fn rule_full_tint(t: &f32) -> bool {
    *t == 1.0
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            on: false,
            weight: 1.0,
            color: TEXT_COLOR.into(),
            tint: 1.0,
            column_width: true,
            offset: 0.0,
            left_indent: 0.0,
            right_indent: 0.0,
            kind: crate::item::StrokeType::Solid,
            gap_color: designcraft_color::swatch::NONE.into(),
            gap_tint: 1.0,
            keep_in_frame: false,
        }
    }
}

/// How wide a paragraph border or shading box is: the column, or the text's longest line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BoxWidth {
    #[default]
    Column,
    Text,
}

/// Where a paragraph border or shading box starts above the first line (before its top offset).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BoxTop {
    /// The tallest ascent on the line.
    #[default]
    Ascent,
    Baseline,
    /// The top of the em box.
    EmBox,
    /// The top of the line's leading.
    Leading,
}

/// Where a paragraph border or shading box ends below the last line (before its bottom offset).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BoxBottom {
    /// The deepest descent on the line.
    #[default]
    Descent,
    Baseline,
    /// The bottom of the em box.
    EmBox,
}

/// Where a bullet or number sits against the first line's start (the left and first-line
/// indents): it starts there, is centred on it, or ends there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListType {
    #[default]
    None,
    Bullets,
    Numbers,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NumberStyle {
    #[default]
    Arabic,
    UpperRoman,
    LowerRoman,
    UpperLetters,
    LowerLetters,
    /// 01, 02, …
    ArabicLeadingZero,
    /// 001, 002, …
    ArabicThreeDigits,
    /// 0001, 0002, …
    ArabicFourDigits,
    /// *, †, ‡, §, ¶, ‖, then doubled (**, ††, …): footnotes.
    Symbols,
    /// 一, 二, 三, …: Chinese numerals written digit by digit, so 10 is 一〇 and 38 is 三八.
    Kanji,
}

/// Numbers past this are written in Arabic digits by the styles that repeat characters (roman
/// numerals, letters, symbols), so a hostile start number can't build a huge label.
const MAX_REPEATED_FORM: u32 = 3999;

impl NumberStyle {
    pub fn format(self, n: u32) -> String {
        if n > MAX_REPEATED_FORM
            && matches!(
                self,
                NumberStyle::UpperRoman | NumberStyle::LowerRoman | NumberStyle::UpperLetters | NumberStyle::LowerLetters | NumberStyle::Symbols
            )
        {
            return n.to_string();
        }
        match self {
            NumberStyle::Arabic => n.to_string(),
            NumberStyle::ArabicLeadingZero => format!("{n:02}"),
            NumberStyle::ArabicThreeDigits => format!("{n:03}"),
            NumberStyle::ArabicFourDigits => format!("{n:04}"),
            NumberStyle::UpperRoman => roman(n),
            NumberStyle::LowerRoman => roman(n).to_lowercase(),
            NumberStyle::UpperLetters => letters(n),
            NumberStyle::LowerLetters => letters(n).to_lowercase(),
            NumberStyle::Symbols => {
                const SYM: [char; 6] = ['*', '†', '‡', '§', '¶', '‖'];
                let n = n.max(1) as usize - 1;
                std::iter::repeat_n(SYM[n % SYM.len()], n / SYM.len() + 1).collect()
            }
            NumberStyle::Kanji => kanji_digits(n),
        }
    }
}

/// Expand the metacharacters of a list's label text (`^t` tab, `^>` en space, `^^` caret, …),
/// putting `number` in for `^#`. Level placeholders (`^1`…`^9`) and metacharacters with no
/// meaning in a label are dropped; a label never holds a paragraph break.
pub fn expand_list_text(s: &str, number: &str) -> String {
    expand_list_levels(s, number, &[])
}

/// [`expand_list_text`] with `levels[k]` in for `^1`…`^9` (level k + 1's current number; a level
/// past the end is dropped).
pub fn expand_list_levels(s: &str, number: &str, levels: &[String]) -> String {
    let mut out = String::with_capacity(s.len() + number.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '^' {
            if c != '\n' && c != '\r' {
                out.push(c);
            }
            continue;
        }
        let Some(m) = it.next() else {
            out.push('^');
            break;
        };
        let r = match m {
            '#' => {
                out.push_str(number);
                continue;
            }
            d @ '1'..='9' => {
                let k = d as usize - '1' as usize;
                if let Some(l) = levels.get(k) {
                    out.push_str(l);
                }
                continue;
            }
            '^' => '^',
            't' => '\t',
            'y' => crate::story::RIGHT_INDENT_TAB,
            'i' => crate::story::INDENT_HERE,
            'n' => crate::story::FORCED_LINE_BREAK,
            'm' => '\u{2003}',
            '>' => '\u{2002}',
            'f' => '\u{2001}',
            '|' => '\u{200A}',
            '<' => '\u{2009}',
            '/' => '\u{2007}',
            '.' => '\u{2008}',
            's' => '\u{A0}',
            'S' => '\u{202F}',
            '_' => '\u{2014}',
            '=' => '\u{2013}',
            '~' => '\u{2011}',
            '{' => '\u{201C}',
            '}' => '\u{201D}',
            '[' => '\u{2018}',
            ']' => '\u{2019}',
            '"' => '"',
            '\'' => '\'',
            _ => continue,
        };
        out.push(r);
    }
    out
}

fn kanji_digits(n: u32) -> String {
    const DIGITS: [char; 10] = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
    n.to_string().chars().map(|c| c.to_digit(10).and_then(|d| DIGITS.get(d as usize)).copied().unwrap_or(c)).collect()
}

fn roman(mut n: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let t = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for (v, r) in t {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// A, B, …, Z, AA, BB, … (InDesign's letter numbering repeats the letter).
fn letters(n: u32) -> String {
    if n == 0 {
        return String::new();
    }
    let i = (n - 1) % 26;
    let reps = (n - 1) / 26 + 1;
    std::iter::repeat_n(char::from(b'A' + i as u8), reps as usize).collect()
}

/// Levels a list can have.
pub const LIST_LEVELS: usize = 9;

/// Index (0-based) of list level `level` (1-based, clamped to 1–[`LIST_LEVELS`]).
pub(super) fn list_level_index(level: u32) -> usize {
    (level.clamp(1, LIST_LEVELS as u32) - 1) as usize
}

/// The numbering of one list as it goes: each level's last number and its text, and when each
/// level last numbered a paragraph (for Restart Numbers at This Level After).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ListCounter {
    numbers: [u32; LIST_LEVELS],
    pub(super) labels: [String; LIST_LEVELS],
    seen: [u64; LIST_LEVELS],
    tick: u64,
}

impl ListCounter {
    /// Number the next paragraph of `pp` (a numbered paragraph with text) and return its number.
    pub fn advance(&mut self, pp: &ParaProps) -> u32 {
        let l = list_level_index(pp.list_level);
        self.tick = self.tick.saturating_add(1);
        let since = self.seen[l];
        let restart = pp.restart_numbers
            && match pp.restart_after_level {
                0 => self.seen[..l].iter().any(|&t| t > since),
                m => (m as usize) <= l && self.seen.get(m as usize - 1).is_some_and(|&t| t > since),
            };
        let n = match pp.start_at {
            Some(s) => s.max(1),
            None if restart => 1,
            None => self.numbers[l].saturating_add(1),
        };
        self.numbers[l] = n;
        self.labels[l] = pp.number_style.format(n);
        self.seen[l] = self.tick;
        n
    }
}
