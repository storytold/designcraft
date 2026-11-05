use super::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TabAlign {
    #[default]
    Left,
    Center,
    Right,
    /// Align on a character (default `.`).
    Char,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabStop {
    /// A file that held a non-finite position (written as `null`) reads it back as NaN, which
    /// [`TabStop::sanitized_list`] drops.
    #[serde(deserialize_with = "position_or_nan")]
    pub position: f64,
    #[serde(default)]
    pub align: TabAlign,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub leader: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub align_on: String,
}

/// Most stops a paragraph holds.
pub const MAX_TAB_STOPS: usize = 100;

/// Furthest stop position: 216 in, the largest page.
pub const MAX_TAB_POSITION: f64 = 15_552.0;

/// Longest leader, in characters.
pub const MAX_TAB_LEADER: usize = 8;

/// Stops closer than this share a position.
pub const SAME_TAB_POSITION: f64 = 0.01;

/// Smallest type size the engine accepts, in points.
pub const MIN_TYPE_SIZE: f64 = 0.1;

/// Largest type size the engine accepts, in points, as InDesign's own field allows.
pub const MAX_TYPE_SIZE: f64 = 1296.0;

/// Largest measure an imported indent, spacing or offset can carry, in points: the furthest tab
/// stop, which is the widest page.
pub const MAX_MEASURE: f64 = MAX_TAB_POSITION;

/// A type size read from a file, or `None` when it is not a size the engine can mean.
///
/// A file can hold any number, including `NaN`, `inf`, `0`, a negative or `1e300`. Every importer
/// had its own spelling of this test, or only checked that the value was finite -- so tagged text
/// took `<cSize:1e300>` and set a 1e300 pt size on the text (#1593). A size outside the range is
/// dropped rather than clamped, so the text keeps the size its style gives it.
pub fn imported_type_size(pt: f64) -> Option<f64> {
    (pt.is_finite() && (MIN_TYPE_SIZE..=MAX_TYPE_SIZE).contains(&pt)).then_some(pt)
}

/// An indent, spacing or offset read from a file, clamped to ±[`MAX_MEASURE`]; `None` when the
/// value is not finite.
///
/// Unlike a type size this is clamped rather than dropped: a file that says "indent everything"
/// still indents to the widest page the engine has, where dropping the value would quietly
/// restore the style's own indent and hide what the file asked for (#1593).
pub fn imported_measure(pt: f64) -> Option<f64> {
    pt.is_finite().then(|| pt.clamp(-MAX_MEASURE, MAX_MEASURE))
}

impl TabStop {
    /// A finite position clamped to `0..=MAX_TAB_POSITION`.
    pub fn clamp_position(v: f64) -> Result<f64, String> {
        if v.is_finite() { Ok(v.clamp(0.0, MAX_TAB_POSITION)) } else { Err("position must be a finite number".into()) }
    }

    /// A leader: up to [`MAX_TAB_LEADER`] characters, no control characters.
    pub fn check_leader(v: &str) -> Result<(), String> {
        if v.chars().count() > MAX_TAB_LEADER || v.chars().any(char::is_control) {
            return Err(format!("`leader`: at most {MAX_TAB_LEADER} printable characters"));
        }
        Ok(())
    }

    /// An align-on character: one printable character or none (a decimal point).
    pub fn check_align_on(v: &str) -> Result<(), String> {
        if v.chars().count() > 1 || v.chars().any(char::is_control) {
            return Err("`alignOn`: one printable character".into());
        }
        Ok(())
    }

    /// A tab-stop list as the Tabs commands keep it: at most [`MAX_TAB_STOPS`] stops, positions
    /// clamped, leaders and align-on characters checked, a char stop aligning on `.` unless it
    /// names a character, sorted by position, and one stop per position (the later one wins).
    pub fn checked_list(mut tabs: Vec<TabStop>) -> Result<Vec<TabStop>, String> {
        if tabs.len() > MAX_TAB_STOPS {
            return Err(format!("a paragraph holds at most {MAX_TAB_STOPS} tab stops"));
        }
        for t in &mut tabs {
            t.position = Self::clamp_position(t.position)?;
            Self::check_leader(&t.leader)?;
            Self::check_align_on(&t.align_on)?;
        }
        Ok(Self::normalized(tabs))
    }

    /// A tab-stop list read from a file, repaired to what [`checked_list`](Self::checked_list)
    /// accepts: stops with a non-finite position dropped, positions clamped, control characters
    /// removed from leaders and align-on characters and both cut to their limits, sorted, one stop
    /// per position (the later one wins), and the first [`MAX_TAB_STOPS`] by position kept.
    pub fn sanitized_list(tabs: Vec<TabStop>) -> Vec<TabStop> {
        let tabs = tabs
            .into_iter()
            .filter_map(|mut t| {
                t.position = Self::clamp_position(t.position).ok()?;
                if Self::check_leader(&t.leader).is_err() {
                    t.leader = t.leader.chars().filter(|c| !c.is_control()).take(MAX_TAB_LEADER).collect();
                }
                if Self::check_align_on(&t.align_on).is_err() {
                    t.align_on = t.align_on.chars().filter(|c| !c.is_control()).take(1).collect();
                }
                Some(t)
            })
            .collect();
        let mut out = Self::normalized(tabs);
        out.truncate(MAX_TAB_STOPS);
        out
    }

    /// Finite, clamped stops sorted by position, one per position: of stops closer than
    /// [`SAME_TAB_POSITION`], the later one in `tabs` wins. A char stop with no character aligns
    /// on `.`.
    fn normalized(tabs: Vec<TabStop>) -> Vec<TabStop> {
        let mut stops: Vec<(usize, TabStop)> = tabs.into_iter().enumerate().collect();
        stops.sort_by(|a, b| a.1.position.total_cmp(&b.1.position));
        let mut out: Vec<(usize, TabStop)> = Vec::with_capacity(stops.len());
        for (i, mut t) in stops {
            if t.align == TabAlign::Char && t.align_on.is_empty() {
                t.align_on = ".".into();
            }
            match out.last_mut() {
                Some(last) if t.position - last.1.position < SAME_TAB_POSITION => {
                    if i > last.0 {
                        *last = (i, t);
                    }
                }
                _ => out.push((i, t)),
            }
        }
        out.into_iter().map(|(_, t)| t).collect()
    }
}

/// A stop's position; `null` (how JSON writes a non-finite number) reads as NaN.
fn position_or_nan<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN))
}

/// A paragraph's tab-stop list in either attribute form, as read from a file.
pub(super) trait FileTabs: Sized {
    fn sanitized(self) -> Self;
}

impl FileTabs for Vec<TabStop> {
    fn sanitized(self) -> Self {
        TabStop::sanitized_list(self)
    }
}

impl FileTabs for Option<Vec<TabStop>> {
    fn sanitized(self) -> Self {
        self.map(TabStop::sanitized_list)
    }
}

/// Deserializes a `tabs` field through [`TabStop::sanitized_list`]: a saved document's tab lists
/// get the limits the Tabs commands keep.
pub(super) fn sanitized_tabs<'de, D: serde::Deserializer<'de>, T: Deserialize<'de> + FileTabs>(d: D) -> Result<T, D::Error> {
    T::deserialize(d).map(FileTabs::sanitized)
}
