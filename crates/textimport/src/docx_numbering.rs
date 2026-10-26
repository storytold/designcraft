//! Word lists: `word/numbering.xml` and the `w:numPr` of paragraphs and paragraph styles.
//!
//! A `w:num` (numId) points at a `w:abstractNum`, whose `w:lvl` elements (levels 0–8) give
//! each level's format, label text, start number, indents and the text after the label.
//!
//! Numbering follows Word's main rules, simplified:
//! - Numbers count per level. A paragraph at level L restarts the levels below it (unless a
//!   level's `w:lvlRestart` says otherwise).
//! - `num` instances of one abstractNum share their counters, so a list carries on across
//!   them and across paragraphs that aren't in a list. A `num` with a `w:startOverride` counts
//!   on its own, starting at the override.
//! - Levels map to the model's list levels (`ilvl` + 1). Parent levels in a label (`%1.%2.`)
//!   become the model's level references (`^1.^2.`); where the model's level numbers would differ
//!   from Word's (a legal-style label, a level the model hasn't numbered), the importer writes
//!   them as text instead ([`ListPara::fixed_expression`]).

use std::collections::HashMap;

use designcraft_doc::{ListType, NumberStyle, ParaAttrs};

use crate::docx::El;

/// Most `w:abstractNum` or `w:num` elements read; the rest are ignored.
pub(crate) const MAX_DEFS: usize = 10_000;
/// Word's list levels are 0–8.
const LEVELS: usize = 9;
/// Longest label text (`w:lvlText`) kept, in characters.
const MAX_LVL_TEXT: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Format {
    #[default]
    Bullet,
    /// No number: the label is the level text alone.
    Nothing,
    Number(NumberStyle),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Suffix {
    #[default]
    Tab,
    Space,
    Nothing,
}

#[derive(Clone, Debug, Default)]
struct Level {
    format: Format,
    text: String,
    start: u32,
    suffix: Suffix,
    left: Option<f64>,
    /// First-line indent in points (negative for a hanging indent).
    first_line: Option<f64>,
    /// `w:lvlRestart`: 0 = never restart, n = restart after level n (1-based) or a higher one.
    restart: Option<u32>,
    /// `w:isLgl`: parent levels in the label are written as decimal numbers.
    legal: bool,
    /// The font of the label (`w:rPr/w:rFonts`), which decides what a Symbol/Wingdings bullet is.
    font: Option<String>,
    picture: bool,
}

#[derive(Debug, Default)]
struct Num {
    abstract_id: String,
    /// `w:lvlOverride` per level: a start override and/or a replacement level.
    start_override: [Option<u32>; LEVELS],
    level_override: [Option<Level>; LEVELS],
}

impl Num {
    fn has_start_override(&self) -> bool {
        self.start_override.iter().any(Option::is_some)
    }
}

/// A list paragraph: the paragraph attributes that make it one, and its number in Word.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ListPara {
    pub(crate) attrs: ParaAttrs,
    /// The paragraph's number at its level (also counted for bullets).
    pub(crate) number: u32,
    /// Parent levels the number expression refers to (`^k`, 1-based) and the text Word shows
    /// for each.
    pub(crate) parents: Vec<(u32, String)>,
    /// The number expression with Word's parent level numbers written as text.
    pub(crate) fixed_expression: Option<String>,
}

/// The lists of a Word document and the numbering state while its paragraphs are read in order.
#[derive(Debug, Default)]
pub(crate) struct Numbering {
    abstracts: HashMap<String, [Option<Level>; LEVELS]>,
    nums: HashMap<String, Num>,
    /// Current number per level, per counter key (an abstractNum, or a `num` with overrides).
    counters: HashMap<String, [Option<u32>; LEVELS]>,
}

fn level_index(e: &El) -> Option<usize> {
    e.attr("ilvl").and_then(|v| v.trim().parse::<usize>().ok()).filter(|l| *l < LEVELS)
}

fn parse_u32(v: Option<String>) -> Option<u32> {
    let v = v?;
    let v = v.trim();
    // Negative starts count from 0; numbers too large for a u32 are clamped.
    match v.parse::<i64>() {
        Ok(n) => Some(u32::try_from(n.max(0)).unwrap_or(u32::MAX)),
        Err(_) => None,
    }
}

fn number_style(fmt: &str) -> Format {
    match fmt {
        "bullet" => Format::Bullet,
        "none" => Format::Nothing,
        "decimalZero" => Format::Number(NumberStyle::ArabicLeadingZero),
        "upperRoman" => Format::Number(NumberStyle::UpperRoman),
        "lowerRoman" => Format::Number(NumberStyle::LowerRoman),
        "upperLetter" => Format::Number(NumberStyle::UpperLetters),
        "lowerLetter" => Format::Number(NumberStyle::LowerLetters),
        "chicago" => Format::Number(NumberStyle::Symbols),
        // decimal and every format the model has no style for (ordinal, cardinalText,
        // decimalEnclosedCircle, the East Asian counting systems, …).
        _ => Format::Number(NumberStyle::Arabic),
    }
}

fn parse_level(e: &El) -> Level {
    // Twips, clamped as elsewhere: a level's indent is a measure, not any number (#1593).
    let tw = |v: Option<&str>| v.and_then(|v| v.trim().parse::<f64>().ok()).map(|v| v / 20.0).and_then(designcraft_doc::attrs::imported_measure);
    let ind = e.child("pPr").and_then(|p| p.child("ind"));
    Level {
        format: e.val("numFmt").map_or(Format::Number(NumberStyle::Arabic), |f| number_style(&f)),
        text: e.child("lvlText").and_then(|t| t.attr("val")).unwrap_or_default().chars().take(MAX_LVL_TEXT).collect(),
        start: parse_u32(e.val("start")).unwrap_or(1),
        suffix: match e.val("suff").as_deref() {
            Some("space") => Suffix::Space,
            Some("nothing") => Suffix::Nothing,
            _ => Suffix::Tab,
        },
        left: ind.and_then(|i| tw(i.attr("left").or(i.attr("start")))),
        first_line: ind.and_then(|i| tw(i.attr("firstLine")).or(tw(i.attr("hanging")).map(|h| -h))),
        restart: parse_u32(e.val("lvlRestart")),
        legal: e.on("isLgl") == Some(true),
        font: e.child("rPr").and_then(|r| r.child("rFonts")).and_then(|f| f.attr("ascii").or(f.attr("hAnsi"))).map(str::to_string),
        picture: e.child("lvlPicBulletId").is_some(),
    }
}

/// The bullet Word shows for `text` in `font`. Symbol and Wingdings bullets are stored as
/// private-use characters (U+F0xx) or as the font's own code points; the common ones become
/// their Unicode look-alikes, unknown ones a plain bullet.
fn bullet_char(text: &str, font: Option<&str>) -> String {
    let font = font.unwrap_or_default().to_ascii_lowercase();
    let symbol = font.contains("symbol");
    let wingdings = font.contains("wingdings");
    text.chars()
        .map(|c| {
            let code = u32::from(c);
            let low = if (0xF000..=0xF0FF).contains(&code) { code - 0xF000 } else { code };
            if wingdings {
                return match low {
                    0x6C => '●',
                    0x6E => '■',
                    0x71 => '❑',
                    0x76 => '❖',
                    0xA7 => '▪',
                    0xD8 => '➢',
                    0xFC => '✓',
                    _ => '•',
                };
            }
            if symbol {
                return match low {
                    0x2D => '–',
                    0x6F => '◦',
                    _ => '•',
                };
            }
            match c {
                // Word's second-level bullet is a Courier New "o".
                'o' => '◦',
                '\u{E000}'..='\u{F8FF}' => '•',
                c => c,
            }
        })
        .filter(|c| *c != '\n' && *c != '\r')
        .collect()
}

impl Numbering {
    /// Read `numbering.xml`'s root (`w:numbering`).
    pub(crate) fn parse(root: &El) -> Self {
        let mut n = Numbering::default();
        for e in root.els() {
            match e.name.as_str() {
                "abstractNum" if n.abstracts.len() < MAX_DEFS => {
                    let Some(id) = e.attr("abstractNumId") else { continue };
                    let mut levels: [Option<Level>; LEVELS] = Default::default();
                    for l in e.els().filter(|l| l.name == "lvl") {
                        if let Some(slot) = level_index(l).and_then(|i| levels.get_mut(i)) {
                            *slot = Some(parse_level(l));
                        }
                    }
                    n.abstracts.insert(id.to_string(), levels);
                }
                "num" if n.nums.len() < MAX_DEFS => {
                    let (Some(id), Some(abstract_id)) = (e.attr("numId"), e.val("abstractNumId")) else { continue };
                    let mut num = Num { abstract_id, ..Default::default() };
                    for o in e.els().filter(|o| o.name == "lvlOverride") {
                        let Some(i) = level_index(o) else { continue };
                        if let Some(slot) = num.start_override.get_mut(i) {
                            *slot = parse_u32(o.val("startOverride"));
                        }
                        if let (Some(slot), Some(l)) = (num.level_override.get_mut(i), o.child("lvl")) {
                            *slot = Some(parse_level(l));
                        }
                    }
                    n.nums.insert(id.to_string(), num);
                }
                _ => {}
            }
        }
        n
    }

    /// The list formatting of the next paragraph with `w:numId` `num_id` at level `ilvl`, counting
    /// it. `None` (a plain paragraph) for numId 0, an unknown list or level, or a level past 8.
    pub(crate) fn next(&mut self, num_id: &str, ilvl: u32) -> Option<ListPara> {
        let lv = usize::try_from(ilvl).ok().filter(|l| *l < LEVELS)?;
        let num = self.nums.get(num_id)?;
        let abs = self.abstracts.get(&num.abstract_id)?;
        let level = |k: usize| num.level_override.get(k).and_then(Option::as_ref).or_else(|| abs.get(k).and_then(Option::as_ref));
        let def = level(lv)?;
        let own = num.has_start_override();
        let key = if own { format!("n:{num_id}") } else { format!("a:{}", num.abstract_id) };
        let start = |k: usize| {
            let o = if own { num.start_override.get(k).copied().flatten() } else { None };
            o.or_else(|| level(k).map(|l| l.start)).unwrap_or(1)
        };
        let counters = self.counters.entry(key).or_default();
        let n = counters.get(lv).copied().flatten().map_or_else(|| start(lv), |v| v.saturating_add(1));
        if let Some(c) = counters.get_mut(lv) {
            *c = Some(n);
        }
        for (m, c) in counters.iter_mut().enumerate().skip(lv + 1) {
            let restart = match level(m).and_then(|l| l.restart) {
                None => true,
                Some(0) => false,
                Some(r) => ilvl < r,
            };
            if restart {
                *c = None;
            }
        }
        // Parent levels' current numbers, as text, for the label.
        let parent = |k: usize| -> String {
            let v = counters.get(k).copied().flatten().unwrap_or_else(|| start(k));
            match level(k).map(|l| l.format) {
                Some(Format::Number(s)) if !def.legal => s.format(v),
                Some(Format::Number(_)) => v.to_string(),
                _ => String::new(),
            }
        };
        // `refs`: a parent number level becomes the model's `^k`; otherwise Word's text.
        let mut parents = Vec::new();
        let mut label = |own_level: &str, escape: bool, refs: bool| -> String {
            let mut out = String::new();
            let mut it = def.text.chars().peekable();
            while let Some(c) = it.next() {
                if c == '%'
                    && let Some(k) = it.peek().and_then(|d| d.to_digit(10)).filter(|d| (1..=9).contains(d))
                {
                    it.next();
                    let k = k as usize - 1;
                    let number = matches!(level(k).map(|l| l.format), Some(Format::Number(_)));
                    if k == lv {
                        out.push_str(own_level);
                    } else if refs && number && !def.legal {
                        out.push('^');
                        out.push_str(&(k + 1).to_string());
                        parents.push((k as u32 + 1, parent(k)));
                    } else {
                        let p = parent(k);
                        out.push_str(&if escape { p.replace('^', "^^") } else { p });
                    }
                } else if escape && c == '^' {
                    out.push_str("^^");
                } else if c != '\n' && c != '\r' {
                    out.push(c);
                }
            }
            out
        };
        // The model's defaults: `^t` after a number, a literal tab after a bullet.
        let (number_sep, bullet_sep) = match def.suffix {
            Suffix::Tab => ("^t", "\t"),
            Suffix::Space => (" ", " "),
            Suffix::Nothing => ("", ""),
        };
        let mut a = ParaAttrs::default();
        let mut fixed_expression = None;
        match def.format {
            Format::Number(style) => {
                a.list_type = Some(ListType::Numbers);
                a.number_style = Some(style);
                a.number_expression = Some(format!("{}{number_sep}", label("^#", true, true)));
                fixed_expression = Some(format!("{}{number_sep}", label("^#", true, false)));
            }
            Format::Bullet => {
                a.list_type = Some(ListType::Bullets);
                a.bullet_char = Some(if def.picture { "•".into() } else { bullet_char(&def.text, def.font.as_deref()) });
                a.list_separator = Some(bullet_sep.into());
            }
            Format::Nothing => {
                a.list_type = Some(ListType::Bullets);
                a.bullet_char = Some(label("", false, false));
                a.list_separator = Some(bullet_sep.into());
            }
        }
        a.left_indent = def.left;
        a.first_line_indent = def.first_line;
        a.list_level = Some(ilvl + 1);
        match def.restart {
            None => {}
            Some(0) => a.restart_numbers = Some(false),
            Some(r) => a.restart_after_level = Some(r),
        }
        Some(ListPara { attrs: a, number: n, parents, fixed_expression })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_and_wingdings_bullets_become_unicode() {
        assert_eq!(bullet_char("\u{F0B7}", Some("Symbol")), "•");
        assert_eq!(bullet_char("\u{F0A7}", Some("Wingdings")), "▪");
        assert_eq!(bullet_char("o", Some("Courier New")), "◦");
        assert_eq!(bullet_char("–", None), "–");
        assert_eq!(bullet_char("\u{F076}", None), "•");
    }
}
