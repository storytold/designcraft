//! The OpenType features a font offers (its GSUB and GPOS feature lists) and the names it gives
//! its stylistic sets, for the Typography panel to show only what the selected fonts can do.

use std::collections::{BTreeMap, BTreeSet};

use skrifa::MetadataProvider;
use skrifa::raw::TableProvider;
use skrifa::raw::tables::layout::FeatureParams;

use crate::FontFace;

/// What one font's layout tables offer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontFeatures {
    /// The feature tags its GSUB and GPOS tables define, such as `liga` or `ss01`.
    pub tags: BTreeSet<String>,
    /// The name each stylistic set (1 to 20) gives itself, from its feature parameters.
    pub stylistic_sets: BTreeMap<u32, String>,
}

/// The features `face` offers. A face without layout tables offers none.
pub fn font_features(face: &FontFace) -> FontFeatures {
    let mut out = FontFeatures::default();
    let Some(font) = face.skrifa() else { return out };
    let mut lists = Vec::new();
    if let Ok(gsub) = font.gsub()
        && let Ok(list) = gsub.feature_list()
    {
        lists.push(list);
    }
    if let Ok(gpos) = font.gpos()
        && let Ok(list) = gpos.feature_list()
    {
        lists.push(list);
    }
    for list in &lists {
        for record in list.feature_records() {
            let tag = record.feature_tag().to_string();
            let set = tag.strip_prefix("ss").and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=20).contains(n));
            if let Some(set) = set
                && !out.stylistic_sets.contains_key(&set)
                && let Ok(feature) = record.feature(list.offset_data())
                && let Some(Ok(FeatureParams::StylisticSet(params))) = feature.feature_params()
                && let Some(name) = font.localized_strings(params.ui_name_id()).english_or_first().map(|s| s.to_string()).filter(|s| !s.is_empty())
            {
                out.stylistic_sets.insert(set, name);
            }
            out.tags.insert(tag);
        }
    }
    out
}
