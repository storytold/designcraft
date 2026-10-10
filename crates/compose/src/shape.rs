//! Paragraph → styled, positioned glyphs (before line breaking).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use designcraft_doc::{Capitalization, CharProps, Kerning, Leading, Position, Story, Styles, story};
use designcraft_fonts::{FaceRef, Feature, FontFace, ScopedFonts, ShapedGlyph, feature};

use crate::RunStyle;

/// One glyph with every character attribute applied, in points.
#[derive(Clone, Debug)]
pub struct Glyph {
    pub face: FaceRef,
    pub gid: u32,
    /// Story byte offset of the cluster and its byte length (0 for generated glyphs).
    pub byte: usize,
    pub len: usize,
    /// First source character of the cluster (special characters keep their code).
    pub ch: char,
    /// Advance in points (tracking and horizontal scale applied).
    pub adv: f64,
    pub dx: f64,
    pub dy: f64,
    /// Font units → points (incl. horizontal / vertical scale).
    pub sx: f64,
    pub sy: f64,
    /// Baseline shift (positive = up), including super/subscript.
    pub shift: f64,
    pub ascent: f64,
    /// The font's typographic ascender at the point size (vertical scale doesn't apply): the Ascent
    /// first baseline offset.
    pub typo_ascent: f64,
    pub descent: f64,
    /// The leading this character asks for (absolute, or auto = size × auto %).
    pub leading: f64,
    /// Cap height and x height, scaled like `ascent`: the Cap Height and x Height first baseline
    /// offsets.
    pub cap: f64,
    pub xh: f64,
    pub size: f64,
    /// Index into the composed story's [`RunStyle`] table.
    pub style: u32,
    /// Unbreakable (No Break).
    pub no_break: bool,
    pub locked_advance: bool,
    /// Tate-chu-yoko in a vertical frame: [centre of the group's em along the line from `x`,
    /// this glyph's left edge across the line from that centre, the em] (see [`collapse_tcy`]).
    pub tcy: Option<[f64; 3]>,
    pub tcy_offset: [f64; 2],
    /// Width of a word space (U+0020) in this glyph's font, size and horizontal scale: the unit of
    /// word and letter spacing in justification.
    pub space: f64,
    /// Resolved CJK boundary constraint, independent of glyph outlines.
    pub break_after: Option<bool>,
    pub cjk_hang: f64,
    pub ideographic_space_elastic: bool,
    pub character_alignment: designcraft_doc::cjk::CharacterAlignment,
    pub leading_model: designcraft_doc::cjk::LeadingModel,
    pub character_direction: designcraft_doc::arabic::CharacterDirection,
    pub display_char: char,
    pub bidi_offset: usize,
    pub bidi_end: usize,
    pub allow_kashidas: bool,
    pub safe_tatweel_before: bool,
    pub shaping_rtl: bool,
    /// The character's language as a BCP 47 tag ([`designcraft_doc::language_tag`]).
    pub lang: Option<&'static str>,
    /// Upright in a vertical frame: shaped top to bottom (`adv` is its vertical advance) and hung
    /// from its vertical origin on the line's centre (see [`crate::PlacedGlyph::vertical_xf`]).
    pub upright: bool,
    /// Part of the bullet or number the paragraph's list adds: not the line's own text, so it
    /// doesn't raise the Ascent first baseline.
    pub list_label: bool,
}

impl Glyph {
    pub fn is_space(&self) -> bool {
        matches!(self.ch, ' ' | '\u{2002}'..='\u{200A}') || (self.ch == '\u{3000}' && self.ideographic_space_elastic)
    }
    pub fn is_letter(&self) -> bool {
        self.ch.is_alphabetic() || matches!(self.ch, '\'' | '’')
    }
}

pub const SOFT_HYPHEN: char = '\u{AD}';

/// Shaped paragraph.
pub struct ShapedPara {
    pub glyphs: Vec<Glyph>,
    /// Byte range of the paragraph text (without its `\n`).
    pub range: std::ops::Range<usize>,
}

/// Context for substitutions (page number markers).
#[derive(Clone, Debug, Default)]
pub struct SubstCtx {
    pub page_name: Option<String>,
    pub section_marker: Option<String>,
    /// Text variable values by index (on the page being composed).
    pub vars: Arc<Vec<String>>,
    /// Footnote reference labels by story byte of the reference character.
    pub notes: HashMap<usize, String>,
    /// Footnote reference formatting: position and character style (`None` = the text's).
    pub note_position: designcraft_doc::Position,
    pub note_style: Option<String>,
    /// Cross-reference texts by story byte of the cross-reference mark.
    pub xrefs: HashMap<usize, String>,
    /// Anchored objects by story byte of their mark.
    pub objects: HashMap<usize, ObjectSpec>,
    /// Conditions currently hidden (conditional text with only these isn't shown).
    pub hidden_conditions: Vec<String>,
    /// Set in a Vertical Type frame: upright runs are shaped top to bottom (vertical forms and
    /// metrics), the rest horizontally to be turned as a whole.
    pub vertical: bool,
}

/// The stand-in character of hidden conditional text: no width, no break, not drawn.
pub const HIDDEN: char = '\u{2060}';

/// Size and placement of an anchored object for line layout.
#[derive(Clone, Copy, Debug, Default)]
pub struct ObjectSpec {
    pub index: usize,
    pub w: f64,
    pub h: f64,
    /// Inline: raise above the baseline. Above line: None.
    pub y_offset: Option<f64>,
    /// Above line: space before + after.
    pub space: f64,
    /// Custom position: takes no space in the text.
    pub custom: bool,
}

pub(crate) struct StyleTable<'a> {
    pub styles: &'a mut Vec<RunStyle>,
    /// Font availability by family (looked up once per composition).
    pub missing: &'a mut HashMap<String, bool>,
}

impl StyleTable<'_> {
    fn intern(&mut self, db: &ScopedFonts<'_>, p: &CharProps) -> u32 {
        let rs = RunStyle {
            fill: p.fill.clone(),
            fill_tint: p.fill_tint,
            stroke: p.stroke.clone(),
            stroke_tint: p.stroke_tint,
            stroke_weight: p.stroke_weight,
            underline: p.underline,
            strikethrough: p.strikethrough,
            underline_rule: crate::Rule {
                offset: p.underline_offset.unwrap_or(p.size * 0.12),
                weight: p.underline_weight.unwrap_or(p.size / 14.0),
                color: if p.underline_color.is_empty() { p.fill.clone() } else { p.underline_color.clone() },
                tint: if p.underline_color.is_empty() { p.fill_tint } else { p.underline_tint },
            },
            strike_rule: crate::Rule {
                offset: -p.strikethrough_offset.unwrap_or(p.size * 0.3),
                weight: p.strikethrough_weight.unwrap_or(p.size / 14.0),
                color: if p.strikethrough_color.is_empty() { p.fill.clone() } else { p.strikethrough_color.clone() },
                tint: if p.strikethrough_color.is_empty() { p.fill_tint } else { p.strikethrough_tint },
            },
            skew: p.skew,
            size: p.size,
            missing_font: *self.missing.entry(p.font_family.clone()).or_insert_with(|| !p.font_family.is_empty() && !db.has_family(&p.font_family)),
            custom_tracking: p.tracking.abs() > 1e-9 || matches!(p.kerning, designcraft_doc::Kerning::Manual(_)),
            condition: p.conditions.first().cloned(),
            inserted: p.change == designcraft_doc::ChangeMark::Inserted,
            xml_tag: (!p.xml_tag.is_empty()).then(|| p.xml_tag.clone()),
            ruby: (!p.ruby.is_empty()).then(|| p.ruby.clone()),
            kenten: p.kenten,
            kenten_character: p.kenten_character.clone(),
            warichu: p.warichu,
            warichu_lines: p.warichu_lines,
            warichu_size: p.warichu_size,
            warichu_line_spacing: p.warichu_line_spacing,
            warichu_align: p.warichu_alignment,
            warichu_chars_before: p.warichu_chars_before_break,
            warichu_chars_after: p.warichu_chars_after_break,
        };
        if let Some(i) = self.styles.iter().rposition(|s| *s == rs) {
            return i as u32;
        }
        self.styles.push(rs);
        (self.styles.len() - 1) as u32
    }

    /// `env` for text in run style `style`: a missing font's substitute draws the characters it
    /// lacks from fallback fonts whatever the document's setting.
    fn env_for(&self, env: TypeEnv, style: u32) -> TypeEnv {
        let missing = self.styles.get(style as usize).is_some_and(|s| s.missing_font);
        TypeEnv { glyph_fallback: env.glyph_fallback || missing, ..env }
    }
}

/// Resolve and shape one paragraph. Its drop cap ends at byte `drop.0` (`range.start` without
/// one) and takes the character style `drop.1` ("" = none).
pub(crate) fn shape_para(
    db: &ScopedFonts<'_>,
    styles: &Styles,
    story: &Story,
    pi: usize,
    range: std::ops::Range<usize>,
    para_chars: &CharProps,
    auto_leading: TypeEnv,
    sub: &SubstCtx,
    table: &mut StyleTable<'_>,
    nested: &[designcraft_doc::NestedStyle],
    grep: &[designcraft_doc::GrepStyle],
    lines: &[(std::ops::Range<usize>, String)],
    drop: (usize, &str),
) -> ShapedPara {
    let _ = pi;
    let mut glyphs = Vec::with_capacity(range.len());
    // Nested line styles lie under nested and GREP styles, and the drop cap's style over them all
    // (later overlays win).
    let mut overlays: Vec<(std::ops::Range<usize>, String)> = lines.to_vec();
    if !nested.is_empty() || !grep.is_empty() {
        overlays.extend(crate::overlay::overlays(&story.text, range.clone(), nested, grep, drop.0));
    }
    if drop.0 > range.start && !drop.1.is_empty() && drop.1 != designcraft_doc::NO_CHAR_STYLE {
        overlays.push((range.start..drop.0, drop.1.to_string()));
    }
    // Each run, cut where nested / GREP styles start and end.
    let mut segments: Vec<(usize, usize, Option<&str>, &designcraft_doc::CharFormat)> = Vec::new();
    for (rr, fmt) in story.runs() {
        let a = rr.start.max(range.start);
        let b = rr.end.min(range.end);
        if a >= b {
            continue;
        }
        if overlays.is_empty() {
            segments.push((a, b, None, fmt));
            continue;
        }
        let mut cuts: Vec<usize> = overlays.iter().flat_map(|(r, _)| [r.start, r.end]).filter(|x| *x > a && *x < b).collect();
        cuts.push(a);
        cuts.push(b);
        cuts.sort_unstable();
        cuts.dedup();
        for w in cuts.windows(2) {
            let top = overlays.iter().rev().find(|(r, _)| r.start <= w[0] && w[0] < r.end).map(|(_, s)| s.as_str());
            segments.push((w[0], w[1], top, fmt));
        }
    }
    let mut resolved = Vec::new();
    for (a, b, overlay, fmt) in segments {
        // A nested / GREP character style sits under the run's own style and overrides.
        let base;
        let para_chars = match overlay {
            Some(st) => {
                base = styles.resolve_char(para_chars, &designcraft_doc::CharFormat { style: st.to_string(), over: Default::default() });
                &base
            }
            None => para_chars,
        };
        let props = styles.resolve_char(para_chars, fmt);
        let family = props.font_family.trim_start_matches("CompositeFont/");
        if let Some(font) = styles.composite_fonts.iter().find(|f| f.name == family) {
            let mut start = a;
            let mut selected = None;
            for (offset, c) in story.text.get(a..b).unwrap_or("").char_indices() {
                let entry = font.entry(c);
                if entry != selected && a + offset > start {
                    resolved.push((start, a + offset, composite_props(db, &props, selected), fmt, para_chars.clone()));
                    start = a + offset;
                }
                selected = entry;
            }
            resolved.push((start, b, composite_props(db, &props, selected), fmt, para_chars.clone()));
        } else {
            resolved.push((a, b, props, fmt, para_chars.clone()));
        }
    }
    for (a, b, props, fmt, resolved_base) in resolved {
        let style = table.intern(db, &props);
        // A missing font's substitute stands in for the whole font: fallback fonts help it.
        let auto_leading = table.env_for(auto_leading, style);
        let deleted = props.change == designcraft_doc::ChangeMark::Deleted;
        if deleted || (!props.conditions.is_empty() && props.conditions.iter().all(|c| sub.hidden_conditions.contains(c))) {
            // Hidden conditional text: zero-width, unbreakable, undrawn place-holders keep every
            // byte addressable (caret, selection) without taking space.
            let face = db.face(&props.font_family, &props.font_style);
            for (i, c) in story.text[a..b].char_indices() {
                if c == '\n' {
                    continue;
                }
                let mut g = control_glyph(&face, &props, auto_leading, style, a + i, c);
                g.ch = HIDDEN;
                (g.ascent, g.typo_ascent, g.descent, g.leading, g.cap, g.xh) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
                glyphs.push(g);
            }
            continue;
        }
        let is_ref = |c: char| c == designcraft_doc::FOOTNOTE_REF || c == designcraft_doc::ENDNOTE_REF;
        if !story.text[a..b].contains(is_ref) {
            shape_run(db, &story.text, a..b, &props, auto_leading, style, sub, &mut glyphs);
            continue;
        }
        // Footnote references take the reference position / character style; endnote references
        // are superscript.
        let mut rf = fmt.clone();
        if let Some(cs) = &sub.note_style {
            rf.style = cs.clone();
        }
        rf.over.position = Some(sub.note_position);
        let rprops = styles.resolve_char(&resolved_base, &rf);
        let rstyle = table.intern(db, &rprops);
        let renv = table.env_for(auto_leading, rstyle);
        let mut ef = fmt.clone();
        ef.over.position = Some(designcraft_doc::Position::Superscript);
        let eprops = styles.resolve_char(&resolved_base, &ef);
        let estyle = table.intern(db, &eprops);
        let eenv = table.env_for(auto_leading, estyle);
        let mut k = a;
        for (i, m) in story.text[a..b].match_indices(is_ref) {
            let i = a + i;
            if k < i {
                shape_run(db, &story.text, k..i, &props, auto_leading, style, sub, &mut glyphs);
            }
            let e = i + m.len();
            if m.starts_with(designcraft_doc::ENDNOTE_REF) {
                shape_run(db, &story.text, i..e, &eprops, eenv, estyle, sub, &mut glyphs);
            } else {
                shape_run(db, &story.text, i..e, &rprops, renv, rstyle, sub, &mut glyphs);
            }
            k = e;
        }
        if k < b {
            shape_run(db, &story.text, k..b, &props, auto_leading, style, sub, &mut glyphs);
        }
    }
    collapse_tcy(&mut glyphs, sub.vertical);
    ShapedPara { glyphs, range }
}

/// Tate-chu-yoko: in vertical text each run of marked glyphs takes one em along the line, its
/// glyphs set side by side across it; elsewhere the mark is dropped.
pub(crate) fn collapse_tcy(glyphs: &mut [Glyph], vertical: bool) {
    let mut i = 0;
    while i < glyphs.len() {
        if glyphs[i].tcy.is_none() {
            i += 1;
            continue;
        }
        let j = i + glyphs[i..].iter().position(|g| g.tcy.is_none()).unwrap_or(glyphs.len() - i);
        if !vertical {
            glyphs[i..j].iter_mut().for_each(|g| g.tcy = None);
            i = j;
            continue;
        }
        let run = &mut glyphs[i..j];
        let width: f64 = run.iter().map(|g| g.adv).sum();
        let em = run.iter().map(|g| g.size).fold(0.0, f64::max);
        let step = em / run.len() as f64;
        let mut across = -width / 2.0;
        for (k, g) in run.iter_mut().enumerate() {
            g.tcy = Some([em / 2.0 - k as f64 * step + g.tcy_offset[1], across + g.dx + g.tcy_offset[0], em]);
            across += g.adv;
            g.adv = step;
            g.dx = 0.0;
        }
        i = j;
    }
}

fn features_for(p: &CharProps) -> Vec<Feature> {
    let mut v = Vec::new();
    if !matches!(p.kerning, Kerning::Metrics | Kerning::Optical) {
        v.extend(feature("-kern"));
    }
    if !p.ligatures || p.tracking.abs() > 1e-9 {
        v.extend(feature("-liga"));
        v.extend(feature("-clig"));
    }
    if p.capitalization == Capitalization::AllCaps {
        v.extend(feature("case"));
    }
    if matches!(p.capitalization, Capitalization::SmallCaps | Capitalization::OpenTypeAllSmallCaps) {
        v.extend(feature("smcp"));
    }
    if p.capitalization == Capitalization::OpenTypeAllSmallCaps {
        v.extend(feature("c2sc"));
    }
    match p.position {
        Position::OtSuperscript => v.extend(feature("sups")),
        Position::OtSubscript => v.extend(feature("subs")),
        Position::OtNumerator => v.extend(feature("numr")),
        Position::OtDenominator => v.extend(feature("dnom")),
        _ => {}
    }
    let form = match p.glyph_form.as_str() {
        "ExpertForm" => "expt",
        "TraditionalForm" => "trad",
        "SimplifiedForm" => "smpl",
        "JIS78Form" => "jp78",
        "JIS83Form" => "jp83",
        "JIS90Form" => "jp90",
        "JIS04Form" => "jp04",
        _ => "",
    };
    if !form.is_empty() {
        v.extend(feature(form));
    }
    for f in &p.otf_features {
        v.extend(feature(f));
    }
    let positional = match p.positional_form.as_str() {
        "Initial" => Some("init"),
        "Medial" => Some("medi"),
        "Final" => Some("fina"),
        "Isolated" => Some("isol"),
        _ => None,
    };
    if let Some(tag) = positional {
        for form in ["isol", "init", "medi", "fina"] {
            v.extend(feature(&format!("{}{}", if form == tag { "" } else { "-" }, form)));
        }
    }
    v
}

fn is_mark(c: char) -> bool {
    unicode_bidi::bidi_class(c) == unicode_bidi::BidiClass::NSM || matches!(c, '\u{200C}' | '\u{200D}')
}

fn composite_props(db: &ScopedFonts<'_>, base: &CharProps, entry: Option<&designcraft_doc::cjk::CompositeFontEntry>) -> CharProps {
    let mut p = base.clone();
    if let Some(e) = entry {
        p.font_family.clone_from(&e.family);
        p.font_style.clone_from(&e.style);
        p.size *= e.relative_size.clamp(0.01, 100.0);
        p.h_scale *= e.horizontal_scale.clamp(0.01, 100.0);
        p.v_scale *= e.vertical_scale.clamp(0.01, 100.0);
        p.baseline_shift += base.size * e.baseline_shift;
        if e.scale_option {
            let face = db.face(&p.font_family, &p.font_style);
            let (asc, desc) = face.vertical_metrics();
            let center = (asc - desc) / (2.0 * (asc + desc).max(1e-9));
            p.baseline_shift += (base.size * base.v_scale - p.size * p.v_scale) * center;
        }
    }
    p
}

#[allow(clippy::too_many_arguments)]
fn shape_run(
    db: &ScopedFonts<'_>,
    text: &str,
    range: std::ops::Range<usize>,
    p: &CharProps,
    env: TypeEnv,
    style: u32,
    sub: &SubstCtx,
    out: &mut Vec<Glyph>,
) {
    let start = out.len();
    shape_run_raw(db, text, range, p, env, style, sub, out);
    let run = &mut out[start..];
    let em = p.size * p.h_scale;
    for g in run.iter_mut().filter(|g| g.adv > 0.0 && !g.ch.is_control()) {
        let (mut left, mut right) = (0.0, 0.0);
        if p.tsume > 0.0 && !g.ch.is_whitespace() {
            let outline = db.outline(&g.face, g.gid);
            if !outline.elements().is_empty() {
                let bounds = designcraft_geom::Shape::bounding_box(&*outline);
                left = (g.dx + bounds.x0 * g.sx).clamp(0.0, g.adv);
                right = (g.adv - g.dx - bounds.x1 * g.sx).clamp(0.0, g.adv - left);
            }
        }
        let tsume = p.tsume.clamp(0.0, 1.0);
        let before = p.leading_aki.unwrap_or(0.0).clamp(0.0, 100.0) * em;
        let after = p.trailing_aki.unwrap_or(0.0).clamp(0.0, 100.0) * em;
        g.dx += before - left * tsume;
        g.adv += before + after - (left + right) * tsume;
    }
    if p.jidori > 0 && !run.is_empty() && !run.iter().any(|g| g.ch.is_control()) {
        let clusters: Vec<usize> = run.iter().enumerate().filter(|(_, g)| g.len > 0).map(|(i, _)| i).collect();
        let natural: f64 = run.iter().map(|g| g.adv).sum();
        let target = f64::from(p.jidori.min(10000)) * em;
        if clusters.len() == 1 {
            if let Some(g) = run.first_mut() {
                g.dx += (target - natural) * 0.5;
                g.adv += target - natural;
            }
        } else if clusters.len() > 1 {
            let gap = (target - natural) / (clusters.len() - 1) as f64;
            for &i in clusters.iter().skip(1) {
                if let Some(g) = run.get_mut(i.saturating_sub(1)) {
                    g.adv += gap;
                }
            }
        }
        let end = run.len().saturating_sub(1);
        for g in run.iter_mut().take(end) {
            g.break_after = Some(false);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn shape_run_raw(
    db: &ScopedFonts<'_>,
    text: &str,
    range: std::ops::Range<usize>,
    p: &CharProps,
    auto_leading: TypeEnv,
    style: u32,
    sub: &SubstCtx,
    out: &mut Vec<Glyph>,
) {
    let primary = db.face(&p.font_family, &p.font_style);
    // The language picks the fallback for CJK characters the primary font lacks. Without
    // fallback, the primary font sets them: the shaper gives its .notdef (glyph 0).
    let lang = designcraft_doc::language_tag(&p.language);
    let fallback = |c: char| {
        if auto_leading.glyph_fallback { db.fallback_for(c, primary.id(), lang).unwrap_or_else(|| primary.clone()) } else { primary.clone() }
    };
    let strong = |c| match unicode_bidi::bidi_class(c) {
        unicode_bidi::BidiClass::AL => Some(true),
        unicode_bidi::BidiClass::L | unicode_bidi::BidiClass::R => Some(false),
        _ => None,
    };
    let mut arabic_context = text.get(..range.start).unwrap_or("").chars().rev().take_while(|c| !matches!(c, '\n' | '\r')).find_map(strong);
    // Split into segments: font coverage changes and special characters (markers, tabs, breaks).
    let mut seg_start = range.start;
    let mut seg_face = primary.clone();
    let flush = |a: usize, b: usize, face: &Arc<FontFace>, out: &mut Vec<Glyph>| {
        if a < b {
            shape_segment(db, text, a..b, None, p, face, auto_leading, style, out, sub.vertical);
        }
    };
    for (i, c) in text[range.clone()].char_indices() {
        let i = range.start + i;
        if let Some(context) = strong(c) {
            arabic_context = Some(context);
        }
        // Digits drawn in another script (World-Ready Digits): each is set as its substitute.
        {
            use designcraft_doc::Digits;
            let mode = if p.digits != Digits::Default {
                p.digits
            } else {
                match arabic_context {
                    Some(false) => Digits::Arabic,
                    Some(true) if Digits::Native.map('0', &p.language) == '0' => Digits::Hindi,
                    _ => Digits::Native,
                }
            };
            let d = mode.map(c, &p.language);
            if d != c {
                flush(seg_start, i, &seg_face, out);
                seg_start = i + c.len_utf8();
                let face = if primary.covers(d) { primary.clone() } else { fallback(d) };
                shape_segment(db, text, i..i + c.len_utf8(), Some(d.encode_utf8(&mut [0; 4])), p, &face, auto_leading, style, out, sub.vertical);
                continue;
            }
        }
        let special = matches!(
            c,
            '\t' | story::FORCED_LINE_BREAK
                | story::PAGE_NUMBER
                | story::NEXT_PAGE_NUMBER
                | story::PREV_PAGE_NUMBER
                | story::SECTION_MARKER
                | story::COLUMN_BREAK
                | story::FRAME_BREAK
                | story::PAGE_BREAK
                | story::ODD_PAGE_BREAK
                | story::EVEN_PAGE_BREAK
                | story::INDENT_HERE
                | story::RIGHT_INDENT_TAB
                | story::TABLE_ANCHOR
                | designcraft_doc::FOOTNOTE_REF
                | designcraft_doc::ENDNOTE_REF
                | designcraft_doc::NOTE_MARK
                | designcraft_doc::XREF_MARK
                | designcraft_doc::ANCHOR_MARK
                | designcraft_doc::INDEX_MARK
                | designcraft_doc::OBJECT_MARK
        ) || designcraft_doc::vars::var_index(c).is_some();
        if special {
            flush(seg_start, i, &seg_face, out);
            seg_start = i + c.len_utf8();
            if let Some(vi) = designcraft_doc::vars::var_index(c) {
                match sub.vars.get(vi).filter(|v| !v.is_empty()) {
                    Some(v) => shape_segment(db, text, i..i + c.len_utf8(), Some(v), p, &primary, auto_leading, style, out, sub.vertical),
                    None => {
                        let mut g = control_glyph(&primary, p, auto_leading, style, i, c);
                        g.adv = 0.0;
                        out.push(g);
                    }
                }
                continue;
            }
            match c {
                story::PAGE_NUMBER | story::NEXT_PAGE_NUMBER | story::PREV_PAGE_NUMBER | story::SECTION_MARKER => {
                    let s = if c == story::SECTION_MARKER {
                        sub.section_marker.clone().unwrap_or_else(|| "Section".into())
                    } else {
                        sub.page_name.clone().unwrap_or_else(|| "#".into())
                    };
                    shape_segment(db, text, i..i + c.len_utf8(), Some(&s), p, &primary, auto_leading, style, out, sub.vertical);
                }
                designcraft_doc::OBJECT_MARK => {
                    let mut g = control_glyph(&primary, p, auto_leading, style, i, c);
                    if let Some(o) = sub.objects.get(&i).filter(|o| !o.custom) {
                        let auto = matches!(p.leading, Leading::Auto);
                        match o.y_offset {
                            // Sits on the baseline like a (big) character.
                            Some(y) => {
                                g.adv = o.w;
                                g.ascent = g.ascent.max(o.h + y);
                                g.typo_ascent = g.typo_ascent.max(o.h + y);
                                g.cap = g.cap.max(o.h + y);
                                g.xh = g.xh.max(o.h + y);
                                g.descent = g.descent.max(-y);
                            }
                            // Pushes its line down by its height and spacing.
                            None => {
                                g.ascent += o.h + o.space;
                                g.typo_ascent += o.h + o.space;
                                g.cap += o.h + o.space;
                                g.xh += o.h + o.space;
                            }
                        }
                        if auto || o.y_offset.is_none() {
                            g.leading = g.leading.max(g.ascent + g.descent);
                        }
                    }
                    out.push(g);
                }
                designcraft_doc::XREF_MARK => match sub.xrefs.get(&i).filter(|t| !t.is_empty()) {
                    Some(t) => shape_segment(db, text, i..i + c.len_utf8(), Some(t), p, &primary, auto_leading, style, out, sub.vertical),
                    None => {
                        let mut g = control_glyph(&primary, p, auto_leading, style, i, c);
                        g.adv = 0.0;
                        out.push(g);
                    }
                },
                designcraft_doc::FOOTNOTE_REF | designcraft_doc::ENDNOTE_REF => {
                    let s = sub.notes.get(&i).map_or("#", String::as_str);
                    shape_segment(db, text, i..i + c.len_utf8(), Some(s), p, &primary, auto_leading, style, out, sub.vertical);
                }
                _ => {
                    // Zero-width control glyph carrying metrics (tabs get their width at line layout).
                    let mut g = control_glyph(&primary, p, auto_leading, style, i, c);
                    if c == '\t'
                        || c == story::RIGHT_INDENT_TAB
                        || c == story::TABLE_ANCHOR
                        || c == designcraft_doc::ANCHOR_MARK
                        || c == designcraft_doc::INDEX_MARK
                    {
                        g.adv = 0.0;
                    }
                    out.push(g);
                }
            }
            continue;
        }
        let covered = c.is_whitespace() || c.is_control() || c == SOFT_HYPHEN || primary.covers(c);
        let face = if covered || is_mark(c) { if is_mark(c) { seg_face.clone() } else { primary.clone() } } else { fallback(c) };
        if face.id() != seg_face.id() {
            flush(seg_start, i, &seg_face, out);
            seg_start = i;
            seg_face = face;
        }
    }
    flush(seg_start, range.end, &seg_face, out);
}

type WordMap = HashMap<Box<str>, Arc<[ShapedGlyph]>>;
type WordKey = (u32, bool, bool, String, Option<&'static str>);

/// Shaped words (font units) by (face, vertical, caps, features, language), then text. Lookups
/// take read locks and misses are shaped outside any lock, so parallel composition doesn't
/// serialize here.
static WORD_CACHE: RwLock<Vec<(WordKey, Arc<RwLock<WordMap>>)>> = RwLock::new(Vec::new());
const WORD_CACHE_MAX: usize = 50_000;

fn word_map(key: WordKey) -> Arc<RwLock<WordMap>> {
    if let Some((_, m)) = WORD_CACHE.read().unwrap_or_else(|e| e.into_inner()).iter().find(|e| e.0 == key) {
        return m.clone();
    }
    let mut w = WORD_CACHE.write().unwrap_or_else(|e| e.into_inner());
    if let Some((_, m)) = w.iter().find(|e| e.0 == key) {
        return m.clone();
    }
    if w.len() > 64 {
        w.clear();
    }
    let m = Arc::new(RwLock::new(HashMap::new()));
    w.push((key, m.clone()));
    m
}

/// Shape `src` word by word through a cache (like a browser's word cache): the text is split
/// after each U+0020 so repeated words are shaped once. Shaping does not cross word spaces in the
/// scripts we lay out, and clusters stay byte offsets into `src`.
fn shape_cached(
    face: &FontFace,
    src: &str,
    feats: &[Feature],
    lang: Option<&'static str>,
    caps: bool,
    vertical: bool,
    (before, after): (&str, &str),
) -> Vec<ShapedGlyph> {
    use designcraft_fonts::ShapeContext;
    let map = |c: char| if caps { c.to_uppercase().next().unwrap_or(c) } else { c };
    let shape = |s: &str, context: ShapeContext<'_>| {
        if vertical {
            designcraft_fonts::shape_vertical(face, s, feats, map, context)
        } else {
            designcraft_fonts::shape_with_context(face, s, feats, map, context)
        }
    };
    if src.len() < 2 || !src.contains(' ') || src.chars().any(designcraft_fonts::is_rtl) {
        return shape(src, ShapeContext { before, after, language: lang });
    }
    let words = word_map((face.id(), vertical, caps, format!("{feats:?}"), lang));
    let pieces: Vec<&str> = src.split_inclusive(' ').collect();
    let mut found: Vec<Option<Arc<[ShapedGlyph]>>> = {
        let r = words.read().unwrap_or_else(|e| e.into_inner());
        pieces.iter().map(|p| r.get(*p).cloned()).collect()
    };
    let mut fresh: Vec<(Box<str>, Arc<[ShapedGlyph]>)> = Vec::new();
    for (k, f) in found.iter_mut().enumerate() {
        if f.is_none() {
            let g: Arc<[ShapedGlyph]> = shape(pieces[k], ShapeContext { language: lang, ..Default::default() }).into();
            fresh.push((pieces[k].into(), g.clone()));
            *f = Some(g);
        }
    }
    if !fresh.is_empty() {
        let mut w = words.write().unwrap_or_else(|e| e.into_inner());
        if w.len() + fresh.len() > WORD_CACHE_MAX {
            w.clear();
        }
        w.extend(fresh);
    }
    let mut out = Vec::with_capacity(src.len());
    let mut start = 0;
    for (piece, glyphs) in pieces.iter().zip(found) {
        let glyphs = glyphs.unwrap_or_else(|| Arc::from(Vec::new()));
        out.extend(glyphs.iter().map(|g| ShapedGlyph { cluster: g.cluster + start, ..*g }));
        start += piece.len();
    }
    out
}

fn metrics(face: &FontFace, p: &CharProps, auto_leading: TypeEnv) -> (f64, f64, f64, f64, f64, f64, f64, f64) {
    let (size, shift) = effective_size(p, &auto_leading.adv);
    let k = size / face.upem;
    let vs = p.v_scale;
    let leading = match p.leading {
        Leading::Auto => p.size * auto_leading.auto_leading,
        Leading::Points(v) => v,
    };
    (size, k, face.ascent * k * vs, face.descent * k * vs, leading, face.cap_height * k * vs, face.x_height * k * vs, shift)
}

/// Auto leading and the document's Advanced Type settings, for shaping.
#[derive(Clone, Copy, Debug)]
pub struct TypeEnv {
    pub auto_leading: f64,
    pub adv: designcraft_doc::AdvancedType,
    /// Characters the font lacks are drawn from fallback fonts (else as its .notdef glyph).
    pub glyph_fallback: bool,
}

/// Synthesised super/subscript: size and shift from Advanced Type (percent of the font size).
fn effective_size(p: &CharProps, adv: &designcraft_doc::AdvancedType) -> (f64, f64) {
    match p.position {
        Position::Superscript => (p.size * adv.superscript_size / 100.0, p.baseline_shift + p.size * adv.superscript_position / 100.0),
        Position::Subscript => (p.size * adv.subscript_size / 100.0, p.baseline_shift - p.size * adv.subscript_position / 100.0),
        _ => (p.size, p.baseline_shift),
    }
}

fn control_glyph(face: &Arc<FontFace>, p: &CharProps, auto_leading: TypeEnv, style: u32, byte: usize, ch: char) -> Glyph {
    let (size, k, ascent, descent, leading, cap, xh, shift) = metrics(face, p, auto_leading);
    Glyph {
        face: FaceRef::of(face),
        gid: face.glyph_for(' '),
        byte,
        len: ch.len_utf8(),
        ch,
        adv: 0.0,
        dx: 0.0,
        dy: 0.0,
        sx: k * p.h_scale,
        sy: k * p.v_scale,
        shift,
        ascent,
        typo_ascent: face.typo_ascent * k,
        descent,
        leading,
        cap,
        xh,
        size,
        style,
        no_break: p.no_break,
        locked_advance: p.jidori > 0,
        tcy: p.tate_chu_yoko.then_some([0.0; 3]),
        tcy_offset: [p.tate_chu_yoko_x_offset, p.tate_chu_yoko_y_offset],
        space: face.advance(face.glyph_for(' ')) * k * p.h_scale,
        break_after: None,
        cjk_hang: 0.0,
        ideographic_space_elastic: false,
        character_alignment: p.character_alignment,
        leading_model: p.leading_model,
        character_direction: p.character_direction,
        display_char: ch,
        bidi_offset: 0,
        bidi_end: 0,
        allow_kashidas: p.allow_kashidas,
        safe_tatweel_before: false,
        shaping_rtl: false,
        lang: designcraft_doc::language_tag(&p.language),
        upright: false,
        list_label: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn shape_segment(
    db: &ScopedFonts<'_>,
    text: &str,
    range: std::ops::Range<usize>,
    replacement: Option<&str>,
    p: &CharProps,
    face: &Arc<FontFace>,
    auto_leading: TypeEnv,
    style: u32,
    out: &mut Vec<Glyph>,
    vertical: bool,
) {
    let _ = db;
    let (size, k, ascent, descent, leading, cap, xh, shift) = metrics(face, p, auto_leading);
    let hs = p.h_scale;
    let tracking = p.tracking / 1000.0 * p.size;
    let manual = if let Kerning::Manual(v) = p.kerning { v / 1000.0 * p.size } else { 0.0 };
    let src = replacement.unwrap_or(&text[range.clone()]);
    let caps = p.capitalization == Capitalization::AllCaps;
    let feats = features_for(p);
    // Upright in vertical text, Horizontal Kana takes the vertical kana alternates instead.
    let mut upright_feats = feats.clone();
    if vertical && designcraft_doc::otf::is_on(&p.otf_features, "hkna") {
        upright_feats.extend(feature("-hkna"));
        upright_feats.extend(feature("vkna"));
    }
    let space = face.advance(face.glyph_for(' ')) * k * hs;
    // The language picks the font's localized forms (`locl`); the text around the run lets
    // letters join across a change of style.
    let lang = designcraft_doc::language_tag(&p.language);
    // Vertical frames: upright runs top to bottom, the rest (turned as a whole) horizontally.
    // Tate-chu-yoko is set across the line, so it stays horizontal.
    let mut shaped: Vec<ShapedGlyph> = Vec::new();
    let mut upright: Vec<bool> = Vec::new();
    for (r, up) in upright_runs(src, vertical && !p.tate_chu_yoko) {
        let around = match replacement {
            None => (text.get(..range.start + r.start).unwrap_or(""), text.get(range.start + r.end..).unwrap_or("")),
            Some(_) => ("", ""),
        };
        let g = shape_cached(face, &src[r.clone()], if up { &upright_feats } else { &feats }, lang, caps, up, around);
        upright.extend(std::iter::repeat_n(up, g.len()));
        shaped.extend(g.into_iter().map(|g| ShapedGlyph { cluster: g.cluster + r.start, ..g }));
    }
    // Upright glyphs fill the ideographic em box across the line.
    let (em_top, em_bottom) = face.em_box();
    let k_v = k * p.v_scale;
    let n = shaped.len();
    let fref = FaceRef::of(face);
    for (gi, sg) in shaped.iter().enumerate() {
        let up = upright.get(gi).copied().unwrap_or(false);
        let (byte, len, ch) = if replacement.is_some() {
            (range.start, if gi == 0 { range.len() } else { 0 }, text[range.start..].chars().next().unwrap_or(' '))
        } else {
            let cl = range.start + sg.cluster;
            let end = shaped[gi + 1..].iter().map(|r| range.start + r.cluster).find(|&c| c > cl).unwrap_or(range.end);
            (cl, end.saturating_sub(cl), text[cl..].chars().next().unwrap_or(' '))
        };
        let last_in_cluster = gi + 1 == n || shaped[gi + 1].cluster != sg.cluster;
        // Upright: down the line is the shaper's -y, across it (text space -y) its x.
        let mut adv = if up { -(sg.y_advance as f64) * k_v } else { sg.x_advance as f64 * k * hs };
        let (dx, dy) = if up {
            (-(sg.y_offset as f64) * k_v, -(sg.x_offset as f64) * k * hs)
        } else {
            (sg.x_offset as f64 * k * hs, -(sg.y_offset as f64) * k * p.v_scale)
        };
        let (ascent, descent) = if up { (em_top * k_v, -em_bottom * k_v) } else { (ascent, descent) };
        if ch == SOFT_HYPHEN {
            adv = 0.0;
        } else if last_in_cluster {
            adv += tracking + manual;
        }
        // Only the first glyph of a cluster owns the bytes (so ranges partition the text).
        let first_in_cluster = gi == 0 || shaped[gi - 1].cluster != sg.cluster;
        let mark = face.glyph_is_mark(sg.gid);
        let mark_x = if mark { p.diacritic_x_offset.clamp(-1000.0, 1000.0) / 1000.0 * p.size * hs } else { 0.0 };
        let mark_y = if mark { p.diacritic_y_offset.clamp(-1000.0, 1000.0) / 1000.0 * p.size * p.v_scale } else { 0.0 };
        out.push(Glyph {
            face: fref,
            gid: sg.gid,
            byte,
            len: if first_in_cluster || replacement.is_some() { len } else { 0 },
            ch,
            adv,
            dx: dx + mark_x,
            dy: dy - mark_y,
            sx: k * hs,
            sy: k * p.v_scale,
            shift,
            ascent,
            typo_ascent: face.typo_ascent * k,
            descent,
            leading,
            cap,
            xh,
            size,
            style,
            no_break: p.no_break,
            locked_advance: p.jidori > 0,
            tcy: p.tate_chu_yoko.then_some([0.0; 3]),
            tcy_offset: [p.tate_chu_yoko_x_offset, p.tate_chu_yoko_y_offset],
            space,
            break_after: None,
            cjk_hang: 0.0,
            ideographic_space_elastic: false,
            character_alignment: p.character_alignment,
            leading_model: p.leading_model,
            character_direction: p.character_direction,
            display_char: if replacement.is_some() && matches!(ch, '0'..='9' | '\u{0660}'..='\u{0669}' | '\u{06F0}'..='\u{06F9}') {
                src.chars().next().unwrap_or(ch)
            } else {
                ch
            },
            bidi_offset: 0,
            bidi_end: 0,
            allow_kashidas: p.allow_kashidas,
            safe_tatweel_before: sg.safe_tatweel_before,
            shaping_rtl: sg.rtl,
            lang,
            upright: up,
            list_label: false,
        });
    }
}

/// `src` cut into runs set upright in vertical text (CJK) and runs that turn with the line, as
/// (byte range, upright); combining marks stay with the run they follow. One horizontal run when
/// `vertical` is false.
fn upright_runs(src: &str, vertical: bool) -> Vec<(std::ops::Range<usize>, bool)> {
    let mut runs: Vec<(std::ops::Range<usize>, bool)> = Vec::new();
    for (i, c) in src.char_indices() {
        let up = vertical && crate::upright_in_vertical(c);
        match runs.last_mut() {
            Some((r, u)) if *u == up || is_mark(c) => r.end = i + c.len_utf8(),
            _ => runs.push((i..i + c.len_utf8(), up)),
        }
    }
    runs
}

/// A hyphen glyph in the face/size of `g`, placed after it (zero source length).
pub(crate) fn hyphen_after(g: &Glyph) -> Glyph {
    let gid = designcraft_fonts::first_glyph(&g.face, &['-', '\u{2010}']);
    let mut h = g.clone();
    h.gid = gid;
    h.adv = g.face.advance(gid) * g.sx;
    h.dx = 0.0;
    h.dy = 0.0;
    h.byte = g.byte + g.len;
    h.len = 0;
    h.ch = '-';
    h
}
