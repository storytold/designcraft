//! Unicode Mojikumi rules shared by composition and preflight.
//!
//! IDML class numbers follow the documented Mojikumi override interchange format;
//! they are not the similarly numbered JLReq classes. Spacing is measured in em.
use crate::{Styles, cjk::MojikumiAki};

#[derive(Clone, Copy, Debug, Default)]
pub struct Aki {
    pub min: f64,
    pub desired: f64,
    pub max: f64,
    pub priority: u8,
    /// Discrete endpoints rather than a continuously floating amount.
    pub discrete: bool,
}
impl Aki {
    fn fixed(value: f64) -> Self {
        Self { min: value, desired: value, max: value, ..Self::default() }
    }
}

/// Resolved table; custom rows override the base preset in document order.
pub struct Rules<'a> {
    chinese: bool,
    rows: &'a [MojikumiAki],
}
impl<'a> Rules<'a> {
    pub fn resolve(styles: &'a Styles, name: &str) -> Result<Option<Self>, String> {
        if matches!(name, "" | "Nothing" | "None") {
            return Ok(None);
        }
        let key = name.strip_prefix("MojikumiTable/").unwrap_or(name);
        let table = styles.mojikumi_tables.iter().find(|t| t.name == key);
        let base = table.map_or(key, |t| if t.based_on.is_empty() { t.name.as_str() } else { t.based_on.as_str() });
        let chinese = match base.trim_start_matches("$ID/") {
            "SimpChineseDefault" | "TradChineseDefault" => true,
            "LineEndAllOneHalfEmEnum" | "kMojikumiDefaultName1" => false,
            _ => return Err(format!("unknown base preset `{base}`")),
        };
        let rows = table.map_or(&[][..], |t| t.overrides.as_slice());
        for r in rows {
            if !valid_class(r.target_class) || !valid_class(r.side_class) {
                return Err(format!("unsupported character classes {}/{}", r.target_class, r.side_class));
            }
            if ![r.minimum, r.desired, r.maximum].iter().all(|v| v.is_finite() && (-1.0..=100.0).contains(v))
                || r.minimum > r.desired
                || r.desired > r.maximum
                || !(0..=9).contains(&r.priority)
            {
                return Err("invalid spacing range or priority".into());
            }
        }
        Ok(Some(Self { chinese, rows }))
    }

    fn override_pair(&self, left: i16, right: i16) -> Option<&MojikumiAki> {
        self.rows
            .iter()
            .rev()
            .find(|r| if r.after { r.target_class == left && r.side_class == right } else { r.target_class == right && r.side_class == left })
    }

    /// Preset spacing restores only blank removed from shaped punctuation bodies.
    /// Explicit custom rows retain their requested dimensions.
    pub fn pair_with_bodies(&self, left: i16, right: i16, after_left: f64, before_right: f64) -> Aki {
        let mut aki = self.pair(left, right);
        if self.override_pair(left, right).is_none() {
            let missing = if matches!(left, 22 | 23) {
                leading(right) - before_right
            } else if right == 22 {
                trailing(left) - after_left
            } else {
                trailing(left).max(leading(right)) - after_left.max(before_right)
            }
            .max(0.0);
            aki.min = (aki.min - missing).max(0.0);
            aki.desired = (aki.desired - missing).max(0.0);
            aki.max = (aki.max - missing).max(0.0);
        }
        aki
    }

    pub fn pair(&self, left: i16, right: i16) -> Aki {
        if let Some(row) = self.override_pair(left, right) {
            return Aki {
                min: row.minimum,
                desired: row.desired,
                max: row.maximum,
                priority: if row.priority == 0 { 10 } else { row.priority as u8 },
                discrete: row.does_not_float,
            };
        }
        // Boundaries: Chinese keeps full punctuation; the half-em Japanese set
        // trims opening punctuation at line start and closing punctuation at end.
        if matches!(left, 22 | 23) {
            return Aki::fixed(if self.chinese { leading(right) } else { 0.0 });
        }
        if right == 22 {
            return Aki::fixed(if self.chinese { trailing(left) } else { 0.0 });
        }
        let punctuation = trailing(left).max(leading(right));
        if punctuation > 0.0 {
            return Aki { min: 0.0, desired: punctuation, max: punctuation, priority: 1, discrete: false };
        }
        let roman = |c| matches!(c, 18 | 25);
        let ideograph = |c| matches!(c, 3 | 7..=9 | 11 | 12 | 24 | 33);
        if (roman(left) && ideograph(right)) || (ideograph(left) && roman(right)) {
            return Aki { min: 0.0, desired: 0.25, max: 0.5, priority: 3, discrete: false };
        }
        if ideograph(left) && ideograph(right) {
            return Aki { min: 0.0, desired: 0.0, max: 0.25, priority: 10, discrete: false };
        }
        Aki::default()
    }
}

pub fn valid_class(c: i16) -> bool {
    matches!(c, 1..=12 | 18 | 21..=33)
}
/// Natural half-body blank before a full-width punctuation character.
pub fn leading(c: i16) -> f64 {
    match c {
        1 | 26 | 27 => 0.5,
        4 | 5 | 32 => 0.25,
        _ => 0.0,
    }
}
pub fn trailing(c: i16) -> f64 {
    match c {
        2 | 6 | 21 | 28..=31 => 0.5,
        4 | 5 | 32 => 0.25,
        _ => 0.0,
    }
}

/// Unicode classification, independent of font availability and localized names.
pub fn class(c: char) -> i16 {
    match c {
        '「' | '『' | '｢' => 26,
        '（' => 27,
        '〈' | '《' | '【' | '〔' | '〖' | '〘' | '［' | '｛' | '‘' | '“' => 1,
        '」' | '』' | '｣' => 28,
        '）' => 29,
        '〉' | '》' | '】' | '〕' | '〗' | '〙' | '］' | '｝' | '’' | '”' => 2,
        '。' | '｡' => 6,
        '、' | '､' => 21,
        '，' => 30,
        '．' => 31,
        '：' | '；' => 32,
        '・' | '·' => 5,
        '！' | '？' => 4,
        '…' | '‥' | '—' | '―' => 7,
        '￥' | '＄' | '£' | '¥' => 8,
        '％' | '‰' | '℃' | '°' => 9,
        '\u{3000}' => 10,
        'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'っ' | 'ゃ' | 'ゅ' | 'ょ' | 'ゎ' | 'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ッ' | 'ャ' | 'ュ' | 'ョ' | 'ヮ'
        | 'ー' | '々' | 'ゝ' | 'ゞ' | 'ヽ' | 'ヾ' => 3,
        '\u{3040}'..='\u{309f}' => 11,
        '\u{30a0}'..='\u{30ff}' | '\u{31f0}'..='\u{31ff}' => 33,
        '０'..='９' => 24,
        '0'..='9' => 25,
        '\u{2e80}'..='\u{a4cf}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}' | '\u{ff01}'..='\u{ff60}' | '\u{20000}'..='\u{323af}' => 12,
        _ => 18,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cjk::MojikumiTable;

    #[test]
    fn overrides_are_directional_and_unknown_tables_are_not_silently_applied() {
        let mut styles = Styles::default();
        styles.mojikumi_tables.push(MojikumiTable {
            name: "Pair".into(),
            based_on: "SimpChineseDefault".into(),
            overrides: vec![MojikumiAki {
                target_class: 12,
                side_class: 18,
                after: false,
                minimum: 0.1,
                desired: 0.2,
                maximum: 0.3,
                priority: 2,
                does_not_float: false,
            }],
        });
        let rule = Rules::resolve(&styles, "MojikumiTable/Pair").unwrap().unwrap();
        assert_eq!(rule.pair(18, 12).desired, 0.2);
        assert_eq!(rule.pair(12, 18).desired, 0.25);
        assert!(Rules::resolve(&styles, "Unknown").is_err());
        assert!(Rules::resolve(&styles, "Nothing").unwrap().is_none());
        styles.mojikumi_tables[0].overrides[0].minimum = 0.4;
        assert!(Rules::resolve(&styles, "MojikumiTable/Pair").is_err());
    }

    #[test]
    fn chinese_and_half_em_boundaries_differ_and_classes_cover_punctuation() {
        let styles = Styles::default();
        let cn = Rules::resolve(&styles, "SimpChineseDefault").unwrap().unwrap();
        let jp = Rules::resolve(&styles, "LineEndAllOneHalfEmEnum").unwrap().unwrap();
        assert_eq!(cn.pair(22, class('（')).desired, 0.5);
        assert_eq!(jp.pair(22, class('（')).desired, 0.0);
        assert_eq!(cn.pair(class('。'), 22).desired, 0.5);
        assert_eq!(jp.pair(class('。'), 22).desired, 0.0);
        assert_ne!(class('１'), class('1'));
        assert_ne!(class('，'), class('、'));
        assert_ne!(class('「'), class('（'));
    }
}
