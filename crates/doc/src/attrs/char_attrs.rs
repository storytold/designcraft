use super::{
    Capitalization, ChangeMark, Deserialize, Digits, Kerning, Leading, Position, Serialize, attr_key, deserialize_explicit_value, unknown_attribute,
};

attr_set! {
    /// Character attributes (Character panel / character styles / the character part of paragraph styles).
    CharAttrs / CharProps {
        font_family: String = "Source Serif 4".into(),
        font_style: String = "Regular".into(),
        /// Point size.
        size: f64 = 12.0,
        leading: Leading = Leading::Auto,
        kerning: Kerning = Kerning::Metrics,
        /// Tracking in 1/1000 em.
        tracking: f64 = 0.0,
        /// Explicit CJK aki, in em; None restores automatic spacing.
        leading_aki: Option<f64> = None => "deserialize_explicit_value",
        trailing_aki: Option<f64> = None => "deserialize_explicit_value",
        /// Proportional character compression, 0..1.
        tsume: f64 = 0.0,
        /// Target em cells for the contiguous formatted group (0 disables jidori).
        jidori: u32 = 0,
        character_alignment: crate::cjk::CharacterAlignment = crate::cjk::CharacterAlignment::Baseline,
        leading_model: crate::cjk::LeadingModel = crate::cjk::LeadingModel::Roman,
        /// The kenten mark's text: a custom mark, or the preset of `kenten_kind` (see
        /// [`crate::cjk_settings::kenten_mark`]); empty is the sesame dot.
        kenten_character: String = String::new(),
        /// Horizontal / vertical scale, 1.0 = 100%.
        h_scale: f64 = 1.0,
        v_scale: f64 = 1.0,
        baseline_shift: f64 = 0.0,
        /// Skew (false italic), degrees.
        skew: f64 = 0.0,
        fill: String = "[Black]".into(),
        fill_tint: f32 = 1.0,
        stroke: String = "[None]".into(),
        stroke_tint: f32 = 1.0,
        stroke_weight: f64 = 1.0,
        /// Stroke behind fill: the stroke is painted under the fill and lies wholly outside the
        /// letters at its full weight, so the letters keep their shape. Off, it straddles the outline.
        stroke_behind_fill: bool = false,
        capitalization: Capitalization = Capitalization::Normal,
        position: Position = Position::Normal,
        underline: bool = false,
        strikethrough: bool = false,
        /// Underline Options: weight and offset below the baseline (`None` = from the size),
        /// colour (empty = the text colour) and tint.
        underline_weight: Option<f64> = None => "deserialize_explicit_value",
        underline_offset: Option<f64> = None => "deserialize_explicit_value",
        underline_color: String = String::new(),
        underline_tint: f32 = 1.0,
        /// Strikethrough Options: offset above the baseline.
        strikethrough_weight: Option<f64> = None => "deserialize_explicit_value",
        strikethrough_offset: Option<f64> = None => "deserialize_explicit_value",
        strikethrough_color: String = String::new(),
        strikethrough_tint: f32 = 1.0,
        glyph_form: String = String::new(),
        ligatures: bool = true,
        no_break: bool = false,
        /// Tate-chu-yoko: in vertical text, the run is set horizontally within one em of the line.
        tate_chu_yoko: bool = false,
        tate_chu_yoko_x_offset: f64 = 0.0,
        tate_chu_yoko_y_offset: f64 = 0.0,
        /// Ruby: the reading set small above the text (to its right in vertical text). Group ruby
        /// sets it over the whole run; per-character ruby holds one reading per character,
        /// separated by U+3000. Its other settings are the `ruby_*` fields below.
        ruby: String = String::new(),
        /// Kenten: an emphasis dot above each character.
        kenten: bool = false,
        /// Warichu: the run is set in smaller lines stacked inside one line height.
        warichu: bool = false,
        /// How many warichu lines (InDesign's minimum is 2).
        warichu_lines: u32 = 2,
        /// Warichu size as a percentage of the parent size.
        warichu_size: f64 = 50.0,
        /// Extra points between warichu baselines. 0 is automatic (one small em).
        warichu_line_spacing: f64 = 0.0,
        warichu_alignment: crate::cjk::WarichuAlignment = crate::cjk::WarichuAlignment::Auto,
        /// Minimum characters on a warichu line before a break, and on the line after it.
        warichu_chars_before_break: u32 = 1,
        warichu_chars_after_break: u32 = 1,
        // Kenten Settings / Kenten Color (`kenten` switches the marks on).
        /// The mark; [`crate::cjk_settings::KentenKind::Custom`] draws `kenten_character`.
        kenten_kind: crate::cjk_settings::KentenKind = crate::cjk_settings::KentenKind::SesameDot,
        /// Font of a custom mark (empty = the text's font) and its style.
        kenten_font: String = String::new(),
        kenten_font_style: String = String::new(),
        /// Mark size in points; `None` is half the text size.
        kenten_size: Option<f64> = None => "deserialize_explicit_value",
        /// Horizontal / vertical scale of the mark, 1.0 = 100%.
        kenten_x_scale: f64 = 1.0,
        kenten_y_scale: f64 = 1.0,
        /// Points between the text's em box and the mark's.
        kenten_distance: f64 = 0.0,
        kenten_position: crate::cjk_settings::KentenPosition = crate::cjk_settings::KentenPosition::AboveRight,
        kenten_alignment: crate::cjk_settings::KentenAlignment = crate::cjk_settings::KentenAlignment::Center,
        /// How a custom mark was entered (IDML `KentenCharacterSet`), kept for interchange.
        kenten_character_set: String = String::new(),
        /// Mark fill and stroke swatches (empty = the text's), tints and stroke weight (`None` =
        /// the text's).
        kenten_fill: String = String::new(),
        kenten_fill_tint: Option<f32> = None => "deserialize_explicit_value",
        kenten_stroke: String = String::new(),
        kenten_stroke_tint: Option<f32> = None => "deserialize_explicit_value",
        kenten_stroke_weight: Option<f64> = None => "deserialize_explicit_value",
        kenten_overprint_fill: crate::cjk_settings::AdornmentOverprint = crate::cjk_settings::AdornmentOverprint::Auto,
        kenten_overprint_stroke: crate::cjk_settings::AdornmentOverprint = crate::cjk_settings::AdornmentOverprint::Auto,
        // Shatai (斜体): the character is compressed across the direction at `shatai_angle`.
        /// Compression in percent of the em (0 = off).
        shatai_magnification: f64 = 0.0,
        /// Degrees from the line direction, counter-clockwise.
        shatai_angle: f64 = 45.0,
        /// Turn the compressed character back so its edge along the line stays along the line.
        shatai_adjust_rotation: bool = false,
        /// Fit the advance to the compressed em box.
        shatai_adjust_tsume: bool = true,
        // Ruby settings (see [`crate::ruby`]). Defaults are the IDML text defaults.
        ruby_type: crate::ruby::RubyType = crate::ruby::RubyType::Group,
        /// How a ruby shorter than its parent is spread over it.
        ruby_alignment: crate::ruby::RubyAlignment = crate::ruby::RubyAlignment::Jis,
        ruby_position: crate::ruby::RubyPosition = crate::ruby::RubyPosition::AboveRight,
        /// Points along the line, and away from the parent text across it.
        ruby_x_offset: f64 = 0.0,
        ruby_y_offset: f64 = 0.0,
        /// Empty: the parent's font and style.
        ruby_font: String = String::new(),
        ruby_font_style: String = String::new(),
        /// Points; `None` is half the parent size.
        ruby_font_size: Option<f64> = None => "deserialize_explicit_value",
        /// 1.0 = 100%.
        ruby_x_scale: f64 = 1.0,
        ruby_y_scale: f64 = 1.0,
        /// Use the font's ruby glyphs (OpenType `ruby`).
        ruby_open_type_pro: bool = true,
        /// Tate-chu-yoko in vertical ruby for runs of up to this many digits (0 = off).
        ruby_auto_tcy_digits: u32 = 0,
        ruby_auto_tcy_include_roman: bool = false,
        ruby_auto_tcy_auto_scale: bool = true,
        /// IDML `RubyOverhang`, kept for round trips; `ruby_overhang_amount` sets the overhang.
        ruby_overhang: bool = false,
        ruby_overhang_amount: crate::ruby::RubyOverhang = crate::ruby::RubyOverhang::OneRuby,
        ruby_parent_spacing: crate::ruby::RubyParentSpacing = crate::ruby::RubyParentSpacing::Aki121,
        /// A longer ruby at a line's start or end is set flush with the line edge.
        ruby_auto_align: bool = true,
        /// A longer ruby is narrowed first, down to `ruby_scaling_min` (1.0 = 100%).
        ruby_auto_scaling: bool = false,
        ruby_scaling_min: f64 = 0.66,
        /// Ruby colour: empty is the text's colour; `None` tint and weight follow the text.
        ruby_fill: String = String::new(),
        ruby_fill_tint: Option<f32> = None => "deserialize_explicit_value",
        ruby_stroke: String = String::new(),
        ruby_stroke_tint: Option<f32> = None => "deserialize_explicit_value",
        ruby_stroke_weight: Option<f64> = None => "deserialize_explicit_value",
        ruby_overprint_fill: crate::cjk_settings::AdornmentOverprint = crate::cjk_settings::AdornmentOverprint::Auto,
        ruby_overprint_stroke: crate::cjk_settings::AdornmentOverprint = crate::cjk_settings::AdornmentOverprint::Auto,
        /// Digits (World-Ready): how 0–9 are drawn.
        digits: Digits = Digits::Default,
        character_direction: crate::arabic::CharacterDirection = crate::arabic::CharacterDirection::Default,
        /// Character-level permission, additionally gated by paragraph kashidas.
        allow_kashidas: bool = true,
        diacritic_position: crate::arabic::DiacriticPosition = crate::arabic::DiacriticPosition::Default,
        /// Additional mark offsets, in thousandths of an em. Positive y is up.
        diacritic_x_offset: f64 = 0.0,
        diacritic_y_offset: f64 = 0.0,
        /// IDML contextual-form request, separate from general OpenType features.
        positional_form: String = String::new(),
        language: String = "English: USA".into(),
        /// Additional OpenType features, e.g. `["onum", "ss01"]`.
        otf_features: Vec<String> = Vec::new(),
        /// Conditional text: the conditions applied (Window › Type & Tables › Conditional Text).
        conditions: Vec<String> = Vec::new(),
        /// Track Changes: text added or deleted while tracking (deleted text isn't laid out).
        change: ChangeMark = ChangeMark::None,
        /// XML: the element this text is tagged with (Tags panel, inline).
        xml_tag: String = String::new(),
    }
}
