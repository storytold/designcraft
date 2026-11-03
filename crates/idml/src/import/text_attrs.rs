use super::object_style::corners;
use super::story::MAX_STORY_NESTING;
use super::values::tint;
use super::{
    Arc, CharAttrs, El, GridAlign, HashSet, IdmlError, Importer, Kerning, ListType, ParaAttrs, Result, SpanColumns, Story, Styles, TabStop, names,
    st, unescape_id,
};

/// Whether sanitizing a file's tab list dropped, moved or shortened a stop. Sorting, and the `.`
/// a char stop with no character aligns on, change nothing.
fn tab_list_repaired(read: &[TabStop], sanitized: &[TabStop]) -> bool {
    let kept = |t: &TabStop| {
        sanitized.iter().any(|o| {
            o.position == t.position
                && o.leader == t.leader
                && (o.align_on == t.align_on || (t.align_on.is_empty() && o.align == designcraft_doc::TabAlign::Char))
        })
    };
    read.len() != sanitized.len() || !read.iter().all(kept)
}

pub(super) fn mojikumi_row(e: &El) -> Result<designcraft_doc::cjk::MojikumiAki> {
    let number = |key: &str| -> Result<f64> {
        let Some(raw) = e.prop(key) else {
            return Ok(0.0);
        };
        raw.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| IdmlError::Invalid(format!("Mojikumi row has invalid {key}")))
    };
    let integer = |key: &str| -> Result<i16> {
        let v = number(key)?;
        if v.fract() != 0.0 || v < f64::from(i16::MIN) || v > f64::from(i16::MAX) {
            return Err(IdmlError::Invalid(format!("Mojikumi {key} must be an in-range integer")));
        }
        Ok(v as i16)
    };
    let boolean = |key: &str| -> Result<bool> {
        match e.prop(key).as_deref().map(str::trim) {
            None | Some("false") => Ok(false),
            Some("true") => Ok(true),
            _ => Err(IdmlError::Invalid(format!("Mojikumi row has invalid {key}"))),
        }
    };
    let row = designcraft_doc::cjk::MojikumiAki {
        target_class: integer("TargetMojikumiClass")?,
        side_class: integer("SideMojikumiClass")?,
        after: boolean("SideIsAfterTarget")?,
        minimum: number("Minimum")?,
        desired: number("Desired")?,
        maximum: number("Maximum")?,
        priority: integer("CompressionPriority")?,
        does_not_float: boolean("AkiDoesNotFloat")?,
    };
    Ok(row)
}

impl<'r> Importer<'r> {
    pub(super) fn char_attrs(&mut self, e: &El) -> CharAttrs {
        let mut a = CharAttrs::default();
        if let Some(f) = e.prop("AppliedFont") {
            // Some writers append the style after a tab. Spaces are part of a family's name
            // ("Minion Pro " is another font than "Minion Pro"); line breaks around it aren't.
            let fam = f.split('\t').next().unwrap_or(&f).trim_matches(['\n', '\r']);
            a.font_family = self.composite_names.get(fam.trim()).cloned().or_else(|| names::font_family_in(fam));
        }
        // "Nothing" leaves the style unset: it comes from the paragraph style.
        a.font_style = e.prop("FontStyle").filter(|s| !matches!(s.trim(), "Nothing" | "$ID/Nothing"));
        a.size = e.num("PointSize");
        a.leading = e.prop("Leading").and_then(|l| names::leading_in(l.trim()));
        // Automatic kerning may carry an inactive numeric value (including the
        // 1e11 IDML sentinel). It must not become a multi-million-em advance.
        a.kerning = e.prop("KerningMethod").and_then(|k| match k.trim().trim_start_matches("$ID/") {
            "Optical" => Some(Kerning::Optical),
            "Metrics" | "Metrics - Roman Only" => Some(Kerning::Metrics),
            "None" => Some(Kerning::None),
            _ => None,
        });
        if a.kerning.is_none() {
            a.kerning = e.num("KerningValue").filter(|v| v.is_finite() && v.abs() < 1e10).map(Kerning::Manual);
        }
        a.tracking = e.num("Tracking");
        a.leading_aki = e.num("LeadingAki").map(|v| (v >= 0.0).then_some(v));
        a.trailing_aki = e.num("TrailingAki").map(|v| (v >= 0.0).then_some(v));
        a.tsume = e.num("Tsume").map(|v| (v / 100.0).clamp(0.0, 1.0));
        a.jidori = e.num("Jidori").map(|v| v.clamp(0.0, 10000.0) as u32);
        a.leading_model = e.prop("LeadingModel").and_then(|v| crate::cjk::leading_in(&v));
        a.character_alignment = e.prop("CharacterAlignment").and_then(|v| crate::cjk::alignment_in(&v));
        a.h_scale = e.num("HorizontalScale").map(|v| v / 100.0);
        a.v_scale = e.num("VerticalScale").map(|v| v / 100.0);
        a.baseline_shift = e.num("BaselineShift");
        a.skew = e.num("Skew");
        if let Some(r) = e.prop("FillColor") {
            a.fill = Some(self.swatch_ref(r.trim()));
        }
        a.fill_tint = tint(e.num("FillTint"));
        if let Some(r) = e.prop("StrokeColor") {
            a.stroke = Some(self.swatch_ref(r.trim()));
        }
        a.stroke_tint = tint(e.num("StrokeTint"));
        a.stroke_weight = e.num("StrokeWeight");
        // Text strokes are centred on the outline or lie outside it, behind the fill.
        a.stroke_behind_fill = e.prop("StrokeAlignment").and_then(|v| match v.trim() {
            "OutsideAlignment" => Some(true),
            "CenterAlignment" => Some(false),
            _ => None,
        });
        a.capitalization = e.prop("Capitalization").and_then(|v| names::caps_in(v.trim()));
        a.position = e.prop("Position").and_then(|v| names::position_in(v.trim()));
        a.underline = e.boolean("Underline");
        a.strikethrough = e.boolean("StrikeThru");
        a.ligatures = e.boolean("Ligatures");
        // Underline / strikethrough options (-9999 = automatic).
        for k in ["Underline", "StrikeThru"] {
            let num = |n: &str| e.num(&format!("{k}{n}")).filter(|v| *v > -9000.0);
            // An explicit Text Color resets an inherited swatch; absence still inherits.
            // Resolve the sentinel before swatch_ref, which maps unknown names to [None].
            let color = e.prop(&format!("{k}Color")).map(|r| if r.trim() == "Text Color" { String::new() } else { self.swatch_ref(r.trim()) });
            let tint = tint(e.num(&format!("{k}Tint")));
            if k == "Underline" {
                a.underline_weight = num("Weight").map(Some);
                a.underline_offset = num("Offset").map(Some);
                a.underline_color = color;
                a.underline_tint = tint;
            } else {
                a.strikethrough_weight = num("Weight").map(Some);
                a.strikethrough_offset = num("Offset").map(Some);
                a.strikethrough_color = color;
                a.strikethrough_tint = tint;
            }
        }
        read_otf_features(e, &mut a);
        a.glyph_form = e.prop("GlyphForm");
        a.no_break = e.boolean("NoBreak");
        a.tate_chu_yoko = e.boolean("Tatechuyoko");
        a.tate_chu_yoko_x_offset = e.num("TatechuyokoXOffset");
        a.tate_chu_yoko_y_offset = e.num("TatechuyokoYOffset");
        a.warichu = e.boolean("Warichu");
        a.warichu_lines = e.num("WarichuLines").map(|v| v.max(2.0) as u32);
        a.warichu_size = e.num("WarichuSize");
        a.warichu_line_spacing = e.num("WarichuLineSpacing");
        a.warichu_alignment = e.prop("WarichuAlignment").as_deref().and_then(crate::cjk::warichu_align_in);
        a.warichu_chars_before_break = e.num("WarichuCharsBeforeBreak").map(|v| v.max(0.0) as u32);
        a.warichu_chars_after_break = e.num("WarichuCharsAfterBreak").map(|v| v.max(0.0) as u32);
        // InDesign writes the flag as a number (1); older writers wrote a boolean.
        if let Some(on) = e.boolean("RubyFlag").or_else(|| e.num("RubyFlag").map(|v| v != 0.0)) {
            a.ruby = Some(if on { e.prop("RubyString").unwrap_or_default() } else { String::new() });
        }
        crate::ruby::read(e, &mut a, |r| self.swatch_ref(r));
        if let Some(k) = e.get("KentenKind") {
            a.kenten = Some(k != "None");
            a.kenten_character = crate::cjk::kenten_character(k).map(str::to_string);
            if k == "Custom" {
                a.kenten_character = e.prop("KentenCustomCharacter");
            }
        }
        a.digits = e.prop("DigitsType").and_then(|s| names::digits_in(s.trim()));
        a.character_direction = e.prop("CharacterDirection").and_then(|s| crate::arabic::direction_in(&s));
        a.allow_kashidas = e.prop("Kashidas").and_then(|s| crate::arabic::kashidas_in(&s));
        a.diacritic_position = e.prop("DiacriticPosition").and_then(|s| crate::arabic::diacritic_in(&s));
        a.diacritic_x_offset = e.num("XOffsetDiacritic").filter(|v| v.is_finite());
        a.diacritic_y_offset = e.num("YOffsetDiacritic").filter(|v| v.is_finite());
        a.positional_form = e.prop("PositionalForm");
        self.cjk_settings_chars(e, &mut a);
        if let Some(v) = e.get("AppliedConditions").filter(|v| !v.trim().is_empty()) {
            let names: Vec<String> = v.split_whitespace().filter_map(|r| self.condition_names.get(r).cloned()).collect();
            if !names.is_empty() {
                a.conditions = Some(names);
            }
        }
        a.language = e.prop("AppliedLanguage").map(|l| l.trim().trim_start_matches("$ID/").to_string());
        a
    }

    pub(super) fn para_attrs(&mut self, e: &El) -> ParaAttrs {
        let mut a = ParaAttrs::default();
        let u = |k: &str| e.num(k).map(|v| v.max(0.0) as u32);
        let frac = |k: &str| e.num(k).map(|v| v / 100.0);
        a.align = e.prop("Justification").and_then(|v| names::align_in(v.trim()));
        a.kinsoku_hang = e.prop("KinsokuHangType").and_then(|v| match v.as_str() {
            "None" => Some(designcraft_doc::cjk::KinsokuHang::None),
            "KinsokuHangRegular" => Some(designcraft_doc::cjk::KinsokuHang::Regular),
            "KinsokuHangForce" => Some(designcraft_doc::cjk::KinsokuHang::Force),
            _ => None,
        });
        a.mojikumi = e.prop("Mojikumi").map(|name| self.mojikumi_names.get(&name).cloned().unwrap_or_else(|| unescape_id(&name)));
        a.kinsoku_type = e.prop("KinsokuType");
        a.bunri_kinshi = e.boolean("BunriKinshi");
        a.rensuuji = e.boolean("Rensuuji");
        a.treat_ideographic_space_as_space = e.boolean("TreatIdeographicSpaceAsSpace");
        cjk_settings_para(e, &mut a);
        if let Some(k) = e.prop("KinsokuSet") {
            if k == "Nothing" || k == "None" {
                a.kinsoku = Some(Some(Default::default()));
            } else if let Some(set) = self.kinsoku.get(&k) {
                a.kinsoku = Some(Some(set.clone()));
            } else if let Some(set) = designcraft_doc::cjk::Kinsoku::named(&k) {
                a.kinsoku = Some(Some(set));
            }
        }
        a.direction = e.prop("ParagraphDirection").map(|v| {
            if v.trim() == "RightToLeftDirection" { designcraft_doc::TextDirection::RightToLeft } else { designcraft_doc::TextDirection::LeftToRight }
        });
        a.kashidas = e.prop("Kashidas").and_then(|s| crate::arabic::kashidas_in(&s));
        a.arabic_justification = e.prop("ParagraphJustification");
        a.paragraph_kashida_width = e.num("ParagraphKashidaWidth").filter(|v| v.is_finite()).map(Some);
        a.left_indent = e.num("LeftIndent");
        a.right_indent = e.num("RightIndent");
        a.first_line_indent = e.num("FirstLineIndent");
        a.last_line_indent = e.num("LastLineIndent");
        a.space_before = e.num("SpaceBefore");
        if let Some(r) = e.prop("AppliedNumberingList") {
            let r = r.trim().trim_start_matches("NumberingList/");
            a.list_name = Some(if r.starts_with("$ID/") { String::new() } else { unescape_id(r) });
        }
        match e.prop("NumberingContinue").as_deref().map(str::trim) {
            Some("false") => a.start_at = Some(e.num("NumberingStartAt").map(|n| n.max(1.0) as u32)),
            Some("true") => a.start_at = Some(None),
            _ => {}
        }
        self.read_nested_styles(e, &mut a);
        a.space_after = e.num("SpaceAfter");
        // A number, or an enumeration (Ignore) that turns it off even over a based-on style's value.
        a.same_para_style_spacing = e.prop("SameParaStyleSpacing").map(|v| v.trim().parse::<f64>().ok().filter(|v| v.is_finite()));
        a.drop_cap_lines = u("DropCapLines");
        a.drop_cap_chars = u("DropCapCharacters");
        // DropcapDetail bits: 1 = Align Left Edge, 2 = Scale for Descenders.
        if let Some(v) = e.num("DropcapDetail").filter(|v| v.is_finite() && *v >= 0.0) {
            let bits = v as u32;
            a.drop_cap_align_left = Some(bits & 1 != 0);
            a.drop_cap_scale_descenders = Some(bits & 2 != 0);
        }
        if let Some(g) = e.prop("GridAlignment") {
            a.grid_align = Some(if g.trim() == "None" {
                GridAlign::None
            } else if e.boolean("GridAlignFirstLineOnly") == Some(true) {
                GridAlign::FirstLineOnly
            } else {
                GridAlign::AllLines
            });
            if g.trim() != "None" {
                a.grid_reference = Some(crate::cjk::grid_reference_in(&g));
            }
        }
        a.composer = e.prop("Composer").map(|c| composer_in(&c));
        a.hyphenate = e.boolean("Hyphenation");
        a.hyph_min_word = u("HyphenateWordsLongerThan");
        a.hyph_after_first = u("HyphenateAfterFirst");
        a.hyph_before_last = u("HyphenateBeforeLast");
        a.hyph_limit = u("HyphenateLadderLimit");
        a.hyph_zone = e.num("HyphenationZone");
        a.hyph_capitalized = e.boolean("HyphenateCapitalizedWords");
        a.hyph_last_word = e.boolean("HyphenateLastWord");
        a.hyph_across_column = e.boolean("HyphenateAcrossColumns");
        a.hyph_weight = e.num("HyphenWeight").map(|v| v / 10.0);
        a.word_space_min = frac("MinimumWordSpacing");
        a.word_space_desired = frac("DesiredWordSpacing");
        a.word_space_max = frac("MaximumWordSpacing");
        a.letter_space_min = frac("MinimumLetterSpacing");
        a.letter_space_desired = frac("DesiredLetterSpacing");
        a.letter_space_max = frac("MaximumLetterSpacing");
        a.glyph_scale_min = frac("MinimumGlyphScaling");
        a.glyph_scale_desired = frac("DesiredGlyphScaling");
        a.glyph_scale_max = frac("MaximumGlyphScaling");
        a.auto_leading = frac("AutoLeading");
        a.single_word_justify = e.prop("SingleWordJustification").and_then(|v| names::align_in(v.trim()));
        a.keep_with_next = u("KeepWithNext");
        a.keep_lines_together = e.boolean("KeepLinesTogether");
        a.ignore_optical_margin = e.boolean("IgnoreEdgeAlignment");
        a.keep_all_lines = e.boolean("KeepAllLinesTogether");
        a.keep_first = u("KeepFirstLines");
        a.keep_last = u("KeepLastLines");
        a.start_paragraph = e.prop("StartParagraph").and_then(|v| names::start_para_in(v.trim()));
        if let Some(t) = e.prop("SpanColumnType") {
            let n = e.prop("SpanSplitColumnCount").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
            a.span_columns = Some(match t.trim() {
                "SpanColumns" => SpanColumns::Span(n),
                "SplitColumns" => SpanColumns::Split(n.max(2)),
                _ => SpanColumns::Single,
            });
        }
        a.split_inside_gutter = e.num("SplitColumnInsideGutter").filter(|v| v.is_finite());
        a.split_outside_gutter = e.num("SplitColumnOutsideGutter").filter(|v| v.is_finite());
        a.rule_above = self.rule(e, "RuleAbove");
        a.rule_below = self.rule(e, "RuleBelow");
        a.tabs = self.read_tabs(e);
        a.list_type = e.prop("BulletsAndNumberingListType").map(|v| match v.trim() {
            "BulletList" => ListType::Bullets,
            "NumberedList" => ListType::Numbers,
            _ => ListType::None,
        });
        // A format with no counterpart (CJK, Arabic, Hebrew numbering) is left unset: decimal.
        a.number_style = e.prop("NumberingFormat").and_then(|v| names::numbering_format_in(&v));
        a.number_expression = e.prop("NumberingExpression");
        a.list_separator = e.prop("BulletsTextAfter");
        if let Some(b) = e.prop_el("BulletChar") {
            // A glyph of a named font (`GlyphWithFont`) has no character to map to.
            let unicode = matches!(b.get("BulletCharacterType"), Some("UnicodeOnly" | "UnicodeWithFont"));
            if let Some(c) = b.get("BulletCharacterValue").and_then(|v| v.trim().parse::<u32>().ok()).and_then(char::from_u32).filter(|_| unicode) {
                a.bullet_char = Some(c.to_string());
            }
        }
        a.balance_ragged = e.prop("BalanceRaggedLines").map(|v| v.trim() != "NoBalancing" && v.trim() != "false");
        a.shading_on = e.boolean("ParagraphShadingOn");
        if let Some(c) = e.prop("ParagraphShadingColor") {
            a.shading_color = Some(self.swatch_ref(c.trim()));
        }
        a.shading_tint = tint(e.num("ParagraphShadingTint"));
        a.shading_overprint = e.boolean("ParagraphShadingOverprint");
        let sides = |f: &dyn Fn(&str) -> String| -> Option<[f64; 4]> {
            let v = ["Top", "Left", "Bottom", "Right"].map(|s| e.num(&f(s)));
            v.iter().any(Option::is_some).then(|| v.map(|x| x.unwrap_or(0.0)))
        };
        a.shading_offsets = sides(&|s| format!("ParagraphShading{s}Offset"));
        a.border_on = e.boolean("ParagraphBorderOn");
        if let Some(c) = e.prop("ParagraphBorderColor") {
            a.border_color = Some(self.swatch_ref(c.trim()));
        }
        a.border_tint = tint(e.num("ParagraphBorderTint"));
        a.border_overprint = e.boolean("ParagraphBorderOverprint");
        a.border_weights = sides(&|s| format!("ParagraphBorder{s}LineWeight"));
        a.border_offsets = sides(&|s| format!("ParagraphBorder{s}Offset"));
        a.border_type = e.prop("ParagraphBorderType").map(|v| names::stroke_type_in(v.trim()));
        a.border_cap = e.prop("ParagraphBorderStrokeEndCap").map(|v| names::cap_in(v.trim()));
        a.border_join = e.prop("ParagraphBorderStrokeEndJoin").map(|v| names::join_in(v.trim()));
        a.border_corners = corners(e, "ParagraphBorder");
        a.border_width = e.prop("ParagraphBorderWidth").map(|v| names::box_width_in(v.trim()));
        a.border_top = e.prop("ParagraphBorderTopOrigin").map(|v| names::box_top_in(v.trim()));
        a.border_bottom = e.prop("ParagraphBorderBottomOrigin").map(|v| names::box_bottom_in(v.trim()));
        a.border_display_if_splits = e.boolean("ParagraphBorderDisplayIfSplits");
        a.border_merge = e.boolean("MergeConsecutiveParaBorders");
        if let Some(c) = e.prop("ParagraphBorderGapColor") {
            a.border_gap_color = Some(self.swatch_ref(c.trim()));
        }
        a.border_gap_tint = tint(e.num("ParagraphBorderGapTint"));
        a.border_gap_overprint = e.boolean("ParagraphBorderGapOverprint");
        a.shading_corners = corners(e, "ParagraphShading");
        a.shading_width = e.prop("ParagraphShadingWidth").map(|v| names::box_width_in(v.trim()));
        a.shading_top = e.prop("ParagraphShadingTopOrigin").map(|v| names::box_top_in(v.trim()));
        a.shading_bottom = e.prop("ParagraphShadingBottomOrigin").map(|v| names::box_bottom_in(v.trim()));
        a.shading_clip = e.boolean("ParagraphShadingClipToFrame");
        a.shading_nonprinting = e.boolean("ParagraphShadingSuppressPrinting");
        a.span_space_before = e.num("SpanColumnMinSpaceBefore").filter(|v| v.is_finite());
        a.span_space_after = e.num("SpanColumnMinSpaceAfter").filter(|v| v.is_finite());
        a.list_level = e.num("NumberingLevel").filter(|v| v.is_finite()).map(|v| v.round().clamp(1.0, designcraft_doc::LIST_LEVELS as f64) as u32);
        if let Some(c) = e.prop("BulletsCharacterStyle") {
            a.bullet_char_style = Some(self.char_style_ref(c.trim()));
        }
        if let Some(c) = e.prop("NumberingCharacterStyle") {
            a.number_char_style = Some(self.char_style_ref(c.trim()));
        }
        a.bullet_align = e.prop("BulletsAlignment").map(|v| names::list_align_in(v.trim()));
        a.number_align = e.prop("NumberingAlignment").map(|v| names::list_align_in(v.trim()));
        a.restart_numbers = e.boolean("NumberingApplyRestartPolicy");
        if let Some(r) = e.prop_el("NumberingRestartPolicies") {
            // A specific level restarts after that level; any other policy after any higher one.
            let level = r.get("LowerLevel").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
            a.restart_after_level = Some(if r.get("RestartPolicy").map(str::trim) == Some("SpecificPreviousLevel") { level } else { 0 });
        }
        if let Some(f) = e.prop("BulletsFont") {
            let f = f.trim();
            a.bullet_font = Some(if f == "$ID/" { String::new() } else { f.split('\t').next().unwrap_or(f).to_string() });
        }
        if let Some(f) = e.prop("BulletsFontStyle") {
            let f = f.trim();
            a.bullet_font_style = Some(if f == "Nothing" || f == "$ID/" { String::new() } else { f.trim_start_matches("$ID/").to_string() });
        }
        a
    }

    fn read_nested_styles(&self, e: &El, attrs: &mut ParaAttrs) {
        if let Some(list) = e.prop_el("AllNestedStyles") {
            let styles: Vec<designcraft_doc::NestedStyle> = list
                .find_all("ListItem")
                .map(|item| {
                    let text = |key: &str| item.find(key).map(|element| element.text_content()).unwrap_or_default();
                    let delimiter = item.find("Delimiter");
                    designcraft_doc::NestedStyle {
                        style: self.listed_char_style_ref(&text("AppliedCharacterStyle")),
                        through: text("Inclusive").trim() != "false",
                        count: text("Repetition").trim().parse().unwrap_or(1),
                        until: names::nested_until_in(delimiter.and_then(|element| element.get("type")).unwrap_or("enumeration"), &text("Delimiter")),
                    }
                })
                .collect();
            // IDML has no drop cap style attribute: the drop cap's character style is the list's
            // leading "through 1 Dropcap" nested style.
            attrs.drop_cap_style = styles.first().filter(|style| style.is_drop_cap()).map(|style| style.style.clone());
            attrs.nested_styles = Some(styles);
        }
        if let Some(list) = e.prop_el("AllNestedLineStyles") {
            let styles = list
                .find_all("ListItem")
                .map(|item| {
                    let text = |key: &str| item.find(key).map(|element| element.text_content()).unwrap_or_default();
                    designcraft_doc::NestedLineStyle {
                        style: self.listed_char_style_ref(&text("AppliedCharacterStyle")),
                        lines: text("LineCount").trim().parse().unwrap_or(1),
                    }
                })
                .collect();
            attrs.nested_line_styles = Some(styles);
        }
        if let Some(list) = e.prop_el("AllGREPStyles") {
            let styles = list
                .find_all("ListItem")
                .map(|item| {
                    let text = |key: &str| item.find(key).map(|element| element.text_content()).unwrap_or_default();
                    designcraft_doc::GrepStyle { style: self.listed_char_style_ref(&text("AppliedCharacterStyle")), pattern: text("GrepExpression") }
                })
                .collect();
            attrs.grep_styles = Some(styles);
        }
    }

    fn read_tabs(&mut self, e: &El) -> Option<Vec<TabStop>> {
        let list = e.prop_el("TabList")?;
        let tabs: Vec<_> = list
            .find_all("ListItem")
            .map(|item| {
                let get = |key: &str| item.find(key).map(|element| element.text_content());
                TabStop {
                    position: get("Position").and_then(|value| value.trim().parse::<f64>().ok()).filter(|value| value.is_finite()).unwrap_or(0.0),
                    align: names::tab_align_in(get("Alignment").unwrap_or_default().trim()),
                    leader: get("Leader").unwrap_or_default(),
                    align_on: get("AlignmentCharacter").filter(|character| character != ".").unwrap_or_default(),
                }
            })
            .collect();
        let sanitized = TabStop::sanitized_list(tabs.clone());
        if tab_list_repaired(&tabs, &sanitized) {
            self.repaired_tab_lists = self.repaired_tab_lists.saturating_add(1);
        }
        Some(sanitized)
    }
}

pub(super) fn ace_char(v: &str) -> Option<char> {
    match v.trim() {
        "18" => Some(st::PAGE_NUMBER),
        "19" => Some(st::SECTION_MARKER),
        "7" => Some(st::INDENT_HERE),
        "8" => Some(st::RIGHT_INDENT_TAB),
        _ => None,
    }
}

/// `HL Composer` and `HL Single`, their World-Ready (`… Optyca`) and Japanese (`… J`) forms.
fn composer_in(name: &str) -> designcraft_doc::Composer {
    use designcraft_doc::Composer as C;
    let name = name.trim().trim_start_matches("$ID/");
    match (name.contains("Single"), name.ends_with(" J") || name.contains("Japanese")) {
        (false, false) => C::Paragraph,
        (true, false) => C::SingleLine,
        (false, true) => C::Japanese,
        (true, true) => C::JapaneseSingleLine,
    }
}

/// [`Importer::clear_inherited_drop_cap_styles`] for one story's paragraphs and those of the table
/// cells, footnotes and endnotes in it.
pub(super) fn clear_story_drop_cap_styles(styles: &Styles, story: &mut Story, depth: usize) {
    for p in &mut story.paras {
        if p.para.drop_cap_style.is_none() && p.para.nested_styles.is_some() && inherits_drop_cap_style(styles, Some(&p.style)) {
            p.para.drop_cap_style = Some(st::NO_CHAR_STYLE.into());
        }
    }
    if depth >= MAX_STORY_NESTING {
        return;
    }
    for table in story.tables.values_mut() {
        for cell in &mut Arc::make_mut(table).cells {
            clear_story_drop_cap_styles(styles, &mut cell.text, depth + 1);
        }
    }
    for note in story.notes.iter_mut().chain(story.endnotes.iter_mut()) {
        clear_story_drop_cap_styles(styles, &mut Arc::make_mut(note).text, depth + 1);
    }
}

/// Does paragraph style `name` (and its based-on chain) set a drop cap character style? A style
/// with a nested style list of its own and no drop cap style has none.
pub(super) fn inherits_drop_cap_style<'a>(styles: &'a Styles, mut name: Option<&'a str>) -> bool {
    let mut seen = HashSet::new();
    while let Some(n) = name {
        let Some(s) = styles.para(n).filter(|_| seen.insert(n)) else { break };
        if let Some(c) = s.para.drop_cap_style.as_deref() {
            return !c.is_empty() && c != st::NO_CHAR_STYLE;
        }
        if s.para.nested_styles.is_some() {
            return false;
        }
        name = s.based_on.as_deref();
    }
    false
}

/// Auto tate-chu-yoko, Japanese composition and grid settings of a paragraph range or style.
fn cjk_settings_para(e: &El, a: &mut ParaAttrs) {
    let count = |k: &str| e.num(k).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 1000.0) as u32);
    a.auto_tcy = count("AutoTcy");
    a.auto_tcy_include_roman = e.boolean("AutoTcyIncludeRoman");
    a.rotate_roman = e.boolean("RotateSingleByteCharacters");
    // 欧文泣き別れ: the name is inferred (it follows TreatIdeographicSpaceAsSpace).
    a.roman_word_break = e.boolean("AllowArbitraryHyphenation");
    a.grid_gyoudori = count("GridGyoudori");
    a.paragraph_gyoudori = e.boolean("ParagraphGyoudori");
}

impl Importer<'_> {
    /// Kenten Settings, Kenten Color and Shatai of a character range or style.
    fn cjk_settings_chars(&mut self, e: &El, a: &mut CharAttrs) {
        let finite = |k: &str| e.num(k).filter(|v| v.is_finite());
        if let Some(kind) = e.get("KentenKind").and_then(crate::cjk::kenten_kind_in) {
            a.kenten_kind = Some(kind);
            if a.kenten_character.as_deref().is_none_or(str::is_empty)
                && let Some(mark) = kind.mark()
            {
                a.kenten_character = Some(mark.to_string());
            }
        }
        if let Some(f) = e.prop("KentenFont") {
            let f = f.trim();
            a.kenten_font = Some(if f == "$ID/" { String::new() } else { f.to_string() });
        }
        if let Some(st) = e.prop("KentenFontStyle") {
            let st = st.trim();
            a.kenten_font_style = Some(if st == "Nothing" { String::new() } else { st.to_string() });
        }
        a.kenten_size = finite("KentenFontSize").map(|v| (v > 0.0).then_some(v));
        a.kenten_x_scale = finite("KentenXScale").map(|v| v / 100.0);
        a.kenten_y_scale = finite("KentenYScale").map(|v| v / 100.0);
        a.kenten_distance = finite("KentenPlacement");
        a.kenten_position = e.prop("KentenPosition").and_then(|v| crate::cjk::kenten_position_in(&v));
        a.kenten_alignment = e.prop("KentenAlignment").and_then(|v| crate::cjk::kenten_alignment_in(&v));
        a.kenten_character_set = e.prop("KentenCharacterSet");
        // "Text Color" is the text's own colour; it resets an inherited swatch.
        if let Some(r) = e.prop("KentenFillColor") {
            a.kenten_fill = Some(if r.trim() == "Text Color" { String::new() } else { self.swatch_ref(r.trim()) });
        }
        if let Some(r) = e.prop("KentenStrokeColor") {
            a.kenten_stroke = Some(if r.trim() == "Text Color" { String::new() } else { self.swatch_ref(r.trim()) });
        }
        // Tints and weight: -1 is the text's.
        a.kenten_fill_tint = finite("KentenTint").map(|v| (v >= 0.0).then(|| (v.min(100.0) / 100.0) as f32));
        a.kenten_stroke_tint = finite("KentenStrokeTint").map(|v| (v >= 0.0).then(|| (v.min(100.0) / 100.0) as f32));
        a.kenten_stroke_weight = finite("KentenWeight").map(|v| (v >= 0.0).then_some(v));
        a.kenten_overprint_fill = e.prop("KentenOverprintFill").and_then(|v| crate::cjk::overprint_in(&v));
        a.kenten_overprint_stroke = e.prop("KentenOverprintStroke").and_then(|v| crate::cjk::overprint_in(&v));
        a.shatai_magnification = finite("ShataiMagnification");
        a.shatai_angle = finite("ShataiDegreeAngle").map(|v| v / crate::cjk::SHATAI_ANGLE_UNITS);
        a.shatai_adjust_rotation = e.boolean("ShataiAdjustRotation");
        a.shatai_adjust_tsume = e.boolean("ShataiAdjustTsume");
    }
}

/// OpenType features (only when the element sets any).
fn read_otf_features(e: &El, a: &mut CharAttrs) {
    let mut otf_list: Vec<String> = Vec::new();
    let mut any = false;
    for (tag, _, attr) in designcraft_doc::otf::TOGGLES {
        if let Some(on) = e.boolean(attr) {
            designcraft_doc::otf::set(&mut otf_list, tag, on);
            any = true;
        }
    }
    if let Some(fig) = e.prop("OTFFigureStyle")
        && let Some((id, ..)) = designcraft_doc::otf::FIGURES.iter().find(|f| f.3 == fig.trim())
    {
        designcraft_doc::otf::set_figures(&mut otf_list, id);
        any = true;
    }
    if let Some(m) = e.num("OTFStylisticSets") {
        designcraft_doc::otf::set_stylistic_sets(&mut otf_list, m.max(0.0) as u32);
        any = true;
    }
    if any {
        a.otf_features = Some(otf_list);
    }
}
