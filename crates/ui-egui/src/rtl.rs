//! Read-only interface text: Unicode bidi run ordering followed by Arabic shaping.
//! Source strings stay logical. Never use these galleys for editable text/caret mapping.
use egui::text::{LayoutJob, TextWrapping};
use egui::{Align, Align2, Color32, FontFamily, FontId, Galley, Pos2, Rect, WidgetText};
use std::{collections::HashMap, ops::Range, sync::Arc};
use unicode_bidi::{BidiClass, BidiInfo, bidi_class};
use unicode_segmentation::UnicodeSegmentation;

/// Allocation-free preflight: ordinary Latin, CJK and emoji labels need no bidi analysis.
/// Include directional controls so explicit embeddings/overrides keep their existing behavior.
fn may_have_rtl(text: &str) -> bool {
    !text.is_ascii()
        && text.chars().any(|c| {
            matches!(
                bidi_class(c),
                BidiClass::R
                    | BidiClass::AL
                    | BidiClass::AN
                    | BidiClass::LRE
                    | BidiClass::RLE
                    | BidiClass::LRO
                    | BidiClass::RLO
                    | BidiClass::PDF
                    | BidiClass::LRI
                    | BidiClass::RLI
                    | BidiClass::FSI
                    | BidiClass::PDI
            )
        })
}

fn has_rtl(text: &str) -> bool {
    may_have_rtl(text) && BidiInfo::new(text, None).has_rtl()
}

/// Resolve translated labels through bidi, without changing IDs, commands or stored text.
pub fn widget(ui: &egui::Ui, text: impl Into<WidgetText>) -> WidgetText {
    let text = text.into();
    if !has_rtl(text.text()) {
        return text;
    }
    let mut job = (*text.into_layout_job(ui.style(), egui::FontSelection::Default, Align::Center)).clone();
    job.wrap = TextWrapping::from_wrap_mode_and_width(ui.wrap_mode(), ui.available_width());
    WidgetText::Galley(layout(ui.ctx(), job))
}

/// Labels are read-only: egui's LTR selection indices cannot map bidi glyph order.
/// Accessibility still receives the original logical string through Galley::job.
pub fn label(ui: &mut egui::Ui, text: impl Into<WidgetText>) -> egui::Response {
    let text = text.into();
    let rtl = has_rtl(text.text());
    if !rtl {
        return ui.label(text);
    }
    let selectable = false;
    let rendered = widget(ui, text.clone());
    let elided = matches!(&rendered, WidgetText::Galley(galley) if galley.elided);
    let response = ui.add(egui::Label::new(rendered).selectable(selectable).show_tooltip_when_elided(!rtl));
    if rtl && elided {
        response.on_hover_ui(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            label(ui, text);
        })
    } else {
        response
    }
}

/// Isolate a user-provided name when embedding it in LTR chrome/metadata.
/// A Latin product name or suffix must not change the name's internal reading order.
pub fn isolate(text: &str) -> String {
    if text.chars().any(designcraft_fonts::is_rtl) { format!("\u{2068}{text}\u{2069}") } else { text.to_owned() }
}

pub fn plain(ctx: &egui::Context, text: &str, font: FontId, color: Color32) -> Arc<Galley> {
    layout(ctx, LayoutJob::simple_singleline(text.to_owned(), font, color))
}

pub fn paint(p: &egui::Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32) {
    let galley = plain(p.ctx(), text, font, color);
    let rect = anchor.anchor_rect(Rect::from_min_size(pos, galley.size()));
    p.galley(rect.min, galley, color);
}

/// Reorder directional runs, not characters: the shaper needs logical Arabic to join letters.
fn visual_line(job: &LayoutJob, bidi: &BidiInfo<'_>, line: Range<usize>) -> LayoutJob {
    let mut visual = LayoutJob { break_on_newline: false, ..Default::default() };
    if line.is_empty() {
        if let Some(section) =
            job.sections.iter().find(|s| usize::from(s.byte_range.start) <= line.start && line.start <= usize::from(s.byte_range.end))
        {
            visual.append("", 0.0, section.format.clone());
        }
        return visual;
    }
    let Some(para) = bidi.paragraphs.iter().find(|p| p.range.start <= line.start && line.start < p.range.end) else {
        // Preserve the font/height for an empty line.
        if let Some(section) = job.sections.first() {
            visual.append("", 0.0, section.format.clone());
        }
        return visual;
    };
    let (levels, runs) = bidi.visual_runs(para, line);
    for run in runs {
        let rtl = levels[run.start].is_rtl();
        let mut sections: Vec<_> = job
            .sections
            .iter()
            .filter_map(|section| {
                let start = run.start.max(section.byte_range.start.into());
                let end = run.end.min(section.byte_range.end.into());
                (start < end).then_some((section, start..end))
            })
            .collect();
        if rtl {
            sections.reverse();
        }
        for (section, range) in sections {
            let mut format = section.format.clone();
            if rtl {
                let bold = format.font_id.family == FontFamily::Name("semibold".into());
                format.font_id.family = FontFamily::Name(if bold { "arabic-semibold" } else { "arabic" }.into());
                if bold {
                    format.coords = egui::epaint::text::VariationCoords::new([(*b"wght", 600.0)]);
                }
            }
            let text = &job.text[range];
            if rtl && !text.chars().any(designcraft_fonts::is_rtl) {
                // Neutral-only RTL runs have no script for the shaper to infer a direction.
                // Apply UBA L4 mirroring here; Arabic runs are mirrored by the shaper.
                let neutral: String = text.chars().rev().map(|c| unicode_bidi_mirroring::get_mirrored(c).unwrap_or(c)).collect();
                visual.append(&neutral, section.leading_space, format);
            } else {
                visual.append(text, section.leading_space, format);
            }
        }
    }
    visual
}

const MAX_CACHED_LABELS: usize = 256;
const MAX_CACHED_TEXT_BYTES: usize = 4096;

#[derive(Clone)]
struct CachedLayout {
    galley: Arc<Galley>,
    last_used: u64,
}

#[derive(Clone, Default)]
struct LayoutCache {
    // A tiny galley owned by egui's font cache acts as a lifetime token. Font changes,
    // DPI changes, atlas rebuilds and eviction replace it. Holding the old Arc prevents
    // address reuse. Unlike a font-definition hash, this also detects atlas resets.
    font_token: Option<Arc<Galley>>,
    entries: HashMap<u64, CachedLayout>,
    clock: u64,
}

impl LayoutCache {
    fn lookup(&mut self, token: Arc<Galley>, key: u64, job: &LayoutJob) -> Option<Arc<Galley>> {
        if self.font_token.as_ref().is_none_or(|old| !Arc::ptr_eq(old, &token)) {
            self.entries.clear();
            self.font_token = Some(token);
        }
        self.clock = self.clock.saturating_add(1);
        let entry = self.entries.get_mut(&key)?;
        // Check equality as well as the hash: collisions must never display another label.
        if *entry.galley.job != *job {
            return None;
        }
        entry.last_used = self.clock;
        Some(Arc::clone(&entry.galley))
    }

    fn insert(&mut self, key: u64, galley: Arc<Galley>) {
        if self.entries.len() >= MAX_CACHED_LABELS
            && let Some(oldest) = self.entries.iter().min_by_key(|(_, entry)| entry.last_used).map(|(key, _)| *key)
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(key, CachedLayout { galley, last_used: self.clock });
    }
}

fn layout(ctx: &egui::Context, job: LayoutJob) -> Arc<Galley> {
    if !may_have_rtl(&job.text) {
        return ctx.fonts_mut(|fonts| fonts.layout_job(job));
    }
    // Keep unusually large/dynamic content from filling a UI-label cache.
    if job.text.len() > MAX_CACHED_TEXT_BYTES || job.sections.len() > 256 {
        return layout_uncached(ctx, job);
    }
    let token = ctx.fonts_mut(|fonts| fonts.layout_no_wrap(String::new(), FontId::default(), Color32::TRANSPARENT));
    let id = egui::Id::new("designcraft.rtl.layout-cache");
    let key = egui::util::hash(&job);
    let cached = ctx.data_mut(|data| data.get_temp_mut_or_default::<LayoutCache>(id).lookup(token, key, &job));
    if let Some(galley) = cached {
        return galley;
    }
    let galley = layout_uncached(ctx, job);
    ctx.data_mut(|data| data.get_temp_mut_or_default::<LayoutCache>(id).insert(key, Arc::clone(&galley)));
    galley
}

fn layout_uncached(ctx: &egui::Context, mut job: LayoutJob) -> Arc<Galley> {
    if !has_rtl(&job.text) {
        return ctx.fonts_mut(|fonts| fonts.layout_job(job));
    }
    let original = Arc::new(job.clone());
    if !job.break_on_newline {
        // Single-line widgets show line separators as spaces; byte offsets stay unchanged.
        job.text = job.text.replace(['\n', '\r'], " ");
    }
    let bidi = BidiInfo::new(&job.text, None);
    let shape = |range| ctx.fonts_mut(|fonts| fonts.layout_job(visual_line(&job, &bidi, range)));
    // Wrap in logical reading order BEFORE applying bidi separately to each line.
    let mut ranges = Vec::new();
    let mut start = 0;
    for paragraph in job.text.split_inclusive('\n') {
        let end = start + paragraph.len();
        let content_end = if paragraph.ends_with('\n') { end - 1 } else { end };
        if !job.wrap.max_width.is_finite() || shape(start..content_end).size().x <= job.wrap.max_width {
            ranges.push(start..content_end);
            start = end;
            continue;
        }
        let mut line_start = start;
        let mut word_break = start;
        for (offset, cluster) in job.text[start..content_end].grapheme_indices(true) {
            let index = start + offset;
            let boundary = index + cluster.len();
            if job.wrap.max_width.is_finite() && index > line_start && shape(line_start..boundary).size().x > job.wrap.max_width {
                let cut = if !job.wrap.break_anywhere && word_break > line_start { word_break } else { index };
                ranges.push(line_start..cut);
                line_start = cut;
            }
            if cluster.chars().all(char::is_whitespace) {
                word_break = boundary;
            }
        }
        ranges.push(line_start..content_end);
        start = end;
    }
    if job.text.ends_with('\n') {
        ranges.push(start..start);
    }
    let elided = ranges.len() > job.wrap.max_rows;
    ranges.truncate(job.wrap.max_rows);
    let mut lines = Vec::new();
    for (i, range) in ranges.iter().enumerate() {
        if elided && i + 1 == ranges.len() {
            // Append the ellipsis in logical order, then reorder and shape the final line.
            let mut last = LayoutJob::default();
            for section in &job.sections {
                let start = range.start.max(section.byte_range.start.into());
                let end = range.end.min(section.byte_range.end.into());
                if start < end {
                    last.append(&job.text[start..end], 0.0, section.format.clone());
                }
            }
            let format = last.sections.last().or(job.sections.first()).map(|s| s.format.clone()).unwrap_or_default();
            let tail = job.wrap.overflow_character.map_or(String::new(), |c| c.to_string());
            let base = bidi.paragraphs.iter().find(|p| p.range.contains(&range.start)).map(|p| p.level);
            loop {
                let mut candidate = last.clone();
                candidate.append(&tail, 0.0, format.clone());
                let info = BidiInfo::new(&candidate.text, base);
                let shaped = ctx.fonts_mut(|fonts| fonts.layout_job(visual_line(&candidate, &info, 0..candidate.text.len())));
                if shaped.size().x <= job.wrap.max_width || last.text.is_empty() {
                    lines.push(shaped);
                    break;
                }
                let end = last.text.grapheme_indices(true).next_back().map_or(0, |(i, _)| i);
                last.text.truncate(end);
                for section in &mut last.sections {
                    section.byte_range.end = usize::from(section.byte_range.end).min(end).into();
                }
                last.sections.retain(|s| s.byte_range.start < s.byte_range.end);
            }
        } else {
            lines.push(shape(range.clone()));
        }
    }
    let mut galley = Galley::concat(original, &lines, ctx.pixels_per_point());
    galley.elided = elided;
    // Each Arabic line shares the right edge; Latin runs and numbers remain LTR.
    let width = galley.rect.width();
    galley.mesh_bounds = Rect::NOTHING;
    for row in &mut galley.rows {
        row.pos.x += width - row.size.x;
        galley.mesh_bounds |= row.visuals.mesh_bounds.translate(row.pos.to_vec2());
    }
    Arc::new(galley)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_fonts(test: impl FnOnce(&egui::Context)) {
        let ctx = egui::Context::default();
        // The built-in faces only: installed fonts (the CJK fallbacks) would make the glyphs
        // measured here depend on the machine.
        ctx.set_fonts(crate::theme::font_definitions(designcraft_fonts::CRAFT_FONTS, "ar"));
        ctx.begin_pass(egui::RawInput { max_texture_side: Some(8192), ..Default::default() });
        test(&ctx);
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
    }

    fn displayed(galley: &Galley) -> String {
        galley.rows.iter().flat_map(|row| row.glyphs.iter().filter(|g| g.advance_width > 0.0).map(|g| g.chr)).collect()
    }

    fn cache_job() -> LayoutJob {
        LayoutJob::simple("ملف (PDF 123) جديد للنشر والطباعة".repeat(4), FontId::proportional(15.0), Color32::WHITE, 150.0)
    }

    #[test]
    fn cache_reuses_final_layout_across_frames_and_matches_uncached_output() {
        with_fonts(|ctx| {
            let job = cache_job();
            let first = layout(ctx, job.clone());
            let reference = layout_uncached(ctx, job.clone());
            assert_eq!(displayed(&first), displayed(&reference));
            assert_eq!(first.rect, reference.rect);
            assert_eq!(first.rows.len(), reference.rows.len());
            assert!(Arc::ptr_eq(&first, &layout(ctx, job.clone())));
            ctx.end_pass().textures_delta.clear();
            ctx.begin_pass(egui::RawInput { max_texture_side: Some(8192), ..Default::default() });
            assert!(Arc::ptr_eq(&first, &layout(ctx, job)));
        });
    }

    #[test]
    fn cache_respects_text_formatting_wrapping_and_elision() {
        with_fonts(|ctx| {
            let job = cache_job();
            let first = layout(ctx, job.clone());
            let mut variants = Vec::new();
            let mut changed = job.clone();
            changed.wrap.max_width = 80.0;
            variants.push(changed);
            let mut changed = job.clone();
            changed.wrap.max_rows = 1;
            variants.push(changed);
            let mut changed = job.clone();
            changed.sections[0].format.font_id.size = 24.0;
            variants.push(changed);
            let mut changed = job.clone();
            changed.sections[0].format.color = Color32::RED;
            variants.push(changed);
            variants.push(LayoutJob::simple_singleline("עברית 123".into(), FontId::default(), Color32::WHITE));
            for changed in variants {
                let cached = layout(ctx, changed.clone());
                let reference = layout_uncached(ctx, changed);
                assert!(!Arc::ptr_eq(&first, &cached));
                assert_eq!(cached.job, reference.job);
                assert_eq!(cached.rect, reference.rect);
                assert_eq!(cached.elided, reference.elided);
                assert_eq!(displayed(&cached), displayed(&reference));
            }
        });
    }

    #[test]
    fn cache_invalidates_when_fonts_or_scale_change() {
        with_fonts(|ctx| {
            let job = cache_job();
            let first = layout(ctx, job.clone());
            ctx.end_pass().textures_delta.clear();
            ctx.set_pixels_per_point(2.0);
            ctx.begin_pass(egui::RawInput { max_texture_side: Some(8192), ..Default::default() });
            let scaled = layout(ctx, job.clone());
            assert!(!Arc::ptr_eq(&first, &scaled));
            ctx.end_pass().textures_delta.clear();
            let mut fonts = ctx.fonts(|fonts| fonts.definitions().clone());
            fonts.families.insert(FontFamily::Name("cache-test".into()), Vec::new());
            ctx.set_fonts(fonts);
            ctx.begin_pass(egui::RawInput { max_texture_side: Some(8192), ..Default::default() });
            let new_fonts = layout(ctx, job.clone());
            assert!(!Arc::ptr_eq(&scaled, &new_fonts));
            ctx.end_pass().textures_delta.clear();
            // Changing atlas options recreates the atlas without changing font definitions.
            ctx.begin_pass(egui::RawInput { max_texture_side: Some(4096), ..Default::default() });
            let new_atlas = layout(ctx, job.clone());
            assert!(!Arc::ptr_eq(&new_fonts, &new_atlas));
            assert_eq!(displayed(&new_atlas), displayed(&layout_uncached(ctx, job)));
        });
    }

    #[test]
    fn cache_is_bounded_and_font_token_replacement_invalidates_entries() {
        with_fonts(|ctx| {
            let job = cache_job();
            let galley = layout_uncached(ctx, job.clone());
            let token = ctx.fonts_mut(|fonts| fonts.layout_no_wrap(String::new(), FontId::default(), Color32::TRANSPARENT));
            let mut cache = LayoutCache::default();
            cache.lookup(token, 0, &job);
            for key in 0..=MAX_CACHED_LABELS as u64 {
                cache.clock += 1;
                cache.insert(key, Arc::clone(&galley));
            }
            assert_eq!(cache.entries.len(), MAX_CACHED_LABELS);
            assert!(!cache.entries.contains_key(&0));
            // Replacing the font-cache token models atlas recreation, even with identical fonts.
            let replacement = Arc::new((*galley).clone());
            assert!(cache.lookup(replacement, 1, &job).is_none());
            assert!(cache.entries.is_empty());
        });
    }

    #[test]
    #[ignore = "manual timing comparison, no machine-dependent pass threshold"]
    fn benchmark_cached_wrapping() {
        with_fonts(|ctx| {
            let job = cache_job();
            let _ = layout(ctx, job.clone());
            let start = std::time::Instant::now();
            for _ in 0..200 {
                std::hint::black_box(layout_uncached(ctx, job.clone()));
            }
            let uncached = start.elapsed();
            let start = std::time::Instant::now();
            for _ in 0..200 {
                std::hint::black_box(layout(ctx, job.clone()));
            }
            eprintln!("200 wrapped labels: uncached={uncached:?}, cached={:?}", start.elapsed());
        });
    }

    #[test]
    fn ordinary_multilingual_labels_skip_bidi_analysis() {
        for text in ["", "File (123)", "Édition", "中文排版", "スウォッチ", "e\u{301}", "🎨 🙂"] {
            assert!(!may_have_rtl(text), "{text:?}");
            assert!(!BidiInfo::new(text, None).has_rtl(), "{text:?}");
        }
        for text in ["ملف", "עברית", "PDF ملف 123", "\u{1e900}", "\u{200f}", "\u{202e}abc\u{202c}", "\u{2067}abc\u{2069}", "١٢٣"] {
            assert!(may_have_rtl(text), "{text:?}");
            assert_eq!(has_rtl(text), BidiInfo::new(text, None).has_rtl(), "{text:?}");
        }
    }

    #[test]
    fn non_rtl_widgets_keep_native_text_and_formatting() {
        with_fonts(|ctx| {
            egui::Area::new(egui::Id::new("rtl-fast-path-test")).show(ctx, |ui| {
                for text in ["中文", "日本語", "Édition", "🎨"] {
                    let rendered = widget(ui, egui::RichText::new(text).strong());
                    assert!(matches!(rendered, WidgetText::RichText(_)));
                    assert_eq!(rendered.text(), text);
                    assert!(label(ui, text).rect.is_finite());
                }
            });
        });
    }

    #[test]
    fn mixed_arabic_latin_digits_and_brackets_preserve_logical_source() {
        with_fonts(|ctx| {
            let text = "ملف (PDF 123) جديد";
            let galley = plain(ctx, text, FontId::proportional(15.0), Color32::WHITE);
            assert_eq!(galley.job.text, text);
            let visual = displayed(&galley);
            assert!(visual.contains("PDF 123"), "{visual}");
            let row = &galley.rows[0];
            let left_bracket = row.glyphs.iter().find(|g| g.chr == ')' && !g.uv_rect.is_nothing()).unwrap();
            // A neutral-only final bracket must be mirrored even without an Arabic script run.
            let job = LayoutJob::simple_singleline("ملف (PDF 123)".to_owned(), FontId::proportional(15.0), Color32::WHITE);
            let bidi = BidiInfo::new(&job.text, None);
            let visual_job = visual_line(&job, &bidi, 0..job.text.len());
            assert!(visual_job.text.starts_with("(PDF 123"), "{}", visual_job.text);
            let p = row.glyphs.iter().find(|g| g.chr == 'P').unwrap();
            let three = row.glyphs.iter().find(|g| g.chr == '3').unwrap();
            assert!(left_bracket.pos.x < p.pos.x && p.pos.x < three.pos.x);
            assert!(visual.starts_with('د'), "Arabic final word belongs at the left: {visual}");
            assert!(galley.rect.is_finite());
            assert!(galley.mesh_bounds.is_finite());
        });
    }

    #[test]
    fn isolates_arabic_names_from_latin_brand_and_zoom_suffix() {
        with_fonts(|ctx| {
            for text in [format!("DesignCraft — {}", isolate("ملف (PDF 123) جديد")), format!("{} @ 89%", isolate("ملف (PDF 123) جديد"))]
            {
                let galley = plain(ctx, &text, FontId::proportional(15.0), Color32::WHITE);
                let glyphs = &galley.rows[0].glyphs;
                let x = |c| glyphs.iter().find(|g| g.chr == c && g.advance_width > 0.0).unwrap().pos.x;
                assert!(x('ج') < x('P') && x('P') < x('ف'), "Arabic name must keep its internal RTL order");
                assert_eq!(galley.job.text, text);
            }
        });
    }

    #[test]
    fn joins_arabic_letters_using_one_font_including_spaces() {
        with_fonts(|ctx| {
            let text = "بِدء مُستند جَديد";
            let galley = plain(ctx, text, FontId::proportional(15.0), Color32::WHITE);
            let expected = ctx.fonts_mut(|fonts| {
                fonts.layout_job(LayoutJob::simple_singleline(text.to_owned(), FontId::new(15.0, FontFamily::Name("arabic".into())), Color32::WHITE))
            });
            assert_eq!(displayed(&galley), displayed(&expected));
            assert_eq!(galley.size(), expected.size());
            // Contextual glyphs (atlas positions) must match the unbroken Arabic shaping run.
            for (actual, expected) in galley.rows[0].glyphs.iter().zip(&expected.rows[0].glyphs) {
                assert_eq!(actual.uv_rect, expected.uv_rect);
            }
        });
    }

    #[test]
    fn wraps_first_logical_word_first_and_right_aligns_lines() {
        with_fonts(|ctx| {
            let text = "مستند جديد للطباعة";
            let width = plain(ctx, "مستند ", FontId::proportional(15.0), Color32::WHITE).size().x + 1.0;
            let mut job = LayoutJob::simple_singleline(text.to_owned(), FontId::proportional(15.0), Color32::WHITE);
            job.wrap = TextWrapping::wrap_at_width(width);
            let galley = layout(ctx, job);
            assert!(galley.rows.len() >= 3);
            let first: String = galley.rows[0].glyphs.iter().map(|g| g.chr).collect();
            assert!(first.contains('م') && first.contains('س') && !first.contains('ج'), "{first}");
            for row in &galley.rows {
                assert!((row.pos.x + row.size.x - galley.size().x).abs() < 0.01);
            }
            assert_eq!(galley.job.text, text);
        });
    }

    #[test]
    fn truncates_long_words_and_handles_empty_lines() {
        with_fonts(|ctx| {
            let mut job = LayoutJob::simple_singleline("مستندجديدللطباعة\n\nPDF 123".to_owned(), FontId::proportional(15.0), Color32::WHITE);
            job.wrap = TextWrapping::truncate_at_width(45.0);
            let galley = layout(ctx, job);
            assert_eq!(galley.rows.len(), 1);
            assert!(galley.elided);
            assert!(galley.size().x <= 45.0);
            assert!(displayed(&galley).contains('…'));
            let mut job = LayoutJob::simple_singleline("ملف\n\nجديد\n".to_owned(), FontId::proportional(15.0), Color32::WHITE);
            job.break_on_newline = true;
            let galley = layout(ctx, job);
            assert_eq!(galley.rows.len(), 4);
            assert!(galley.rect.is_finite());
            let galley = plain(ctx, "ملف\nجديد", FontId::proportional(15.0), Color32::WHITE);
            assert_eq!(galley.rows.len(), 1);
            assert_eq!(galley.job.text, "ملف\nجديد");
        });
    }
}
