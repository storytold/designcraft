//! Text variables (Type → Text Variables): named values inserted into stories as a single
//! character (`U+E100 + index`) and resolved at composition, per page where it matters.

use serde::{Deserialize, Serialize};

use crate::Document;

/// First character of the variable-instance range; instance `i` refers to `doc.text_variables[i]`.
pub const VAR_BASE: u32 = 0xE100;
pub const VAR_MAX: usize = 256;

/// The story character for variable index `i`.
pub fn var_char(i: usize) -> Option<char> {
    (i < VAR_MAX).then(|| char::from_u32(VAR_BASE + i as u32)).flatten()
}

/// The variable index of a story character, if it is a variable instance.
pub fn var_index(c: char) -> Option<usize> {
    let u = c as u32;
    (VAR_BASE..VAR_BASE + VAR_MAX as u32).contains(&u).then(|| (u - VAR_BASE) as usize)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Use {
    #[default]
    FirstOnPage,
    LastOnPage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum VarKind {
    Custom {
        text: String,
    },
    /// The last page number of the document (or of the section).
    LastPageNumber {
        #[serde(default)]
        section: bool,
    },
    ChapterNumber,
    /// Document title (InDesign: file name).
    FileName {
        #[serde(default)]
        extension: bool,
    },
    /// Date formats use `yyyy`, `MM`, `MMMM`, `MMM`, `dd`, `d`, `HH`, `mm`.
    CreationDate {
        format: String,
    },
    ModificationDate {
        format: String,
    },
    OutputDate {
        format: String,
    },
    /// The first/last paragraph (or character-style run) on the page using `style`;
    /// pages without one carry the last value from earlier pages.
    RunningHeader {
        style: String,
        #[serde(default, rename = "use")]
        use_: Use,
        /// `style` names a character style instead of a paragraph style.
        #[serde(default)]
        character: bool,
    },
    /// A variable type that isn't computed here (an image caption, say): it shows the result
    /// recorded in the imported file, which already includes any text before and after.
    Imported {
        /// The file's name for the type, e.g. `LiveCaptionType`.
        variable_type: String,
        #[serde(default)]
        result: String,
        /// The type's settings element as read (name and attributes), written back on export.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        settings_element: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        settings: Vec<(String, String)>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextVariable {
    pub name: String,
    #[serde(flatten)]
    pub kind: VarKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub before: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after: String,
}

impl TextVariable {
    pub fn new(name: &str, kind: VarKind) -> TextVariable {
        TextVariable { name: name.into(), kind, before: String::new(), after: String::new() }
    }
}

/// InDesign's predefined variables.
pub fn defaults() -> Vec<TextVariable> {
    vec![
        TextVariable::new("Chapter Number", VarKind::ChapterNumber),
        TextVariable::new("Creation Date", VarKind::CreationDate { format: "MM/dd/yy".into() }),
        TextVariable::new("File Name", VarKind::FileName { extension: false }),
        TextVariable::new("Last Page Number", VarKind::LastPageNumber { section: false }),
        TextVariable::new("Modification Date", VarKind::ModificationDate { format: "MMMM d, yyyy h:mm aa".into() }),
        TextVariable::new("Output Date", VarKind::OutputDate { format: "MM/dd/yy".into() }),
        TextVariable::new(
            "Running Header",
            VarKind::RunningHeader { style: crate::story::BASIC_PARAGRAPH.into(), use_: Use::FirstOnPage, character: false },
        ),
    ]
}

/// Seconds since the Unix epoch (0 where the platform has no clock).
pub fn now() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// (year, month 1–12, day 1–31) from days since 1970-01-01 (H. Hinnant's public-domain algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Days since 1970-01-01 of a civil date (inverse of [`civil`]).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds of an ISO 8601 timestamp such as `2019-12-16T09:48:02+01:00` (seconds, the
/// time and the zone are optional; no zone means UTC).
pub fn parse_iso_date(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |a: usize, b: usize| -> Option<i64> {
        let t = s.get(a..b)?;
        t.bytes().all(|c| c.is_ascii_digit()).then(|| t.parse().ok()).flatten()
    };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if s.get(4..5) != Some("-") || s.get(7..8) != Some("-") || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let mut secs = days_from_civil(y, mo as u32, d as u32) * 86_400;
    let rest = s.get(10..).unwrap_or("");
    let Some(time) = rest.strip_prefix('T') else { return rest.is_empty().then_some(secs) };
    let tnum = |a: usize, b: usize| -> Option<i64> {
        let t = time.get(a..b)?;
        t.bytes().all(|c| c.is_ascii_digit()).then(|| t.parse().ok()).flatten()
    };
    let (h, mi) = (tnum(0, 2)?, tnum(3, 5)?);
    let mut i = 5;
    let mut sec = 0;
    if time.get(5..6) == Some(":") {
        sec = tnum(6, 8)?;
        i = 8;
        // Fractional seconds are ignored.
        if time.get(8..9) == Some(".") {
            i = 9 + time.get(9..)?.bytes().take_while(u8::is_ascii_digit).count();
        }
    }
    if h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    secs += h * 3600 + mi * 60 + sec;
    let zone = time.get(i..)?;
    match zone.chars().next() {
        None | Some('Z') => Some(secs),
        Some(sign @ ('+' | '-')) => {
            let z = zone.get(1..)?;
            let zh: i64 = z.get(0..2)?.parse().ok()?;
            let zm: i64 = z.get(2..).map(|r| r.trim_start_matches(':')).filter(|r| !r.is_empty()).map_or(Some(0), |r| r.parse().ok())?;
            let off = zh * 3600 + zm * 60;
            Some(if sign == '+' { secs - off } else { secs + off })
        }
        _ => None,
    }
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// Format Unix seconds (UTC) with an InDesign-style pattern.
pub fn format_date(secs: i64, fmt: &str) -> String {
    let (y, mo, d) = civil(secs.div_euclid(86_400));
    let sod = secs.rem_euclid(86_400);
    let (h, mi) = ((sod / 3600) as u32, ((sod % 3600) / 60) as u32);
    let tokens: [(&str, String); 13] = [
        ("yyyy", format!("{y:04}")),
        ("yy", format!("{:02}", y.rem_euclid(100))),
        ("MMMM", MONTHS[(mo - 1) as usize].into()),
        ("MMM", MONTHS[(mo - 1) as usize][..3].into()),
        ("MM", format!("{mo:02}")),
        ("M", mo.to_string()),
        ("dd", format!("{d:02}")),
        ("d", d.to_string()),
        ("HH", format!("{h:02}")),
        ("H", h.to_string()),
        ("h", (if h % 12 == 0 { 12 } else { h % 12 }).to_string()),
        ("mm", format!("{mi:02}")),
        ("aa", (if h < 12 { "AM" } else { "PM" }).into()),
    ];
    let mut out = String::new();
    let mut rest = fmt;
    'outer: while !rest.is_empty() {
        for (t, v) in &tokens {
            if let Some(r) = rest.strip_prefix(t) {
                out.push_str(v);
                rest = r;
                continue 'outer;
            }
        }
        let c = rest.chars().next().unwrap_or(' ');
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

impl Document {
    /// The value of variable `i` on absolute page `page`, except running headers (which need the
    /// composed document; see the compose crate). `None` for unknown indices and running headers.
    pub fn variable_value(&self, i: usize, page: Option<usize>) -> Option<String> {
        let v = self.text_variables.get(i)?;
        let body = match &v.kind {
            VarKind::Custom { text } => text.clone(),
            VarKind::LastPageNumber { section } => {
                let n = self.page_count();
                if n == 0 {
                    return Some(String::new());
                }
                let last = if *section {
                    let p = page.unwrap_or(0);
                    let next = self.sections.iter().map(|s| s.start).filter(|s| *s > p).min().unwrap_or(n);
                    next.saturating_sub(1)
                } else {
                    n - 1
                };
                self.page_name(last)
            }
            VarKind::ChapterNumber => self.settings.chapter_number.max(1).to_string(),
            VarKind::FileName { extension } => {
                if *extension {
                    format!("{}.designcraft", self.title)
                } else {
                    self.title.clone()
                }
            }
            VarKind::CreationDate { format } => format_date(self.created, format),
            VarKind::ModificationDate { format } => format_date(if self.modified > 0 { self.modified } else { self.created }, format),
            VarKind::OutputDate { format } => format_date(now(), format),
            VarKind::RunningHeader { .. } => return None,
            VarKind::Imported { result, .. } => return Some(result.clone()),
        };
        Some(format!("{}{}{}", v.before, body, v.after))
    }

    pub fn variable(&self, name: &str) -> Option<usize> {
        self.text_variables.iter().position(|v| v.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        // 2026-10-01 14:05 UTC
        let t = 1_790_863_500;
        assert_eq!(format_date(t, "yyyy-MM-dd HH:mm"), "2026-10-01 14:05");
        assert_eq!(format_date(t, "MMMM d, yyyy h:mm aa"), "October 1, 2026 2:05 PM");
        assert_eq!(format_date(0, "MM/dd/yy"), "01/01/70");
        assert_eq!(format_date(951_782_400, "yyyy-MM-dd"), "2000-02-29");
    }

    #[test]
    fn iso_dates() {
        assert_eq!(parse_iso_date("2026-10-01T14:05:00Z"), Some(1_790_863_500));
        assert_eq!(parse_iso_date("2026-10-01T16:05:00+02:00"), Some(1_790_863_500));
        assert_eq!(parse_iso_date("2026-10-01T09:05-05:00"), Some(1_790_863_500));
        assert_eq!(parse_iso_date("2000-02-29"), Some(951_782_400));
        assert_eq!(parse_iso_date("2019-12-16T09:48:02.5+01:00").map(|t| format_date(t, "MM/dd/yy HH:mm")), Some("12/16/19 08:48".into()));
        for bad in ["", "2019", "2019-13-01", "2019-12-16T25:00", "2019-12-16T09:48+0x", "２０１９-12-16"] {
            assert_eq!(parse_iso_date(bad), None, "{bad}");
        }
    }

    #[test]
    fn chars() {
        assert_eq!(var_index(var_char(3).unwrap()), Some(3));
        assert_eq!(var_index('\u{E000}'), None);
    }
}
