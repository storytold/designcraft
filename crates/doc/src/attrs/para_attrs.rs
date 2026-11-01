use super::{
    Align, BoxBottom, BoxTop, BoxWidth, Composer, Deserialize, GrepStyle, GridAlign, ListAlign, ListCounter, ListType, MAX_TAB_STOPS,
    NestedLineStyle, NestedStyle, NumberStyle, Rule, Serialize, SpanColumns, StartParagraph, TabStop, TextDirection, attr_key,
    deserialize_explicit_value, expand_list_levels, expand_list_text, list_level_index, sanitized_tabs, unknown_attribute,
};

attr_set! {
    /// Paragraph attributes (Paragraph panel / paragraph styles).
    ParaAttrs / ParaProps {
        align: Align = Align::Left,
        /// Paragraph Direction (World-Ready): the base direction lines are ordered in.
        direction: TextDirection = TextDirection::LeftToRight,
        left_indent: f64 = 0.0,
        right_indent: f64 = 0.0,
        first_line_indent: f64 = 0.0,
        last_line_indent: f64 = 0.0,
        space_before: f64 = 0.0,
        space_after: f64 = 0.0,
        /// Space between consecutive paragraphs of the same style; replaces space after/before there (None = ignore).
        same_para_style_spacing: Option<f64> = None => "deserialize_explicit_value",
        /// Drop cap: the paragraph's first `drop_cap_chars` characters (code points, never
        /// splitting a base from its combining marks) set `drop_cap_lines` lines tall, in the
        /// character style `drop_cap_style` ("" or `[None]`: the text's own formatting).
        drop_cap_lines: u32 = 0,
        drop_cap_chars: u32 = 0,
        drop_cap_style: String = String::new(),
        /// Align Left Edge: the drop cap's ink starts at the indent (its first letter's left side
        /// bearing is taken off), and the lines beside it start where its advance ends.
        drop_cap_align_left: bool = true,
        /// Scale for Descenders (kept and round-tripped; doesn't change the layout).
        drop_cap_scale_descenders: bool = false,
        /// Nested styles, in order from the paragraph start.
        nested_styles: Vec<NestedStyle> = Vec::new(),
        /// GREP styles: a character style for every match of a pattern (applied after nested).
        grep_styles: Vec<GrepStyle> = Vec::new(),
        /// Nested line styles: a character style for the paragraph's first lines, in order.
        nested_line_styles: Vec<NestedLineStyle> = Vec::new(),
        grid_align: GridAlign = GridAlign::None,
        composer: Composer = Composer::Paragraph,
        // Hyphenation
        hyphenate: bool = true,
        hyph_min_word: u32 = 5,
        hyph_after_first: u32 = 2,
        hyph_before_last: u32 = 2,
        /// Max consecutive hyphenated lines (0 = unlimited).
        hyph_limit: u32 = 3,
        hyph_zone: f64 = 36.0,
        hyph_capitalized: bool = true,
        hyph_last_word: bool = true,
        hyph_across_column: bool = true,
        /// Better spacing (0) ↔ fewer hyphens (1).
        hyph_weight: f64 = 0.5,
        // Justification (percent as fractions)
        word_space_min: f64 = 0.8,
        word_space_desired: f64 = 1.0,
        word_space_max: f64 = 1.33,
        letter_space_min: f64 = 0.0,
        letter_space_desired: f64 = 0.0,
        letter_space_max: f64 = 0.0,
        glyph_scale_min: f64 = 1.0,
        glyph_scale_desired: f64 = 1.0,
        glyph_scale_max: f64 = 1.0,
        auto_leading: f64 = 1.2,
        single_word_justify: Align = Align::FullyJustified,
        /// Insert Kashidas (World-Ready): justified Arabic lines stretch at joins before spaces.
        kashidas: bool = true,
        /// Preserved vendor policy; non-default policies require a dedicated composer.
        arabic_justification: String = String::new(),
        paragraph_kashida_width: Option<f64> = None => "deserialize_explicit_value",
        /// Korean text breaks between any two syllables (character-based breaking); otherwise
        /// it breaks at spaces, word by word.
        korean_char_breaks: bool = false,
        // Keeps
        keep_with_next: u32 = 0,
        keep_lines_together: bool = false,
        /// All lines in paragraph (true) or at start/end of paragraph (false).
        keep_all_lines: bool = false,
        keep_first: u32 = 2,
        keep_last: u32 = 2,
        start_paragraph: StartParagraph = StartParagraph::Anywhere,
        span_columns: SpanColumns = SpanColumns::Single,
        /// Split Columns: the space between sub-columns and the inset from the column's edges.
        split_inside_gutter: f64 = 6.0,
        split_outside_gutter: f64 = 0.0,
        rule_above: Rule = Rule::default(),
        rule_below: Rule = Rule::default(),
        #[serde(deserialize_with = "sanitized_tabs")]
        tabs: Vec<TabStop> = Vec::new(),
        list_type: ListType = ListType::None,
        /// Named list (Define Lists) the numbers belong to ("" = the story's own list).
        list_name: String = String::new(),
        /// Start At: restart the numbering at this number.
        start_at: Option<u32> = None => "deserialize_explicit_value",
        bullet_char: String = "\u{2022}".into(),
        number_style: NumberStyle = NumberStyle::Arabic,
        /// A numbered paragraph's label: `^#` is the number, other metacharacters expand as in
        /// [`expand_list_text`].
        number_expression: String = "^#.^t".into(),
        /// Text after the bullet (metacharacters expand as in [`expand_list_text`]).
        list_separator: String = "\t".into(),
        /// The hanging indent was set by the Bulleted or Numbered List button, which takes it away
        /// again with the list (an indent the paragraph had before stays).
        list_hang: bool = false,
        balance_ragged: bool = false,
        /// Hang punctuation outside the measure in this paragraph, at each character's own size
        /// (DesignCraft's own per-paragraph switch; a story with Optical Margin Alignment on
        /// overrides it with the story's size).
        optical_margin: bool = false,
        /// Ignore Optical Margin: no hang in this paragraph, whatever the story or
        /// `optical_margin` say.
        ignore_optical_margin: bool = false,
        /// None uses the legacy built-in rules; an empty set explicitly disables kinsoku.
        kinsoku: Option<crate::cjk::Kinsoku> = None => "deserialize_explicit_value",
        /// Preserved spacing-table reference; compatibility audit reports unsupported composition.
        mojikumi: String = String::new(),
        /// Preserved priority policy; the current breaker does not implement these priorities.
        kinsoku_type: String = String::new(),
        kinsoku_hang: crate::cjk::KinsokuHang = crate::cjk::KinsokuHang::None,
        bunri_kinshi: bool = false,
        rensuuji: bool = true,
        treat_ideographic_space_as_space: bool = false,
        // Auto Tate-chu-yoko, Japanese Composition and Grid Settings.
        /// Auto tate-chu-yoko: in vertical text, runs of up to this many half-width digits are set
        /// across one em (0 = off).
        auto_tcy: u32 = 0,
        /// Auto tate-chu-yoko also takes runs of half-width roman letters.
        auto_tcy_include_roman: bool = false,
        /// Rotate Roman Characters in Vertical Text (縦組み中の欧文回転): half-width characters
        /// stand upright one by one instead of lying along the line.
        rotate_roman: bool = false,
        /// Roman word break (欧文泣き別れ): roman words may break between any two letters,
        /// without a hyphen.
        roman_word_break: bool = false,
        /// The point of a line that sits on the grid while `grid_align` is on.
        grid_reference: crate::cjk::CharacterAlignment = crate::cjk::CharacterAlignment::Baseline,
        /// Gyoudori (行取り): grid lines each line takes (0 = as many as its leading needs).
        grid_gyoudori: u32 = 0,
        /// Paragraph gyoudori: the paragraph as a whole, not each line, takes `grid_gyoudori` lines.
        paragraph_gyoudori: bool = false,
        /// Paragraph shading, and how far it reaches past the text: top, left, bottom, right.
        shading_on: bool = false,
        shading_color: String = "[Black]".into(),
        shading_tint: f32 = 0.2,
        /// Overprint (InDesign's Paragraph Shading › Overprint): stored and kept through IDML;
        /// PDF export doesn't overprint yet.
        shading_overprint: bool = false,
        shading_offsets: [f64; 4] = [0.0; 4],
        /// Paragraph border: stroke weights (top, left, bottom, right), colour, and offsets past the
        /// text (top, left, bottom, right).
        border_on: bool = false,
        border_weights: [f64; 4] = [1.0; 4],
        border_color: String = "[Black]".into(),
        border_tint: f32 = 1.0,
        /// Overprint the border's stroke (stored and kept through IDML, as `ParagraphBorderOverprint`).
        border_overprint: bool = false,
        border_offsets: [f64; 4] = [0.0; 4],
        /// The border's stroke type (a document stroke style by name, or a built-in type), cap and
        /// join.
        border_type: crate::item::StrokeType = crate::item::StrokeType::Solid,
        border_cap: crate::item::Cap = crate::item::Cap::Butt,
        border_join: crate::item::Join = crate::item::Join::Miter,
        /// Corner shape and size: top left, top right, bottom right, bottom left.
        border_corners: [designcraft_geom::corners::Corner; 4] = [designcraft_geom::corners::Corner::default(); 4],
        border_width: BoxWidth = BoxWidth::Column,
        border_top: BoxTop = BoxTop::Ascent,
        border_bottom: BoxBottom = BoxBottom::Descent,
        /// A paragraph split across frames or columns gets a closed box in each part (otherwise the
        /// box is open where it splits).
        border_display_if_splits: bool = false,
        /// Consecutive paragraphs with the same border and shading share one box.
        border_merge: bool = true,
        /// The colour between the dashes, dots or stripes of a border that isn't solid.
        border_gap_color: String = "[None]".into(),
        border_gap_tint: f32 = 1.0,
        /// Overprint the border's gap colour (stored and kept through IDML).
        border_gap_overprint: bool = false,
        /// Shading corners: top left, top right, bottom right, bottom left.
        shading_corners: [designcraft_geom::corners::Corner; 4] = [designcraft_geom::corners::Corner::default(); 4],
        shading_width: BoxWidth = BoxWidth::Column,
        shading_top: BoxTop = BoxTop::Ascent,
        shading_bottom: BoxBottom = BoxBottom::Descent,
        /// The shading stops at the edges of the text frame.
        shading_clip: bool = false,
        /// The shading shows on screen only: not printed or exported.
        shading_nonprinting: bool = false,
        /// Least space between a spanning or split paragraph and the column text above and below it.
        span_space_before: f64 = 0.0,
        span_space_after: f64 = 0.0,
        /// List level (1–9): each level numbers on its own, and `^1`…`^9` in a number's text are the
        /// current numbers of those levels.
        list_level: u32 = 1,
        /// Character styles of the bullet and of the number.
        bullet_char_style: String = crate::story::NO_CHAR_STYLE.into(),
        number_char_style: String = crate::story::NO_CHAR_STYLE.into(),
        bullet_align: ListAlign = ListAlign::Left,
        number_align: ListAlign = ListAlign::Left,
        /// A tab stop for the text after the bullet or number, from the column's left edge.
        list_tab: Option<f64> = None => "deserialize_explicit_value",
        /// Restart Numbers at This Level After: a paragraph at a higher level (0 = any; otherwise
        /// that level) starts this level's numbering again.
        restart_numbers: bool = true,
        restart_after_level: u32 = 0,
        /// The bullet's font family and style ("" = the paragraph's).
        bullet_font: String = String::new(),
        bullet_font_style: String = String::new(),
    }
}

impl ParaAttrs {
    /// [`set_json`](Self::set_json), except that a `ruleAbove` / `ruleBelow` object names only the
    /// rule fields to change; the others keep their values in `current` (the resolved attributes
    /// of the paragraph or style being edited). A `tabs` list is checked and sorted as
    /// [`TabStop::checked_list`] does.
    pub fn set_json_over(&mut self, key: &str, value: &serde_json::Value, current: &ParaProps) -> Result<(), String> {
        let rule = match key.chars().filter(|c| *c != '_').collect::<String>().to_ascii_lowercase().as_str() {
            "ruleabove" => &current.rule_above,
            "rulebelow" => &current.rule_below,
            "tabs" if !value.is_null() => {
                // Count before deserializing: the list may be arbitrarily long.
                if value.as_array().is_some_and(|a| a.len() > MAX_TAB_STOPS) {
                    return Err(format!("{key}: a paragraph holds at most {MAX_TAB_STOPS} tab stops"));
                }
                let tabs: Vec<TabStop> = serde_json::from_value(value.clone()).map_err(|e| format!("{key}: {e}"))?;
                self.tabs = Some(TabStop::checked_list(tabs).map_err(|e| format!("{key}: {e}"))?);
                return Ok(());
            }
            _ => return self.set_json(key, value),
        };
        let serde_json::Value::Object(patch) = value else { return self.set_json(key, value) };
        let mut merged = serde_json::to_value(rule).map_err(|e| format!("{key}: {e}"))?;
        let serde_json::Value::Object(fields) = &mut merged else { return Err(format!("{key}: not an object")) };
        // Every field, those at their defaults (left out of the JSON) included.
        const RULE_FIELDS: [&str; 12] =
            ["on", "weight", "color", "tint", "columnWidth", "offset", "leftIndent", "rightIndent", "type", "gapColor", "gapTint", "keepInFrame"];
        for (k, v) in patch {
            if !RULE_FIELDS.contains(&k.as_str()) {
                return Err(format!("{key}: unknown rule field `{k}`"));
            }
            fields.insert(k.clone(), v.clone());
        }
        self.set_json(key, &merged)
    }
}

impl ParaProps {
    /// The label of a numbered paragraph with number `n`.
    pub fn number_label(&self, n: u32) -> String {
        expand_list_text(&self.number_expression, &self.number_style.format(n))
    }

    /// The label of a numbered paragraph that `list` has just numbered ([`ListCounter::advance`]).
    pub fn number_label_in(&self, list: &ListCounter) -> String {
        let level = list_level_index(self.list_level);
        let number = list.labels.get(level).map_or("", String::as_str);
        expand_list_levels(&self.number_expression, number, &list.labels)
    }

    /// The label of a bulleted paragraph.
    pub fn bullet_label(&self) -> String {
        let mut s: String = self.bullet_char.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        s.push_str(&expand_list_text(&self.list_separator, ""));
        s
    }
}
