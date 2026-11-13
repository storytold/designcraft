use super::common::{bool_s, p, pct};
use super::{CHAR_BUILTINS, CharAttrs, El, Ex, ListType, ParaAttrs, SpanColumns, escape_id, names, num, st};

impl<'a> Ex<'a> {
    pub(super) fn char_attrs(&mut self, el: &mut El, props: &mut Vec<El>, a: &CharAttrs) {
        if let Some(v) = a.leading_aki {
            el.set("LeadingAki", num(v.unwrap_or(-1.0)));
        }
        if let Some(v) = a.trailing_aki {
            el.set("TrailingAki", num(v.unwrap_or(-1.0)));
        }
        if let Some(v) = a.tsume {
            el.set("Tsume", num(v * 100.0));
        }
        if let Some(v) = a.jidori {
            el.set("Jidori", v);
        }
        if let Some(v) = a.leading_model {
            el.set("LeadingModel", crate::cjk::leading_out(v));
        }
        if let Some(v) = a.character_alignment {
            el.set("CharacterAlignment", crate::cjk::alignment_out(v));
        }
        self.char_font_attrs(el, props, a);
        if let Some(v) = &a.fill {
            el.set("FillColor", self.sw(v));
        }
        if let Some(v) = a.fill_tint {
            el.set("FillTint", pct(v as f64));
        }
        if let Some(v) = &a.stroke {
            el.set("StrokeColor", self.sw(v));
        }
        if let Some(v) = a.stroke_tint {
            el.set("StrokeTint", pct(v as f64));
        }
        if let Some(v) = a.stroke_weight {
            el.set("StrokeWeight", num(v));
        }
        if let Some(v) = a.stroke_behind_fill {
            el.set("StrokeAlignment", if v { "OutsideAlignment" } else { "CenterAlignment" });
        }
        if let Some(v) = a.capitalization {
            el.set("Capitalization", names::caps_out(v));
        }
        if let Some(v) = a.position {
            el.set("Position", names::position_out(v));
        }
        if let Some(v) = a.underline {
            el.set("Underline", bool_s(v));
        }
        if let Some(v) = a.strikethrough {
            el.set("StrikeThru", bool_s(v));
        }
        if let Some(v) = a.ligatures {
            el.set("Ligatures", bool_s(v));
        }
        self.char_decoration_attrs(el, a);
        if let Some(list) = &a.otf_features {
            for (tag, _, attr) in designcraft_doc::otf::TOGGLES {
                el.set(attr, bool_s(designcraft_doc::otf::is_on(list, tag)));
            }
            let fig = designcraft_doc::otf::figures(list);
            if let Some(f) = designcraft_doc::otf::FIGURES.iter().find(|f| f.0 == fig) {
                el.set("OTFFigureStyle", f.3);
            }
            el.set("OTFStylisticSets", designcraft_doc::otf::stylistic_sets(list).to_string());
        }
        if let Some(v) = &a.glyph_form {
            el.set("GlyphForm", v);
        }
        if let Some(v) = a.no_break {
            el.set("NoBreak", bool_s(v));
        }
        self.char_arabic_attrs(el, a);
        self.char_cjk_layout_attrs(el, props, a);
        self.cjk_settings_chars(el, props, a);
        if let Some(list) = &a.conditions {
            el.set("AppliedConditions", list.iter().map(|c| format!("Condition/{}", escape_id(c))).collect::<Vec<_>>().join(" "));
        }
        if let Some(v) = &a.language {
            el.set("AppliedLanguage", format!("$ID/{v}"));
        }
    }

    fn char_arabic_attrs(&self, el: &mut El, a: &CharAttrs) {
        if let Some(v) = a.digits {
            el.set("DigitsType", names::digits_out(v));
        }
        if let Some(v) = a.character_direction {
            el.set("CharacterDirection", crate::arabic::direction_out(v));
        }
        if let Some(v) = a.allow_kashidas {
            el.set("Kashidas", if v { "DefaultKashidas" } else { "KashidasOff" });
        }
        if let Some(v) = a.diacritic_position {
            el.set("DiacriticPosition", crate::arabic::diacritic_out(v));
        }
        if let Some(v) = a.diacritic_x_offset {
            el.set("XOffsetDiacritic", num(v));
        }
        if let Some(v) = a.diacritic_y_offset {
            el.set("YOffsetDiacritic", num(v));
        }
        if let Some(v) = &a.positional_form {
            el.set("PositionalForm", v);
        }
    }

    fn char_font_attrs(&mut self, el: &mut El, props: &mut Vec<El>, a: &CharAttrs) {
        if let Some(f) = &a.font_family {
            if let Some(composite) = self.d.styles.composite_font(f) {
                props.push(p("AppliedFont", "object", format!("CompositeFont/{}", escape_id(&composite.name))));
            } else {
                props.push(p("AppliedFont", "string", f.clone()));
                let style = a.font_style.clone().unwrap_or_else(|| "Regular".into());
                self.note_font(f, &style);
            }
        }
        if let Some(v) = &a.font_style {
            el.set("FontStyle", v);
        }
        if let Some(v) = a.size {
            el.set("PointSize", num(v));
        }
        if let Some(l) = a.leading {
            let (v, ty) = names::leading_out(l);
            props.push(p("Leading", ty, v));
        }
        if let Some(k) = a.kerning {
            match k {
                designcraft_doc::Kerning::Metrics => el.set("KerningMethod", "$ID/Metrics"),
                designcraft_doc::Kerning::Optical => el.set("KerningMethod", "$ID/Optical"),
                designcraft_doc::Kerning::None => el.set("KerningMethod", "None"),
                designcraft_doc::Kerning::Manual(v) => el.set("KerningValue", num(v)),
            }
        }
        if let Some(v) = a.tracking {
            el.set("Tracking", num(v));
        }
        if let Some(v) = a.h_scale {
            el.set("HorizontalScale", pct(v));
        }
        if let Some(v) = a.v_scale {
            el.set("VerticalScale", pct(v));
        }
        if let Some(v) = a.baseline_shift {
            el.set("BaselineShift", num(v));
        }
        if let Some(v) = a.skew {
            el.set("Skew", num(v));
        }
    }

    fn char_decoration_attrs(&self, el: &mut El, a: &CharAttrs) {
        for (k, w, o, c, t) in [
            ("Underline", a.underline_weight, a.underline_offset, &a.underline_color, a.underline_tint),
            ("StrikeThru", a.strikethrough_weight, a.strikethrough_offset, &a.strikethrough_color, a.strikethrough_tint),
        ] {
            if let Some(w) = w {
                el.set(&format!("{k}Weight"), num(w.unwrap_or(-9999.0)));
            }
            if let Some(o) = o {
                el.set(&format!("{k}Offset"), num(o.unwrap_or(-9999.0)));
            }
            if let Some(c) = c {
                el.set(&format!("{k}Color"), if c.is_empty() { "Text Color".into() } else { self.sw(c) });
            }
            if let Some(t) = t {
                el.set(&format!("{k}Tint"), num(t as f64 * 100.0));
            }
        }
    }

    fn char_cjk_layout_attrs(&mut self, el: &mut El, props: &mut Vec<El>, a: &CharAttrs) {
        if let Some(r) = a.ruby.as_ref() {
            el.set("RubyFlag", if r.is_empty() { "0" } else { "1" });
            el.set("RubyString", r.as_str());
        }
        if let Some((family, style)) = crate::ruby::write(el, props, a, |n| self.sw(n)) {
            self.note_font(&family, &style);
        }
        if let Some(k) = a.kenten {
            el.set("KentenKind", if k { "KentenSesameDot" } else { "None" });
        }
        if let Some(c) = &a.kenten_character
            && !c.is_empty()
        {
            el.set("KentenKind", "Custom");
            el.set("KentenCustomCharacter", c);
        }
        if let Some(v) = a.tate_chu_yoko_x_offset {
            el.set("TatechuyokoXOffset", num(v));
        }
        if let Some(v) = a.tate_chu_yoko_y_offset {
            el.set("TatechuyokoYOffset", num(v));
        }
        if let Some(v) = a.tate_chu_yoko {
            el.set("Tatechuyoko", bool_s(v));
        }
        if let Some(v) = a.warichu {
            el.set("Warichu", bool_s(v));
        }
        if let Some(v) = a.warichu_lines {
            el.set("WarichuLines", v);
        }
        if let Some(v) = a.warichu_size {
            el.set("WarichuSize", num(v));
        }
        if let Some(v) = a.warichu_line_spacing {
            el.set("WarichuLineSpacing", num(v));
        }
        if let Some(v) = a.warichu_alignment {
            el.set("WarichuAlignment", crate::cjk::warichu_align_out(v));
        }
        if let Some(v) = a.warichu_chars_before_break {
            el.set("WarichuCharsBeforeBreak", v);
        }
        if let Some(v) = a.warichu_chars_after_break {
            el.set("WarichuCharsAfterBreak", v);
        }
    }

    fn para_nested_styles(&self, props: &mut Vec<El>, a: &ParaAttrs, parent: Option<&str>) {
        // Nested and GREP styles: property lists of records. The drop cap's character style is the
        // nested style list's leading "through 1 Dropcap" entry (IDML has no attribute for it): a
        // drop cap style set here (`[None]` included) restyles that entry, and a real one is
        // inserted where there is none. Without a list of its own, the inherited list is written
        // when that is what carries the drop cap style, since any list replaces the inherited one.
        let real = |s: &str| !s.is_empty() && s != st::NO_CHAR_STYLE;
        let cap = a.drop_cap_style.as_deref().filter(|s| !s.is_empty());
        let mut list = a.nested_styles.clone();
        if let (Some(cap), None) = (cap, &list) {
            let inherited = parent.map(|n| self.d.styles.resolve_para_style(n).0);
            let inherited_cap = inherited
                .as_ref()
                .is_some_and(|pp| real(&pp.drop_cap_style) || pp.nested_styles.first().is_some_and(|ns| ns.is_drop_cap() && real(&ns.style)));
            if real(cap) || inherited_cap {
                list = Some(inherited.map(|pp| pp.nested_styles).unwrap_or_default());
            }
        }
        if let (Some(cap), Some(list)) = (cap, list.as_mut()) {
            match list.first_mut() {
                Some(ns) if ns.is_drop_cap() => ns.style = cap.to_string(),
                _ if real(cap) => list.insert(0, designcraft_doc::NestedStyle::drop_cap(cap)),
                _ => {}
            }
        }
        let values = craft_idml::model::NestedStyleValues {
            nested: list.map(|list| {
                list.iter()
                    .map(|style| {
                        let (delimiter_type, delimiter) = names::nested_until_out(&style.until);
                        craft_idml::model::NestedStyleValue {
                            character_style: names::style_self("CharacterStyle", CHAR_BUILTINS, &style.style),
                            inclusive: style.through,
                            repetition: style.count,
                            delimiter_type: delimiter_type.into(),
                            delimiter,
                        }
                    })
                    .collect()
            }),
            lines: a.nested_line_styles.as_ref().map(|list| {
                list.iter()
                    .map(|style| craft_idml::model::NestedLineStyleValue {
                        character_style: names::style_self("CharacterStyle", CHAR_BUILTINS, &style.style),
                        line_count: style.lines,
                    })
                    .collect()
            }),
            grep: a.grep_styles.as_ref().map(|list| {
                list.iter()
                    .map(|style| craft_idml::model::GrepStyleValue {
                        character_style: names::style_self("CharacterStyle", CHAR_BUILTINS, &style.style),
                        expression: style.pattern.clone(),
                    })
                    .collect()
            }),
        };
        props.extend(values.to_els().into_iter().map(crate::xml::from_craft));
    }

    fn para_alignment_attrs(&self, el: &mut El, a: &ParaAttrs, parent: Option<&str>) {
        let detail = if a.drop_cap_align_left.is_some() || a.drop_cap_scale_descenders.is_some() {
            let defaults = parent.map(|name| self.d.styles.resolve_para_style(name).0).unwrap_or_default();
            Some(
                u32::from(a.drop_cap_align_left.unwrap_or(defaults.drop_cap_align_left))
                    | u32::from(a.drop_cap_scale_descenders.unwrap_or(defaults.drop_cap_scale_descenders)) << 1,
            )
        } else {
            None
        };
        let (grid_alignment, grid_align_first_line_only) = match a.grid_align {
            Some(designcraft_doc::GridAlign::None) => (Some("None".to_string()), None),
            Some(designcraft_doc::GridAlign::AllLines) => {
                (Some(crate::cjk::grid_reference_out(a.grid_reference.unwrap_or_default()).into()), Some(false))
            }
            Some(designcraft_doc::GridAlign::FirstLineOnly) => {
                (Some(crate::cjk::grid_reference_out(a.grid_reference.unwrap_or_default()).into()), Some(true))
            }
            None => (None, None),
        };
        let values = craft_idml::model::ParagraphAlignmentValues {
            justification: a.align.map(|value| names::align_out(value).into()),
            direction: a.direction.map(|value| {
                if value == designcraft_doc::TextDirection::RightToLeft { "RightToLeftDirection" } else { "LeftToRightDirection" }.into()
            }),
            kashidas: a.kashidas.map(|value| if value { "DefaultKashidas" } else { "KashidasOff" }.into()),
            paragraph_justification: a.arabic_justification.clone(),
            paragraph_kashida_width: a.paragraph_kashida_width.flatten(),
            left_indent: a.left_indent,
            right_indent: a.right_indent,
            first_line_indent: a.first_line_indent,
            last_line_indent: a.last_line_indent,
            space_before: a.space_before,
            space_after: a.space_after,
            same_para_style_spacing: a.same_para_style_spacing,
            drop_cap_lines: a.drop_cap_lines.map(f64::from),
            drop_cap_chars: a.drop_cap_chars.map(f64::from),
            dropcap_detail: detail,
            grid_alignment,
            grid_align_first_line_only,
            composer: a.composer.map(|value| {
                match value {
                    designcraft_doc::Composer::Paragraph => "HL Composer",
                    designcraft_doc::Composer::SingleLine => "HL Single",
                    designcraft_doc::Composer::Japanese => "HL Composer J",
                    designcraft_doc::Composer::JapaneseSingleLine => "HL Single J",
                }
                .into()
            }),
        };
        for (key, value) in values.to_attrs() {
            el.set(&key, value);
        }
    }

    fn para_flow_attrs(&self, el: &mut El, a: &ParaAttrs) {
        let percent = |value: Option<f64>| value.map(|value| value * 100.0);
        let values = craft_idml::model::ParagraphFlowValues {
            hyphenate: a.hyphenate,
            hyphenate_min_word: a.hyph_min_word.map(f64::from),
            hyphenate_after_first: a.hyph_after_first.map(f64::from),
            hyphenate_before_last: a.hyph_before_last.map(f64::from),
            hyphenate_ladder_limit: a.hyph_limit.map(f64::from),
            hyphenation_zone: a.hyph_zone,
            hyphenate_capitalized: a.hyph_capitalized,
            hyphenate_last_word: a.hyph_last_word,
            hyphenate_across_columns: a.hyph_across_column,
            hyphen_weight: a.hyph_weight.map(|value| (value * 10.0).round()),
            word_spacing: [percent(a.word_space_min), percent(a.word_space_desired), percent(a.word_space_max)],
            letter_spacing: [percent(a.letter_space_min), percent(a.letter_space_desired), percent(a.letter_space_max)],
            glyph_scaling: [percent(a.glyph_scale_min), percent(a.glyph_scale_desired), percent(a.glyph_scale_max)],
            auto_leading: percent(a.auto_leading),
            single_word_justification: a.single_word_justify.map(|value| names::align_out(value).into()),
            keep_with_next: a.keep_with_next.map(f64::from),
            keep_lines_together: a.keep_lines_together,
            ignore_edge_alignment: a.ignore_optical_margin,
            keep_all_lines_together: a.keep_all_lines,
            keep_first_lines: a.keep_first.map(f64::from),
            keep_last_lines: a.keep_last.map(f64::from),
            start_paragraph: a.start_paragraph.map(|value| names::start_para_out(value).into()),
        };
        for (key, value) in values.to_attrs() {
            el.set(&key, value);
        }
    }

    fn para_columns_and_list_attrs(&mut self, el: &mut El, props: &mut Vec<El>, a: &ParaAttrs) {
        let span = craft_idml::model::ParagraphSpanValues {
            kind: a.span_columns.map(|span| match span {
                SpanColumns::Single => "SingleColumn".into(),
                SpanColumns::Span(_) => "SpanColumns".into(),
                SpanColumns::Split(_) => "SplitColumns".into(),
            }),
            split_column_count: a.span_columns.and_then(|span| match span {
                SpanColumns::Single => None,
                SpanColumns::Span(count) | SpanColumns::Split(count) => Some(count),
            }),
            inside_gutter: a.split_inside_gutter,
            outside_gutter: a.split_outside_gutter,
            space_before: a.span_space_before,
            space_after: a.span_space_after,
        };
        let (span_attrs, span_properties) = span.to_parts();
        for (key, value) in span_attrs {
            el.set(&key, value);
        }
        props.extend(span_properties.into_iter().map(crate::xml::from_craft));
        if let Some(r) = &a.rule_above {
            self.rule(el, props, "RuleAbove", r);
        }
        if let Some(r) = &a.rule_below {
            self.rule(el, props, "RuleBelow", r);
        }
        if let Some(tabs) = &a.tabs {
            let values: Vec<_> = tabs
                .iter()
                .map(|t| craft_idml::model::TabStopValues {
                    position: t.position,
                    alignment: names::tab_align_out(t.align).into(),
                    leader: t.leader.clone(),
                    alignment_character: if t.align_on.is_empty() { ".".into() } else { t.align_on.clone() },
                })
                .collect();
            props.push(crate::xml::from_craft(craft_idml::model::TabStopValues::to_list(&values)));
        }
        let list = craft_idml::model::ParagraphListValues {
            applied_list: a.list_name.as_deref().map(craft_idml::model::ParagraphListValues::numbering_list_reference),
            list_type: a.list_type.map(|list| {
                match list {
                    ListType::None => "NoList",
                    ListType::Bullets => "BulletList",
                    ListType::Numbers => "NumberedList",
                }
                .into()
            }),
            numbering_format: a.number_style.map(|style| names::numbering_format_out(style).into()),
            expression: a.number_expression.as_ref().map(|text| names::list_text_out(text)),
            separator: a.list_separator.as_ref().map(|text| names::list_text_out(text)),
            bullet_type: a.bullet_char.as_deref().and_then(|text| text.chars().next()).map(|_| "UnicodeOnly".into()),
            bullet_value: a.bullet_char.as_deref().and_then(|text| text.chars().next()).map(|character| u32::from(character).to_string()),
            balance_ragged: a.balance_ragged.map(|balanced| if balanced { "FullyBalanced" } else { "NoBalancing" }.into()),
            level: a.list_level.map(f64::from),
            bullet_character_style: a.bullet_char_style.as_ref().map(|style| names::style_self("CharacterStyle", CHAR_BUILTINS, style)),
            numbering_character_style: a.number_char_style.as_ref().map(|style| names::style_self("CharacterStyle", CHAR_BUILTINS, style)),
            bullet_alignment: a.bullet_align.map(|alignment| names::list_align_out(alignment).into()),
            numbering_alignment: a.number_align.map(|alignment| names::list_align_out(alignment).into()),
            restart_numbers: a.restart_numbers,
            restart_after_level: a.restart_after_level,
            restart_after_specific_level: a.restart_after_level.is_some_and(|level| level != 0),
            numbering_continue: a.start_at.map(|start_at| start_at.is_none()),
            numbering_start_at: a.start_at.flatten().map(f64::from),
            bullet_font: a.bullet_font.clone(),
            bullet_font_style: a.bullet_font_style.clone(),
        };
        for (key, value) in list.to_attrs() {
            el.set(&key, value);
        }
        props.extend(list.to_els().into_iter().map(crate::xml::from_craft));
    }

    fn para_box_attrs(&self, el: &mut El, props: &mut Vec<El>, a: &ParaAttrs) {
        let corners = |values: Option<[designcraft_geom::corners::Corner; 4]>| {
            values.map(|corners| corners.map(|corner| (names::corner_out(corner.shape).to_string(), corner.size)))
        };
        let values = craft_idml::model::ParagraphBoxValues {
            shading_on: a.shading_on,
            shading_color: a.shading_color.as_ref().map(|color| self.sw(color)),
            shading_tint: a.shading_tint.map(|tint| f64::from(tint) * 100.0),
            shading_offsets: a.shading_offsets,
            shading_overprint: a.shading_overprint,
            border_on: a.border_on,
            border_color: a.border_color.as_ref().map(|color| self.sw(color)),
            border_tint: a.border_tint.map(|tint| f64::from(tint) * 100.0),
            border_weights: a.border_weights,
            border_offsets: a.border_offsets,
            border_type: a.border_type.as_ref().map(names::stroke_type_out),
            border_cap: a.border_cap.map(|cap| names::cap_out(cap).to_string()),
            border_join: a.border_join.map(|join| names::join_out(join).to_string()),
            border_corners: corners(a.border_corners),
            border_width: a.border_width.map(|width| names::box_width_out(width).to_string()),
            border_top: a.border_top.map(|origin| names::box_top_out(origin).to_string()),
            border_bottom: a.border_bottom.map(|origin| names::box_bottom_out(origin).to_string()),
            border_display_if_splits: a.border_display_if_splits,
            border_merge: a.border_merge,
            border_gap_color: a.border_gap_color.as_ref().map(|color| self.sw(color)),
            border_gap_tint: a.border_gap_tint.map(|tint| f64::from(tint) * 100.0),
            border_overprint: a.border_overprint,
            border_gap_overprint: a.border_gap_overprint,
            shading_width: a.shading_width.map(|width| names::box_width_out(width).to_string()),
            shading_top: a.shading_top.map(|origin| names::box_top_out(origin).to_string()),
            shading_bottom: a.shading_bottom.map(|origin| names::box_bottom_out(origin).to_string()),
            shading_clip: a.shading_clip,
            shading_nonprinting: a.shading_nonprinting,
            shading_corners: corners(a.shading_corners),
        };
        let (attributes, properties) = values.to_parts();
        for (key, value) in attributes {
            el.set(&key, value);
        }
        props.extend(properties.into_iter().map(crate::xml::from_craft));
    }

    /// Write paragraph attributes `a`, set over the resolved paragraph style `parent` (`None` for the
    /// root style).
    pub(super) fn para_attrs(&mut self, el: &mut El, props: &mut Vec<El>, a: &ParaAttrs, parent: Option<&str>) {
        if let Some(v) = &a.mojikumi {
            props.push(p("Mojikumi", if v.starts_with("MojikumiTable/") { "object" } else { "enumeration" }, v));
        }
        if let Some(v) = &a.kinsoku_type {
            el.set("KinsokuType", v);
        }
        if let Some(v) = a.kinsoku_hang {
            el.set(
                "KinsokuHangType",
                match v {
                    designcraft_doc::cjk::KinsokuHang::None => "None",
                    designcraft_doc::cjk::KinsokuHang::Regular => "KinsokuHangRegular",
                    designcraft_doc::cjk::KinsokuHang::Force => "KinsokuHangForce",
                },
            );
        }
        if let Some(v) = a.bunri_kinshi {
            el.set("BunriKinshi", bool_s(v));
        }
        if let Some(v) = a.rensuuji {
            el.set("Rensuuji", bool_s(v));
        }
        if let Some(v) = a.treat_ideographic_space_as_space {
            el.set("TreatIdeographicSpaceAsSpace", bool_s(v));
        }
        cjk_settings_para(el, a);
        if let Some(Some(k)) = &a.kinsoku {
            props.push(p(
                "KinsokuSet",
                if k.name.is_empty() { "enumeration" } else { "object" },
                if k.name.is_empty() { "Nothing".into() } else { format!("KinsokuTable/{}", escape_id(&k.name)) },
            ));
        }
        self.para_nested_styles(props, a, parent);
        self.para_alignment_attrs(el, a, parent);
        self.para_flow_attrs(el, a);
        self.para_columns_and_list_attrs(el, props, a);
        self.para_box_attrs(el, props, a);
    }
}

/// Auto tate-chu-yoko, Japanese composition and grid settings of a paragraph range or style.
fn cjk_settings_para(el: &mut El, a: &ParaAttrs) {
    if let Some(v) = a.auto_tcy {
        el.set("AutoTcy", v);
    }
    if let Some(v) = a.auto_tcy_include_roman {
        el.set("AutoTcyIncludeRoman", bool_s(v));
    }
    if let Some(v) = a.rotate_roman {
        el.set("RotateSingleByteCharacters", bool_s(v));
    }
    if let Some(v) = a.roman_word_break {
        el.set("AllowArbitraryHyphenation", bool_s(v));
    }
    if let Some(v) = a.grid_gyoudori {
        el.set("GridGyoudori", v);
    }
    if let Some(v) = a.paragraph_gyoudori {
        el.set("ParagraphGyoudori", bool_s(v));
    }
}

impl Ex<'_> {
    /// Kenten Settings, Kenten Color and Shatai of a character range or style.
    fn cjk_settings_chars(&self, el: &mut El, props: &mut Vec<El>, a: &CharAttrs) {
        use designcraft_doc::cjk_settings::KentenKind;
        if let Some(k) = a.kenten_kind
            && a.kenten != Some(false)
        {
            el.set("KentenKind", crate::cjk::kenten_kind_out(k));
            if k != KentenKind::Custom {
                el.attrs.retain(|(n, _)| n != "KentenCustomCharacter");
            } else if let Some(c) = &a.kenten_character {
                el.set("KentenCustomCharacter", c);
            }
        }
        if let Some(f) = &a.kenten_font {
            props.push(p("KentenFont", "string", if f.is_empty() { "$ID/".to_string() } else { f.clone() }));
        }
        if let Some(st) = &a.kenten_font_style {
            props.push(if st.is_empty() { p("KentenFontStyle", "enumeration", "Nothing") } else { p("KentenFontStyle", "string", st.clone()) });
        }
        if let Some(Some(v)) = a.kenten_size {
            el.set("KentenFontSize", num(v));
        }
        if let Some(v) = a.kenten_x_scale {
            el.set("KentenXScale", num(v * 100.0));
        }
        if let Some(v) = a.kenten_y_scale {
            el.set("KentenYScale", num(v * 100.0));
        }
        if let Some(v) = a.kenten_distance {
            el.set("KentenPlacement", num(v));
        }
        if let Some(v) = a.kenten_position {
            el.set("KentenPosition", crate::cjk::kenten_position_out(v));
        }
        if let Some(v) = a.kenten_alignment {
            el.set("KentenAlignment", crate::cjk::kenten_alignment_out(v));
        }
        if let Some(v) = &a.kenten_character_set {
            el.set("KentenCharacterSet", v);
        }
        // The text's own colour is the string "Text Color" (a property); a swatch is a reference.
        for (key, color) in [("KentenFillColor", &a.kenten_fill), ("KentenStrokeColor", &a.kenten_stroke)] {
            match color {
                Some(c) if c.is_empty() => props.push(p(key, "string", "Text Color")),
                Some(c) => el.set(key, self.sw(c)),
                None => {}
            }
        }
        for (key, tint) in [("KentenTint", a.kenten_fill_tint), ("KentenStrokeTint", a.kenten_stroke_tint)] {
            if let Some(t) = tint {
                el.set(key, t.map_or_else(|| "-1".to_string(), |t| num(f64::from(t) * 100.0)));
            }
        }
        if let Some(w) = a.kenten_stroke_weight {
            el.set("KentenWeight", w.map_or_else(|| "-1".to_string(), num));
        }
        if let Some(v) = a.kenten_overprint_fill {
            el.set("KentenOverprintFill", crate::cjk::overprint_out(v));
        }
        if let Some(v) = a.kenten_overprint_stroke {
            el.set("KentenOverprintStroke", crate::cjk::overprint_out(v));
        }
        self.shatai_chars(el, a);
    }

    fn shatai_chars(&self, el: &mut El, a: &CharAttrs) {
        if let Some(v) = a.shatai_magnification {
            el.set("ShataiMagnification", num(v));
        }
        if let Some(v) = a.shatai_angle {
            el.set("ShataiDegreeAngle", num(v * crate::cjk::SHATAI_ANGLE_UNITS));
        }
        if let Some(v) = a.shatai_adjust_rotation {
            el.set("ShataiAdjustRotation", bool_s(v));
        }
        if let Some(v) = a.shatai_adjust_tsume {
            el.set("ShataiAdjustTsume", bool_s(v));
        }
    }
}
