use super::{Deserialize, Serialize};

/// A stream in spine-to-outside order. An omitted series is the frame's main story.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InsetStream {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series: Option<u64>,
    #[serde(default = "one")]
    pub ratio: f64,
}
fn one() -> f64 {
    1.0
}
fn gutter() -> f64 {
    12.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InsetColumns {
    pub streams: Vec<InsetStream>,
    /// Add a catchword below each stream that continues on a later page.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub catchwords: bool,
    #[serde(default = "gutter")]
    pub gutter: f64,
    #[serde(default)]
    pub main_start_lines: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_left_inset: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_right_inset: Option<f64>,
}
impl InsetColumns {
    pub fn main(&self) -> Option<usize> {
        self.streams.iter().position(|s| s.series.is_none())
    }
    /// Reject malformed native/control input before allocating composition trials.
    pub fn valid(&self) -> bool {
        !self.streams.is_empty()
            && self.streams.len() <= 64
            && self.streams.iter().filter(|s| s.series.is_none()).count() == 1
            && self.streams.iter().all(|s| s.ratio.is_finite() && s.ratio > 0.0)
            && self.streams.iter().map(|s| s.ratio).sum::<f64>().is_finite()
            && self.gutter.is_finite()
            && (0.0..=15552.0).contains(&self.gutter)
            && self.main_start_lines <= 65536
            && [self.main_left_inset, self.main_right_inset].into_iter().flatten().all(|v| v.is_finite() && (0.0..=15552.0).contains(&v))
            && self.streams.iter().enumerate().all(|(i, s)| s.series.is_none() || !self.streams.iter().take(i).any(|p| p.series == s.series))
    }
}
