use super::values::tint;
use super::{Columns, ColumnsKind, El, Margins, TextFrameOptions, TextWrap, names};

/// Drop-shadow angle (degrees) from IDML offsets (inverse of export's mapping).
/// A `TextWrapPreference` element (of a page item or a placed graphic).
pub(super) fn text_wrap(w: &El) -> TextWrap {
    let model = craft_idml::model::TextWrapPreference::from_el(&crate::xml::to_craft(w));
    let mut tw = TextWrap {
        mode: names::wrap_mode_in(model.mode().unwrap_or("None")),
        invert: model.inverse(),
        side: names::wrap_side_in(model.side().unwrap_or("BothSides")),
        contour: names::contour_type_in(model.contour_type().unwrap_or("SameAsClipping")),
        ..TextWrap::default()
    };
    if let Some(offsets) = model.offsets() {
        tw.offsets = offsets;
    }
    tw
}

pub(super) fn margins_of(m: &craft_idml::model::MarginPreference) -> (Margins, Columns) {
    let margins = Margins {
        top: m.top().unwrap_or(36.0),
        bottom: m.bottom().unwrap_or(36.0),
        inside: m.left().unwrap_or(36.0),
        outside: m.right().unwrap_or(36.0),
    };
    let count = m.column_count().unwrap_or(1.0).clamp(1.0, f64::from(designcraft_doc::MAX_COLUMNS)) as u32;
    let gutter = m.column_gutter().unwrap_or(12.0);
    let positions = m.column_positions(2 * count as usize + 1).and_then(|pos| custom_columns(&pos, count, gutter));
    (margins, Columns { count, gutter, positions })
}

pub(super) fn margins_of_el(m: &El) -> (Margins, Columns) {
    margins_of(&craft_idml::model::MarginPreference::from_el(&crate::xml::to_craft(m)))
}

/// `ColumnsPositions` when the column guides have been dragged off the even split: `2 × count`
/// offsets from the margin box's left edge, valid as [`designcraft_doc::valid_column_positions`]
/// over the box they span. InDesign writes the attribute whether or not they have been moved, so
/// an even set is dropped: `positions` means "custom", and keeping an even copy would freeze the
/// columns against a later change of count or gutter. Anything malformed is the even split; at
/// most one number past `2 × count` is parsed, so a long list costs nothing.
fn custom_columns(pos: &[f64], count: u32, gutter: f64) -> Option<Vec<f64>> {
    let width = *pos.last()?;
    if !designcraft_doc::valid_column_positions(pos, count, width) {
        return None;
    }
    let even = Columns { count, gutter, positions: None }.edges(width);
    // The tolerance only absorbs float noise in InDesign's arithmetic; a dragged guide moves far more.
    pos.iter().zip(&even).any(|(a, b)| (a - b).abs() > 1e-4).then(|| pos.to_vec())
}

/// `columns` with custom positions only when they fit the margin box of a `page_width`-wide page
/// with these margins (the box's width doesn't depend on which side of the spine it is).
pub(super) fn fit_columns(mut columns: Columns, margins: &Margins, page_width: f64) -> Columns {
    let width = (page_width - margins.inside - margins.outside).max(0.0);
    if columns.valid_positions(width).is_none() {
        columns.positions = None;
    }
    columns
}

/// A `BaselineFrameGridRelativeOption` value (`TopOfInsetOfBaselineFrameGridRelativeOption`, or a
/// short form such as `TopOfInset`); unknown values measure from the top of the page.
pub(super) fn frame_grid_relative(v: &str) -> designcraft_doc::FrameGridRelative {
    use designcraft_doc::FrameGridRelative as R;
    let v = v.strip_suffix("OfBaselineFrameGridRelativeOption").unwrap_or(v);
    match v {
        "TopOfMargin" | "TopMargin" => R::TopMargin,
        "TopOfFrame" => R::TopOfFrame,
        "TopOfInset" | "TopInset" => R::TopInset,
        _ => R::TopOfPage,
    }
}

/// Text Frame Options from `TextFramePreference` elements: each attribute from the first that sets it.
pub(super) fn text_frame_options(prefs: &[&El]) -> TextFrameOptions {
    let elements: Vec<_> = prefs.iter().map(|pref| crate::xml::to_craft(pref)).collect();
    let model_elements: Vec<_> = elements.iter().collect();
    let e = craft_idml::model::TextFramePreferences::from_elements(&model_elements);
    let mut o = TextFrameOptions::default();
    if let Some(v) = e.column_count {
        o.columns = v.clamp(1.0, 216.0) as u32;
    }
    if let Some(v) = e.column_gutter {
        o.gutter = v;
    }
    o.columns_kind = if e.fixed_width {
        ColumnsKind::FixedWidth
    } else if e.flexible_width {
        ColumnsKind::FlexibleWidth
    } else {
        ColumnsKind::FixedNumber
    };
    if o.columns_kind != ColumnsKind::FixedNumber
        && let Some(v) = e.fixed_column_width
    {
        o.column_width = v;
    }
    o.balance_columns = e.balance_columns;
    if let Some(inset) = e.inset {
        o.inset = inset;
    }
    if let Some(v) = e.vertical_justification.as_deref() {
        o.vertical_justification = names::vj_in(v.trim());
    }
    if let Some(v) = e.vertical_threshold {
        o.vj_paragraph_spacing_limit = v;
    }
    if let Some(v) = e.first_baseline.as_deref() {
        o.first_baseline = names::first_baseline_in(v.trim());
    }
    if let Some(v) = e.minimum_first_baseline {
        o.first_baseline_min = v;
    }
    o.ignore_wrap = e.ignore_wrap;
    if let Some(v) = e.auto_sizing_type.as_deref() {
        o.auto_size = names::auto_size_in(v.trim());
    }
    if let Some(v) = e.auto_sizing_reference.as_deref() {
        o.auto_size_ref = names::REF_POINTS.iter().position(|p| *p == v.trim()).unwrap_or(1) as u8;
    }
    let size = |v: Option<f64>| v.map(|v| v.clamp(0.0, 10_000.0));
    o.auto_size_use_min_height = e.use_minimum_height;
    o.auto_size_min_height = size(e.minimum_height).unwrap_or(0.0);
    o.auto_size_use_min_width = e.use_minimum_width;
    o.auto_size_min_width = size(e.minimum_width).unwrap_or(0.0);
    o.auto_size_no_line_breaks = e.no_line_breaks;
    // Column rules (the colour is a swatch reference: see `Importer::frame_options`).
    o.column_rule = e.column_rule_override;
    if let Some(v) = e.column_rule_width {
        o.column_rule_weight = v.clamp(0.0, 1000.0);
    }
    if let Some(t) = tint(e.column_rule_tint) {
        o.column_rule_tint = t.clamp(0.0, 1.0);
    }
    for (value, slot) in [
        (e.column_rule_offset, &mut o.column_rule_offset),
        (e.column_rule_top_inset, &mut o.column_rule_top_inset),
        (e.column_rule_bottom_inset, &mut o.column_rule_bottom_inset),
    ] {
        if let Some(x) = value {
            *slot = x.clamp(-1440.0, 1440.0);
        }
    }
    o
}

/// Property lookup over preference levels: the nearest level wins, attributes before children.
pub(super) struct Prefs<'a>(pub(super) &'a [&'a El]);

impl Prefs<'_> {
    pub(super) fn prop(&self, k: &str) -> Option<String> {
        self.0.iter().find_map(|e| e.prop(k))
    }
}

#[cfg(test)]
mod numeric_property_tests {
    use super::*;

    fn pref(attrs: &[(&str, &str)], props: &[(&str, &str)]) -> El {
        let mut e = El::new("TextFramePreference");
        for (key, value) in attrs {
            e.set(key, *value);
        }
        let mut p = El::new("Properties");
        for (key, value) in props {
            p.push(El::new(key).text(*value));
        }
        e.push(p);
        e
    }

    #[test]
    fn numeric_property_forms_import_every_frame_option() {
        let local = pref(
            &[("UseFixedColumnWidth", "true")],
            &[
                ("TextColumnCount", "3"),
                ("TextColumnGutter", "17"),
                ("TextColumnFixedWidth", "123"),
                ("VerticalThreshold", "19"),
                ("MinimumFirstBaselineOffset", "8"),
                ("ColumnRuleStrokeWidth", "2"),
                ("ColumnRuleStrokeTint", "25"),
                ("ColumnRuleOffset", "3"),
                ("ColumnRuleTopInset", "4"),
                ("ColumnRuleBottomInset", "5"),
            ],
        );
        let o = text_frame_options(&[&local]);
        assert_eq!((o.columns, o.gutter, o.column_width), (3, 17., 123.));
        assert_eq!((o.vj_paragraph_spacing_limit, o.first_baseline_min), (19., 8.));
        assert_eq!(
            (o.column_rule_weight, o.column_rule_tint, o.column_rule_offset, o.column_rule_top_inset, o.column_rule_bottom_inset),
            (2., 0.25, 3., 4., 5.)
        );
    }

    #[test]
    fn numeric_property_precedence_is_nearest_level_then_attribute() {
        let local = pref(&[("TextColumnCount", "4")], &[("TextColumnCount", "5"), ("TextColumnGutter", "17")]);
        let child = pref(
            &[("TextColumnGutter", "90"), ("MinimumFirstBaselineOffset", "8")],
            &[("MinimumFirstBaselineOffset", "9"), ("VerticalThreshold", "19")],
        );
        let base = pref(&[("VerticalThreshold", "99")], &[("ColumnRuleStrokeWidth", "2")]);
        let o = text_frame_options(&[&local, &child, &base]);
        assert_eq!((o.columns, o.gutter, o.first_baseline_min, o.vj_paragraph_spacing_limit, o.column_rule_weight), (4, 17., 8., 19., 2.));
        // An explicit invalid value still masks inherited values, as attribute lookup did.
        let invalid = pref(&[("TextColumnCount", "bad")], &[("TextColumnCount", "6")]);
        assert_eq!(text_frame_options(&[&invalid, &local]).columns, TextFrameOptions::default().columns);
    }
}

#[cfg(test)]
mod frame_preference_safety_tests {
    use super::*;

    fn pref(attrs: &[(&str, &str)], props: &[(&str, &str)]) -> El {
        let mut e = El::new("TextFramePreference");
        for (key, value) in attrs {
            e.set(key, *value);
        }
        let mut p = El::new("Properties");
        for (key, value) in props {
            p.push(El::new(key).text(*value));
        }
        e.push(p);
        e
    }

    #[test]
    fn frame_preference_nonfinite_numbers_keep_finite_defaults() {
        const KEYS: [&str; 10] = [
            "TextColumnCount",
            "TextColumnGutter",
            "TextColumnFixedWidth",
            "VerticalThreshold",
            "MinimumFirstBaselineOffset",
            "ColumnRuleStrokeWidth",
            "ColumnRuleStrokeTint",
            "ColumnRuleOffset",
            "ColumnRuleTopInset",
            "ColumnRuleBottomInset",
        ];
        let fallback = pref(&[("UseFixedColumnWidth", "true")], &[]);
        let expected = text_frame_options(&[&fallback]);
        for value in ["NaN", "inf", "-inf", "1e309"] {
            for attribute in [true, false] {
                let values: Vec<_> = KEYS.iter().map(|key| (*key, value)).collect();
                let mut invalid = if attribute { pref(&values, &[]) } else { pref(&[], &values) };
                invalid.set("UseFixedColumnWidth", "true");
                let inherited = pref(&[], &[("TextColumnCount", "7"), ("TextColumnGutter", "9")]);
                assert_eq!(text_frame_options(&[&invalid, &inherited]), expected, "{value}, attribute={attribute}");
            }
        }
    }

    #[test]
    fn frame_preference_property_booleans_enums_and_precedence() {
        let base = pref(
            &[
                ("UseFixedColumnWidth", "true"),
                ("UseFlexibleColumnWidth", "false"),
                ("VerticalBalanceColumns", "false"),
                ("IgnoreWrap", "false"),
                ("ColumnRuleOverride", "false"),
                ("VerticalJustification", "TopAlign"),
                ("FirstBaselineOffset", "AscentOffset"),
                ("AutoSizingType", "Off"),
                ("AutoSizingReferencePoint", "TopCenterPoint"),
            ],
            &[],
        );
        let local = pref(
            &[],
            &[
                ("UseFixedColumnWidth", "false"),
                ("UseFlexibleColumnWidth", "true"),
                ("VerticalBalanceColumns", "true"),
                ("IgnoreWrap", "true"),
                ("ColumnRuleOverride", "true"),
                ("VerticalJustification", "BottomAlign"),
                ("FirstBaselineOffset", "FixedHeight"),
                ("AutoSizingType", "HeightAndWidth"),
                ("AutoSizingReferencePoint", "BottomRightPoint"),
            ],
        );
        let o = text_frame_options(&[&local, &base]);
        assert_eq!(o.columns_kind, ColumnsKind::FlexibleWidth);
        assert!(o.balance_columns && o.ignore_wrap && o.column_rule);
        assert_eq!(o.vertical_justification, designcraft_doc::VerticalJustification::Bottom);
        assert_eq!(o.first_baseline, designcraft_doc::FirstBaseline::Fixed);
        assert_eq!(o.auto_size, designcraft_doc::AutoSize::HeightAndWidth);
        assert_eq!(o.auto_size_ref, 8);
        let fixed = pref(&[], &[("UseFixedColumnWidth", "true")]);
        assert_eq!(text_frame_options(&[&fixed, &local, &base]).columns_kind, ColumnsKind::FixedWidth);
        // At one level an attribute wins over a conflicting property child, for every key.
        let mut same_level = local.clone();
        for (key, value) in &base.attrs {
            same_level.set(key, value);
        }
        assert_eq!(text_frame_options(&[&same_level, &local]), text_frame_options(&[&base]));
        // Malformed nearer values mask ancestors and use the normal per-field fallback.
        let mut malformed = El::new("TextFramePreference");
        for (key, _) in &base.attrs {
            malformed.set(key, "invalid");
        }
        assert_eq!(text_frame_options(&[&malformed, &local]), TextFrameOptions { auto_size_ref: 1, ..Default::default() });
    }
}

#[cfg(test)]
mod page_column_import_safety_tests {
    use super::*;

    #[test]
    fn frame_preference_page_columns_are_bounded_and_finite() {
        for (raw, expected) in [("4000000000", 216), ("1e300", 216), ("-10", 1), ("NaN", 1), ("inf", 1)] {
            let m = El::new("MarginPreference")
                .attr("ColumnCount", raw)
                .attr("ColumnGutter", "NaN")
                .attr("Top", "inf")
                .attr("Bottom", "-inf")
                .attr("Left", "NaN")
                .attr("Right", "1e309");
            let (margins, columns) = margins_of_el(&m);
            assert_eq!(columns.count, expected);
            assert_eq!(columns.gutter, 12.);
            assert_eq!(margins, Margins::uniform(36.));
        }
    }

    /// Hostile `ColumnsPositions` are the even split: never half-read, never a panic, and a long
    /// list is not parsed past what `ColumnCount` allows (#1096).
    #[test]
    fn hostile_column_positions_fall_back_to_the_even_split() {
        let positions = |count: &str, raw: &str| {
            let m = El::new("MarginPreference").attr("ColumnCount", count).attr("ColumnGutter", "20").attr("ColumnsPositions", raw);
            margins_of_el(&m).1.positions
        };
        assert_eq!(positions("2", "0 150 170 450"), Some(vec![0., 150., 170., 450.]));
        let huge = (0..10_000).map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
        for (count, raw) in [
            ("2", "0 215 235 450"),         // the even split
            ("2", "0 150 170"),             // odd
            ("2", "0 450"),                 // too few for the count
            ("2", "0 100 120 200 220 450"), // too many for the count
            ("2", "0 150 90 450"),          // descending
            ("2", "0 NaN 170 450"),
            ("2", "0 inf 170 450"),
            ("2", "0 150 170 1e309"),
            ("2", "0 x 170 450"),
            ("2", "10 150 170 450"), // not from the margin
            ("2", ""),
            ("2", huge.as_str()),
            ("4000000000", huge.as_str()), // the count is capped at 216, so is the list
            ("216", huge.as_str()),
            ("NaN", "0 150 170 450"),
        ] {
            assert_eq!(positions(count, raw), None, "{count}: {}", raw.get(..raw.len().min(40)).unwrap_or(""));
        }
        // The cap itself: 216 one-point columns with no gutter are the even split; one edge moved
        // makes them custom.
        let gapless = |raw: &str| {
            let m = El::new("MarginPreference").attr("ColumnCount", "216").attr("ColumnGutter", "0").attr("ColumnsPositions", raw);
            margins_of_el(&m).1.positions
        };
        let edges = |first_right: &str| {
            (0..432).map(|i| if i == 1 { first_right.to_string() } else { ((i + 1) / 2).to_string() }).collect::<Vec<_>>().join(" ")
        };
        assert_eq!(gapless(&edges("1")), None);
        assert_eq!(gapless(&edges("0.5")).map(|p| p.len()), Some(432));
    }
}

#[cfg(test)]
mod frame_preference_scalar_tests {
    use super::*;

    #[test]
    fn frame_preference_scalar_whitespace() {
        let values = [
            ("UseFixedColumnWidth", "true"),
            ("UseFlexibleColumnWidth", "false"),
            ("VerticalBalanceColumns", "true"),
            ("IgnoreWrap", "true"),
            ("ColumnRuleOverride", "true"),
            ("VerticalJustification", "BottomAlign"),
            ("FirstBaselineOffset", "FixedHeight"),
            ("AutoSizingType", "HeightAndWidth"),
            ("AutoSizingReferencePoint", "BottomRightPoint"),
        ];
        let preference = |properties: bool, padded: bool, fixed: bool| {
            let mut e = El::new("TextFramePreference");
            let mut props = El::new("Properties");
            for (key, raw) in values {
                let raw = match key {
                    "UseFixedColumnWidth" if !fixed => "false",
                    "UseFlexibleColumnWidth" if !fixed => "true",
                    _ => raw,
                };
                let value = if padded { format!("\n  {raw}\t ") } else { raw.to_string() };
                if properties {
                    props.push(El::new(key).text(value));
                } else {
                    e.set(key, value);
                }
            }
            e.push(props);
            e
        };
        for fixed in [false, true] {
            let clean = preference(false, false, fixed);
            let expected = text_frame_options(&[&clean]);
            assert_eq!(expected.columns_kind, if fixed { ColumnsKind::FixedWidth } else { ColumnsKind::FlexibleWidth });
            assert!(expected.balance_columns && expected.ignore_wrap && expected.column_rule);
            assert_eq!(expected.vertical_justification, designcraft_doc::VerticalJustification::Bottom);
            assert_eq!(expected.first_baseline, designcraft_doc::FirstBaseline::Fixed);
            assert_eq!(expected.auto_size, designcraft_doc::AutoSize::HeightAndWidth);
            assert_eq!(expected.auto_size_ref, 8);
            for properties in [false, true] {
                let padded = preference(properties, true, fixed);
                assert_eq!(text_frame_options(&[&padded]), expected);
            }
            // A malformed nearest attribute masks both its own valid child and farther values.
            let mut invalid = preference(true, true, fixed);
            for (key, _) in values {
                invalid.set(key, " invalid ");
            }
            assert_eq!(text_frame_options(&[&invalid, &clean]), TextFrameOptions { auto_size_ref: 1, ..Default::default() });
        }
        // Object identities use the raw property lookup, not scalar whitespace normalization.
        let exact = El::new("TextFramePreference").child(El::new("Properties").child(El::new("ColumnRuleStrokeColor").text(" Color/Named ")));
        assert_eq!(Prefs(&[&exact]).prop("ColumnRuleStrokeColor").as_deref(), Some(" Color/Named "));
    }
}
