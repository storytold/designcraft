//! Generated catchwords use the same visible source ranges as text variables, without
//! inserting markers into source stories or recursing through the running-header index.
use crate::{ComposeOptions, ComposedStory, FrameSpec, PlacedCatchword};
use designcraft_doc::{CharAttrs, Document, FirstBaseline, Leading, Story, StoryId, TextFrameOptions};
use designcraft_geom::{Point, Rect};
use std::sync::Arc;

pub(super) fn options(
    doc: &Document,
    spec: &FrameSpec,
    series: Option<u64>,
    host: &designcraft_doc::InsetCatchwords,
) -> designcraft_doc::InsetCatchwords {
    let route = spec.opts.note_stream.as_ref().filter(|r| Some(r.series) == series).and_then(|r| r.catchword.style());
    let style = route.or_else(|| series.and_then(|id| doc.note_options(id).catchword_style.as_deref()));
    style.map_or_else(|| host.clone(), |style| designcraft_doc::InsetCatchwords::Styled { style: style.into() })
}

fn attributes(doc: &Document, source: &Story, at: usize, options: &designcraft_doc::InsetCatchwords) -> CharAttrs {
    let base = source.paras.get(source.para_at(at)).map(|p| doc.styles.resolve_para(p).1).unwrap_or_default();
    let chars = doc.styles.resolve_char(&base, source.char_format_at(at));
    let mut attrs = CharAttrs {
        font_family: Some(chars.font_family),
        font_style: Some(chars.font_style),
        fill: Some(chars.fill),
        no_break: Some(true),
        size: Some(9.0),
        leading: Some(Leading::Points(12.0)),
        ..Default::default()
    };
    if let Some(style) = options.style().filter(|s| doc.styles.char_style(s).is_some()) {
        attrs.size = Some(chars.size);
        attrs.leading = Some(chars.leading);
        attrs.merge(&doc.styles.char_style_attrs(style));
    }
    attrs
}

fn styled(doc: &Document, options: &designcraft_doc::InsetCatchwords) -> bool {
    options.style().is_some_and(|style| doc.styles.char_style(style).is_some())
}

pub(super) fn last_line(frame: &crate::FrameText, series: Option<u64>) -> Option<(f64, f64)> {
    if let Some(series) = series {
        frame
            .notes
            .iter()
            .filter(|n| n.series == series)
            .flat_map(|n| n.text.frames.iter().flat_map(move |f| f.lines.iter().map(move |l| (n.origin.y + l.baseline, l.descent))))
            .max_by(|a, b| a.0.total_cmp(&b.0))
    } else {
        frame.lines.last().map(|l| (l.baseline, l.descent))
    }
}

/// Measure the inherited catchword face before the next page's visible word is known.
pub(super) fn row_space(
    doc: &Document,
    source: &Story,
    at: usize,
    spec: &FrameSpec,
    options: &designcraft_doc::InsetCatchwords,
    previous_descent: f64,
) -> f64 {
    if !styled(doc, options) {
        return super::fitting::CATCHWORD_SPACE;
    }
    let sample = compose(doc, source, at, "\u{00a0}", spec, options);
    let descent = sample.frames.first().and_then(|f| f.lines.first()).map_or(previous_descent, |l| l.descent);
    (sample.frames.first().and_then(|f| f.lines.first()).map_or(12.0, |l| l.leading) + descent - previous_descent).max(0.0)
}

pub(super) fn row_height(doc: &Document, source: &Story, at: usize, options: &designcraft_doc::InsetCatchwords) -> f64 {
    if options.style().is_none_or(|style| doc.styles.char_style(style).is_none()) {
        return super::fitting::CATCHWORD_SPACE;
    }
    let attrs = attributes(doc, source, at, options);
    let leading = match attrs.leading {
        Some(Leading::Points(value)) => value,
        _ => attrs.size.unwrap_or(9.0) * 1.2,
    };
    if leading.is_finite() && leading > 0.0 && leading <= 1.0e6 { leading } else { super::fitting::CATCHWORD_SPACE }
}

#[cfg(test)]
thread_local! {
    pub(super) static LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn place(doc: &Document, story: &Story, out: &mut ComposedStory, frames: &[FrameSpec], areas: &[Vec<(Option<u64>, Rect)>]) {
    if areas.iter().all(Vec::is_empty) {
        return;
    }
    #[cfg(test)]
    LOOKUPS.with(|n| n.set(n.get() + 1));
    let main = crate::vars::story_catchwords(doc, out, story);
    let mut words = std::collections::HashMap::new();
    for id in areas.iter().flatten().filter_map(|(id, _)| *id) {
        words.entry(id).or_insert_with(|| crate::vars::series_catchwords(doc, out, id));
    }
    for (spec, regions) in frames.iter().zip(areas) {
        let options = spec.opts.inset_columns.as_ref().map(|c| c.catchwords.clone()).unwrap_or_default();
        let Some(page) = spec.page else { continue };
        for (series, rect) in regions {
            let options = self::options(doc, spec, *series, &options);
            if let Some(style) = options.style().filter(|s| doc.styles.char_style(s).is_none()) {
                let warning = format!("Unknown catchword character style `{style}`; using default 9 pt size and 12 pt leading");
                if !out.warnings.contains(&warning) {
                    out.warnings.push(warning);
                }
            }
            let word = match series {
                Some(id) => words.get(id).and_then(|pages| pages.get(&page)),
                None => main.get(&page).map(|(_, word)| word),
            };
            let Some(word) = word.filter(|s| !s.is_empty()) else { continue };
            let Some(frame) = out.frames.iter_mut().find(|f| f.frame == spec.id) else { continue };
            let part = series
                .and_then(|id| frame.notes.iter().rev().find(|n| n.series == id))
                .and_then(|n| story.notes.get(n.index).map(|source| (&source.text, n.text.frames.last().map_or(0, |f| f.range.end))));
            let (source, at) = part.unwrap_or((story, frame.range.end));
            let text = compose(doc, source, at, word, spec, &options);
            let width = text.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).map(|g| g.x + g.adv).fold(0.0_f64, f64::max);
            let scale = (rect.width() / width.max(0.01)).min(1.0);
            let rtl = source
                .paras
                .get(source.para_at(at))
                .is_some_and(|p| doc.styles.resolve_para(p).0.direction == designcraft_doc::TextDirection::RightToLeft);
            let x = if rtl { rect.x0 } else { rect.x1 - width * scale };
            let (rect, y) = if styled(doc, &options)
                && let Some((previous, descent)) = last_line(frame, *series)
                && let Some(line) = text.frames.first().and_then(|f| f.lines.first())
            {
                let baseline = previous + line.leading;
                let top = previous + descent;
                let rect = Rect::new(rect.x0, top, rect.x1, (baseline + line.descent * scale).max(top));
                (rect, baseline - line.baseline * scale)
            } else {
                (*rect, rect.y0)
            };
            let origin = Point::new(x, y);
            frame.catchwords.push(PlacedCatchword { series: *series, rect, origin, scale, text: Arc::new(text), source: Arc::from(word.as_str()) });
            // Keep auto-size accounting consistent with the caption row reserved by page fitting.
            frame.content_height = frame.content_height.max(rect.y1 - spec.area.y0);
        }
    }
}

fn compose(doc: &Document, source: &Story, at: usize, word: &str, spec: &FrameSpec, options: &designcraft_doc::InsetCatchwords) -> ComposedStory {
    let attrs = attributes(doc, source, at, options);
    let mut text = Story::new(StoryId(0));
    text.insert(0, word);
    text.format_chars(0..word.len(), |f| f.over = attrs.clone());
    text.format_paras(0..word.len(), |p| {
        p.style = designcraft_doc::styles::NO_PARA_STYLE.into();
        p.para.hyphenate = Some(false);
    });
    let frame = FrameSpec {
        area: Rect::new(0.0, 0.0, 1.0e6, 1.0e6),
        opts: TextFrameOptions { first_baseline: FirstBaseline::Ascent, first_baseline_min: attrs.size.unwrap_or(9.0), ..Default::default() },
        exclusions: Vec::new(),
        outline: None,
        grid: None,
        ..spec.clone()
    };
    crate::compose(doc, &text, &[frame], &ComposeOptions { inset_measure: true, ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styled_row_uses_leading_and_boolean_keeps_legacy_space() {
        let mut doc = Document::new(&Default::default());
        Arc::make_mut(&mut doc.styles).character.push(designcraft_doc::styles::CharacterStyle {
            name: "Catchword".into(),
            based_on: None,
            shortcut: String::new(),
            chars: CharAttrs { size: Some(15.0), leading: Some(Leading::Points(24.0)), ..Default::default() },
        });
        assert_eq!(row_height(&doc, &Story::new(StoryId(0)), 0, &true.into()), 18.0);
        assert_eq!(row_height(&doc, &Story::new(StoryId(0)), 0, &designcraft_doc::InsetCatchwords::Styled { style: "Catchword".into() }), 24.0);
        assert_eq!(row_height(&doc, &Story::new(StoryId(0)), 0, &designcraft_doc::InsetCatchwords::Styled { style: "Missing".into() }), 18.0);
    }
}
