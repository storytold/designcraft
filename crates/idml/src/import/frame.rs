use super::values::tint;
use super::{Columns, ColumnsKind, El, Margins, TextFrameOptions, TextWrap, names};

/// Drop-shadow angle (degrees) from IDML offsets (inverse of export's mapping).
/// A `TextWrapPreference` element (of a page item or a placed graphic).
pub(super) fn text_wrap(w: &El) -> TextWrap {
    let mut tw = TextWrap {
        mode: names::wrap_mode_in(w.get("TextWrapMode").unwrap_or("None")),
        invert: w.get("Inverse") == Some("true"),
        side: names::wrap_side_in(w.get("TextWrapSide").unwrap_or("BothSides")),
        contour: names::contour_type_in(w.find("ContourOption").and_then(|c| c.get("ContourType")).unwrap_or("SameAsClipping")),
        ..TextWrap::default()
    };
    if let Some(o) = w.prop_el("TextWrapOffset") {
        tw.offsets = [o.num("Top").unwrap_or(0.0), o.num("Left").unwrap_or(0.0), o.num("Bottom").unwrap_or(0.0), o.num("Right").unwrap_or(0.0)];
    }
    tw
}

pub(super) fn margins_of(m: &El) -> (Margins, Columns) {
    let g = |k: &str, d: f64| m.num(k).filter(|v| v.is_finite()).unwrap_or(d);
    let margins = Margins { top: g("Top", 36.0), bottom: g("Bottom", 36.0), inside: g("Left", 36.0), outside: g("Right", 36.0) };
    let count = g("ColumnCount", 1.0).clamp(1.0, f64::from(designcraft_doc::MAX_COLUMNS)) as u32;
    let gutter = g("ColumnGutter", 12.0);
    let positions = m.get("ColumnsPositions").and_then(|s| custom_columns(s, count, gutter));
    (margins, Columns { count, gutter, positions })
}

/// `ColumnsPositions` when the column guides have been dragged off the even split: `2 × count`
/// offsets from the margin box's left edge, valid as [`designcraft_doc::valid_column_positions`]
/// over the box they span. InDesign writes the attribute whether or not they have been moved, so
/// an even set is dropped: `positions` means "custom", and keeping an even copy would freeze the
/// columns against a later change of count or gutter. Anything malformed is the even split; at
/// most one number past `2 × count` is parsed, so a long list costs nothing.
fn custom_columns(s: &str, count: u32, gutter: f64) -> Option<Vec<f64>> {
    let want = 2 * count as usize;
    let pos = s.split_whitespace().take(want + 1).map(|v| v.parse::<f64>().ok()).collect::<Option<Vec<f64>>>()?;
    let width = *pos.last()?;
    if !designcraft_doc::valid_column_positions(&pos, count, width) {
        return None;
    }
    let even = Columns { count, gutter, positions: None }.edges(width);
    // The tolerance only absorbs float noise in InDesign's arithmetic; a dragged guide moves far more.
    pos.iter().zip(&even).any(|(a, b)| (a - b).abs() > 1e-4).then_some(pos)
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
    let e = Prefs(prefs);
    let mut o = TextFrameOptions::default();
    if let Some(v) = e.num("TextColumnCount") {
        o.columns = v.clamp(1.0, 216.0) as u32;
    }
    if let Some(v) = e.num("TextColumnGutter") {
        o.gutter = v;
    }
    o.columns_kind = if e.prop("UseFixedColumnWidth").as_deref().map(str::trim) == Some("true") {
        ColumnsKind::FixedWidth
    } else if e.prop("UseFlexibleColumnWidth").as_deref().map(str::trim) == Some("true") {
        ColumnsKind::FlexibleWidth
    } else {
        ColumnsKind::FixedNumber
    };
    if o.columns_kind != ColumnsKind::FixedNumber
        && let Some(v) = e.num("TextColumnFixedWidth")
    {
        o.column_width = v;
    }
    o.balance_columns = e.prop("VerticalBalanceColumns").as_deref().map(str::trim) == Some("true");
    if let Some(p) = prefs.iter().find(|p| p.prop("InsetSpacing").is_some())
        && let Some(inset) = inset_spacing(p)
    {
        o.inset = inset;
    }
    if let Some(v) = e.prop("VerticalJustification") {
        o.vertical_justification = names::vj_in(v.trim());
    }
    if let Some(v) = e.num("VerticalThreshold") {
        o.vj_paragraph_spacing_limit = v;
    }
    if let Some(v) = e.prop("FirstBaselineOffset") {
        o.first_baseline = names::first_baseline_in(v.trim());
    }
    if let Some(v) = e.num("MinimumFirstBaselineOffset") {
        o.first_baseline_min = v;
    }
    o.ignore_wrap = e.prop("IgnoreWrap").as_deref().map(str::trim) == Some("true");
    if let Some(v) = e.prop("AutoSizingType") {
        o.auto_size = names::auto_size_in(v.trim());
    }
    if let Some(v) = e.prop("AutoSizingReferencePoint") {
        o.auto_size_ref = names::REF_POINTS.iter().position(|p| *p == v.trim()).unwrap_or(1) as u8;
    }
    let size = |k: &str| e.num(k).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 10_000.0));
    o.auto_size_use_min_height = e.prop("UseMinimumHeightForAutoSizing").as_deref().map(str::trim) == Some("true");
    o.auto_size_min_height = size("MinimumHeightForAutoSizing").unwrap_or(0.0);
    o.auto_size_use_min_width = e.prop("UseMinimumWidthForAutoSizing").as_deref().map(str::trim) == Some("true");
    o.auto_size_min_width = size("MinimumWidthForAutoSizing").unwrap_or(0.0);
    o.auto_size_no_line_breaks = e.prop("UseNoLineBreaksForAutoSizing").as_deref().map(str::trim) == Some("true");
    // Column rules (the colour is a swatch reference: see `Importer::frame_options`).
    o.column_rule = e.prop("ColumnRuleOverride").as_deref().map(str::trim) == Some("true");
    let finite = |k: &str| e.num(k).filter(|v| v.is_finite());
    if let Some(v) = finite("ColumnRuleStrokeWidth") {
        o.column_rule_weight = v.clamp(0.0, 1000.0);
    }
    if let Some(t) = tint(finite("ColumnRuleStrokeTint")) {
        o.column_rule_tint = t.clamp(0.0, 1.0);
    }
    for (k, v) in [
        ("ColumnRuleOffset", &mut o.column_rule_offset),
        ("ColumnRuleTopInset", &mut o.column_rule_top_inset),
        ("ColumnRuleBottomInset", &mut o.column_rule_bottom_inset),
    ] {
        if let Some(x) = finite(k) {
            *v = x.clamp(-1440.0, 1440.0);
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
    fn num(&self, k: &str) -> Option<f64> {
        // The nearest preference wins; within it, attributes precede property children.
        self.prop(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    }
}

/// A scalar applies to every edge; a list is top, left, bottom, right. Keep the
/// existing property-element precedence over the legacy scalar attribute, even
/// when the property is malformed. Finite negative insets remain supported.
fn inset_spacing(e: &El) -> Option<[f64; 4]> {
    let number = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    let Some(property) = e.prop_el("InsetSpacing") else {
        return e.get("InsetSpacing").and_then(number).map(|v| [v; 4]);
    };
    let mut items = property.elements();
    if property.get("type") == Some("list") || property.find("ListItem").is_some() {
        let mut inset = [0.0; 4];
        for edge in &mut inset {
            let item = items.next()?;
            if item.local() != "ListItem" || item.elements().next().is_some() {
                return None;
            }
            *edge = number(&item.text_content())?;
        }
        items.next().is_none().then_some(inset)
    } else if items.next().is_none() {
        number(&property.text_content()).map(|v| [v; 4])
    } else {
        None
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
            let (margins, columns) = margins_of(&m);
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
            margins_of(&m).1.positions
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
            margins_of(&m).1.positions
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
