//! Read-only interface text: Unicode bidi run ordering followed by Arabic shaping.
//! Source strings stay logical. Never use these galleys for editable text/caret mapping.
use egui::text::{LayoutJob, TextWrapping};
use egui::{Align, Align2, Color32, FontFamily, FontId, Galley, Pos2, Rect, WidgetText};
use std::{ops::Range, sync::Arc};
use unicode_bidi::BidiInfo;
use unicode_segmentation::UnicodeSegmentation;

/// Resolve translated labels through bidi, without changing IDs, commands or stored text.
pub fn widget(ui: &egui::Ui, text: impl Into<WidgetText>) -> WidgetText {
    let text = text.into();
    if text.text().is_ascii() || !BidiInfo::new(text.text(), None).has_rtl() {
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
    let rtl = !text.text().is_ascii() && BidiInfo::new(text.text(), None).has_rtl();
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

fn layout(ctx: &egui::Context, mut job: LayoutJob) -> Arc<Galley> {
    if job.text.is_ascii() || !BidiInfo::new(&job.text, None).has_rtl() {
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
        crate::theme::install_fonts(&ctx, "ar");
        ctx.begin_pass(egui::RawInput { max_texture_side: Some(8192), ..Default::default() });
        test(&ctx);
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
    }

    fn displayed(galley: &Galley) -> String {
        galley.rows.iter().flat_map(|row| row.glyphs.iter().filter(|g| g.advance_width > 0.0).map(|g| g.chr)).collect()
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
