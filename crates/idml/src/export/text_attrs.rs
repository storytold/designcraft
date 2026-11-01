use super::common::{bool_s, p, pct};
use super::{CHAR_BUILTINS, CharAttrs, El, Ex, ListType, ParaAttrs, SpanColumns, escape_id, names, num, st};

// Optional paragraph values are omitted when inherited from the style.
macro_rules! n {
    ($a:expr, $el:expr, $f:ident, $k:literal) => {
        if let Some(v) = $a.$f {
            $el.set($k, num(v as f64));
        }
    };
    ($a:expr, $el:expr, $f:ident, $k:literal, pct) => {
        if let Some(v) = $a.$f {
            $el.set($k, pct(v as f64));
        }
    };
}
macro_rules! b {
    ($a:expr, $el:expr, $f:ident, $k:literal) => {
        if let Some(v) = $a.$f {
            $el.set($k, bool_s(v));
        }
    };
}

/// The Overprint switches of a paragraph's shading, border and border gap.
fn para_box_overprints(el: &mut El, a: &ParaAttrs) {
    b!(a, el, shading_overprint, "ParagraphShadingOverprint");
    b!(a, el, border_overprint, "ParagraphBorderOverprint");
    b!(a, el, border_gap_overprint, "ParagraphBorderGapOverprint");
}

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
        if let Some(list) = &list {
            let mut l = El::new("AllNestedStyles").attr("type", "list");
            for ns in list {
                let (ty, delim) = names::nested_until_out(&ns.until);
                l = l.child(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("AppliedCharacterStyle", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, &ns.style)))
                        .child(p("Delimiter", ty, delim))
                        .child(p("Repetition", "long", ns.count.to_string()))
                        .child(p("Inclusive", "boolean", bool_s(ns.through))),
                );
            }
            props.push(l);
        }
        if let Some(list) = &a.nested_line_styles {
            let mut l = El::new("AllNestedLineStyles").attr("type", "list");
            for ns in list {
                l = l.child(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("AppliedCharacterStyle", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, &ns.style)))
                        .child(p("LineCount", "long", ns.lines.to_string())),
                );
            }
            props.push(l);
        }
        if let Some(list) = &a.grep_styles {
            let mut l = El::new("AllGREPStyles").attr("type", "list");
            for g in list {
                l = l.child(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("AppliedCharacterStyle", "object", names::style_self("CharacterStyle", CHAR_BUILTINS, &g.style)))
                        .child(p("GrepExpression", "string", g.pattern.clone())),
                );
            }
            props.push(l);
        }
    }

    fn para_alignment_attrs(&self, el: &mut El, a: &ParaAttrs, parent: Option<&str>) {
        if let Some(v) = a.align {
            el.set("Justification", names::align_out(v));
        }
        if let Some(v) = a.direction {
            el.set(
                "ParagraphDirection",
                if v == designcraft_doc::TextDirection::RightToLeft { "RightToLeftDirection" } else { "LeftToRightDirection" },
            );
        }
        if let Some(v) = a.kashidas {
            el.set("Kashidas", if v { "DefaultKashidas" } else { "KashidasOff" });
        }
        if let Some(v) = &a.arabic_justification {
            el.set("ParagraphJustification", v);
        }
        if let Some(Some(v)) = a.paragraph_kashida_width {
            el.set("ParagraphKashidaWidth", num(v));
        }
        n!(a, el, left_indent, "LeftIndent");
        n!(a, el, right_indent, "RightIndent");
        n!(a, el, first_line_indent, "FirstLineIndent");
        n!(a, el, last_line_indent, "LastLineIndent");
        n!(a, el, space_before, "SpaceBefore");
        n!(a, el, space_after, "SpaceAfter");
        match a.same_para_style_spacing {
            Some(Some(v)) => el.set("SameParaStyleSpacing", num(v)),
            // Explicit Ignore: must be written so it still overrides a based-on style's value.
            Some(None) => el.set("SameParaStyleSpacing", "Nothing"),
            None => {}
        }
        n!(a, el, drop_cap_lines, "DropCapLines");
        n!(a, el, drop_cap_chars, "DropCapCharacters");
        if a.drop_cap_align_left.is_some() || a.drop_cap_scale_descenders.is_some() {
            let d = parent.map(|name| self.d.styles.resolve_para_style(name).0).unwrap_or_default();
            let bits = u32::from(a.drop_cap_align_left.unwrap_or(d.drop_cap_align_left))
                | u32::from(a.drop_cap_scale_descenders.unwrap_or(d.drop_cap_scale_descenders)) << 1;
            el.set("DropcapDetail", bits);
        }
        if let Some(g) = a.grid_align {
            // IDML has one attribute for on/off and the reference: a reference set without
            // `grid_align` isn't written.
            let reference = crate::cjk::grid_reference_out(a.grid_reference.unwrap_or_default());
            match g {
                designcraft_doc::GridAlign::None => el.set("GridAlignment", "None"),
                designcraft_doc::GridAlign::AllLines => {
                    el.set("GridAlignment", reference);
                    el.set("GridAlignFirstLineOnly", "false");
                }
                designcraft_doc::GridAlign::FirstLineOnly => {
                    el.set("GridAlignment", reference);
                    el.set("GridAlignFirstLineOnly", "true");
                }
            }
        }
        if let Some(c) = a.composer {
            el.set(
                "Composer",
                match c {
                    designcraft_doc::Composer::Paragraph => "HL Composer",
                    designcraft_doc::Composer::SingleLine => "HL Single",
                    designcraft_doc::Composer::Japanese => "HL Composer J",
                    designcraft_doc::Composer::JapaneseSingleLine => "HL Single J",
                },
            );
        }
    }

    fn para_flow_attrs(&self, el: &mut El, a: &ParaAttrs) {
        b!(a, el, hyphenate, "Hyphenation");
        n!(a, el, hyph_min_word, "HyphenateWordsLongerThan");
        n!(a, el, hyph_after_first, "HyphenateAfterFirst");
        n!(a, el, hyph_before_last, "HyphenateBeforeLast");
        n!(a, el, hyph_limit, "HyphenateLadderLimit");
        n!(a, el, hyph_zone, "HyphenationZone");
        b!(a, el, hyph_capitalized, "HyphenateCapitalizedWords");
        b!(a, el, hyph_last_word, "HyphenateLastWord");
        b!(a, el, hyph_across_column, "HyphenateAcrossColumns");
        if let Some(v) = a.hyph_weight {
            el.set("HyphenWeight", num((v * 10.0).round()));
        }
        n!(a, el, word_space_min, "MinimumWordSpacing", pct);
        n!(a, el, word_space_desired, "DesiredWordSpacing", pct);
        n!(a, el, word_space_max, "MaximumWordSpacing", pct);
        n!(a, el, letter_space_min, "MinimumLetterSpacing", pct);
        n!(a, el, letter_space_desired, "DesiredLetterSpacing", pct);
        n!(a, el, letter_space_max, "MaximumLetterSpacing", pct);
        n!(a, el, glyph_scale_min, "MinimumGlyphScaling", pct);
        n!(a, el, glyph_scale_desired, "DesiredGlyphScaling", pct);
        n!(a, el, glyph_scale_max, "MaximumGlyphScaling", pct);
        n!(a, el, auto_leading, "AutoLeading", pct);
        if let Some(v) = a.single_word_justify {
            el.set("SingleWordJustification", names::align_out(v));
        }
        n!(a, el, keep_with_next, "KeepWithNext");
        b!(a, el, keep_lines_together, "KeepLinesTogether");
        b!(a, el, ignore_optical_margin, "IgnoreEdgeAlignment");
        b!(a, el, keep_all_lines, "KeepAllLinesTogether");
        n!(a, el, keep_first, "KeepFirstLines");
        n!(a, el, keep_last, "KeepLastLines");
        if let Some(v) = a.start_paragraph {
            el.set("StartParagraph", names::start_para_out(v));
        }
    }

    fn para_columns_and_list_attrs(&mut self, el: &mut El, props: &mut Vec<El>, a: &ParaAttrs) {
        if let Some(sc) = a.span_columns {
            match sc {
                SpanColumns::Single => el.set("SpanColumnType", "SingleColumn"),
                SpanColumns::Span(n) | SpanColumns::Split(n) => {
                    el.set("SpanColumnType", if matches!(sc, SpanColumns::Span(_)) { "SpanColumns" } else { "SplitColumns" });
                    props.push(if n == 0 {
                        p("SpanSplitColumnCount", "enumeration", "All")
                    } else {
                        p("SpanSplitColumnCount", "long", n.to_string())
                    });
                }
            }
        }
        n!(a, el, split_inside_gutter, "SplitColumnInsideGutter");
        n!(a, el, split_outside_gutter, "SplitColumnOutsideGutter");
        if let Some(r) = &a.rule_above {
            self.rule(el, props, "RuleAbove", r);
        }
        if let Some(r) = &a.rule_below {
            self.rule(el, props, "RuleBelow", r);
        }
        if let Some(tabs) = &a.tabs {
            let mut tl = El::new("TabList").attr("type", "list");
            for t in tabs {
                tl.push(
                    El::new("ListItem")
                        .attr("type", "record")
                        .child(p("Alignment", "enumeration", names::tab_align_out(t.align)))
                        .child(p("AlignmentCharacter", "string", if t.align_on.is_empty() { ".".to_string() } else { t.align_on.clone() }))
                        .child(p("Leader", "string", t.leader.clone()))
                        .child(p("Position", "unit", num(t.position))),
                );
            }
            props.push(tl);
        }
        if let Some(l) = a.list_type {
            el.set(
                "BulletsAndNumberingListType",
                match l {
                    ListType::None => "NoList",
                    ListType::Bullets => "BulletList",
                    ListType::Numbers => "NumberedList",
                },
            );
        }
        if let Some(n) = a.number_style {
            props.push(p("NumberingFormat", "string", names::numbering_format_out(n)));
        }
        if let Some(x) = &a.number_expression {
            el.set("NumberingExpression", names::list_text_out(x));
        }
        if let Some(x) = &a.list_separator {
            el.set("BulletsTextAfter", names::list_text_out(x));
        }
        if let Some(c) = a.bullet_char.as_deref().and_then(|s| s.chars().next()) {
            props.push(El::new("BulletChar").attr("BulletCharacterType", "UnicodeOnly").attr("BulletCharacterValue", u32::from(c).to_string()));
        }
        if let Some(b) = a.balance_ragged {
            props.push(p("BalanceRaggedLines", "enumeration", if b { "FullyBalanced" } else { "NoBalancing" }));
        }
    }

    fn para_box_corners(el: &mut El, a: &ParaAttrs) {
        const CORNERS: [&str; 4] = ["TopLeft", "TopRight", "BottomRight", "BottomLeft"];
        for (prefix, corners) in [("ParagraphBorder", a.border_corners), ("ParagraphShading", a.shading_corners)] {
            if let Some(cs) = corners {
                for (c, k) in cs.iter().zip(CORNERS) {
                    el.set(&format!("{prefix}{k}CornerOption"), names::corner_out(c.shape));
                    el.set(&format!("{prefix}{k}CornerRadius"), num(c.size));
                }
            }
        }
    }

    fn para_box_attrs(&self, el: &mut El, props: &mut Vec<El>, a: &ParaAttrs) {
        b!(a, el, shading_on, "ParagraphShadingOn");
        if let Some(c) = &a.shading_color {
            props.push(p("ParagraphShadingColor", "object", self.sw(c)));
        }
        n!(a, el, shading_tint, "ParagraphShadingTint", pct);
        para_box_overprints(el, a);
        const SIDES: [&str; 4] = ["Top", "Left", "Bottom", "Right"];
        if let Some(o) = a.shading_offsets {
            for (i, side) in SIDES.iter().enumerate() {
                el.set(&format!("ParagraphShading{side}Offset"), num(o[i]));
            }
        }
        b!(a, el, border_on, "ParagraphBorderOn");
        if let Some(c) = &a.border_color {
            props.push(p("ParagraphBorderColor", "object", self.sw(c)));
        }
        n!(a, el, border_tint, "ParagraphBorderTint", pct);
        if let Some(w) = a.border_weights {
            for (i, side) in SIDES.iter().enumerate() {
                el.set(&format!("ParagraphBorder{side}LineWeight"), num(w[i]));
            }
        }
        if let Some(o) = a.border_offsets {
            for (i, side) in SIDES.iter().enumerate() {
                el.set(&format!("ParagraphBorder{side}Offset"), num(o[i]));
            }
        }
        if let Some(t) = &a.border_type {
            props.push(p("ParagraphBorderType", "object", names::stroke_type_out(t)));
        }
        if let Some(c) = a.border_cap {
            el.set("ParagraphBorderStrokeEndCap", names::cap_out(c));
        }
        if let Some(j) = a.border_join {
            el.set("ParagraphBorderStrokeEndJoin", names::join_out(j));
        }
        Self::para_box_corners(el, a);
        if let Some(w) = a.border_width {
            el.set("ParagraphBorderWidth", names::box_width_out(w));
        }
        if let Some(t) = a.border_top {
            el.set("ParagraphBorderTopOrigin", names::box_top_out(t));
        }
        if let Some(b) = a.border_bottom {
            el.set("ParagraphBorderBottomOrigin", names::box_bottom_out(b));
        }
        b!(a, el, border_display_if_splits, "ParagraphBorderDisplayIfSplits");
        b!(a, el, border_merge, "MergeConsecutiveParaBorders");
        if let Some(c) = &a.border_gap_color {
            props.push(p("ParagraphBorderGapColor", "object", self.sw(c)));
        }
        n!(a, el, border_gap_tint, "ParagraphBorderGapTint", pct);
        if let Some(w) = a.shading_width {
            el.set("ParagraphShadingWidth", names::box_width_out(w));
        }
        if let Some(t) = a.shading_top {
            el.set("ParagraphShadingTopOrigin", names::box_top_out(t));
        }
        if let Some(b) = a.shading_bottom {
            el.set("ParagraphShadingBottomOrigin", names::box_bottom_out(b));
        }
        b!(a, el, shading_clip, "ParagraphShadingClipToFrame");
        b!(a, el, shading_nonprinting, "ParagraphShadingSuppressPrinting");
    }

    fn para_list_attrs(&self, el: &mut El, props: &mut Vec<El>, a: &ParaAttrs) {
        n!(a, el, span_space_before, "SpanColumnMinSpaceBefore");
        n!(a, el, span_space_after, "SpanColumnMinSpaceAfter");
        n!(a, el, list_level, "NumberingLevel");
        for (k, style) in [("BulletsCharacterStyle", &a.bullet_char_style), ("NumberingCharacterStyle", &a.number_char_style)] {
            if let Some(st) = style {
                props.push(p(k, "object", names::style_self("CharacterStyle", CHAR_BUILTINS, st)));
            }
        }
        if let Some(v) = a.bullet_align {
            el.set("BulletsAlignment", names::list_align_out(v));
        }
        if let Some(v) = a.number_align {
            el.set("NumberingAlignment", names::list_align_out(v));
        }
        b!(a, el, restart_numbers, "NumberingApplyRestartPolicy");
        if let Some(level) = a.restart_after_level {
            let (policy, level) = if level == 0 { ("AnyPreviousLevel", 0) } else { ("SpecificPreviousLevel", level) };
            props.push(
                El::new("NumberingRestartPolicies")
                    .attr("RestartPolicy", policy)
                    .attr("LowerLevel", level.to_string())
                    .attr("UpperLevel", level.to_string()),
            );
        }
        if let Some(f) = &a.bullet_font {
            props.push(p("BulletsFont", "string", if f.is_empty() { "$ID/".to_string() } else { f.clone() }));
        }
        if let Some(f) = a.bullet_font_style.as_ref().filter(|f| !f.is_empty()) {
            props.push(p("BulletsFontStyle", "string", f.clone()));
        }
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
        if let Some(n) = &a.list_name {
            el.set(
                "AppliedNumberingList",
                if n.is_empty() { "NumberingList/$ID/[Default]".to_string() } else { format!("NumberingList/{}", escape_id(n)) },
            );
        }
        if let Some(v) = a.start_at {
            match v {
                Some(n) => {
                    el.set("NumberingContinue", "false");
                    el.set("NumberingStartAt", n);
                }
                None => el.set("NumberingContinue", "true"),
            }
        }
        self.para_nested_styles(props, a, parent);
        self.para_alignment_attrs(el, a, parent);
        self.para_flow_attrs(el, a);
        self.para_columns_and_list_attrs(el, props, a);
        self.para_box_attrs(el, props, a);
        self.para_list_attrs(el, props, a);
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
