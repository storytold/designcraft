//! Generated catchwords use the same visible source ranges as text variables, without
//! inserting markers into source stories or recursing through the running-header index.
use crate::{ComposeOptions, ComposedStory, FrameSpec, PlacedCatchword};
use designcraft_doc::{CharAttrs, Document, FirstBaseline, Leading, Story, StoryId, TextFrameOptions};
use designcraft_geom::{Point, Rect};
use std::sync::Arc;

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
        let Some(page) = spec.page else { continue };
        for (series, rect) in regions {
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
            let text = compose(doc, source, at, word, spec);
            let width = text.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).map(|g| g.x + g.adv).fold(0.0_f64, f64::max);
            let height = text.frames.first().map_or(0.0, |f| f.content_height);
            let scale = (rect.width() / width.max(0.01)).min(rect.height() / height.max(0.01)).min(1.0);
            let rtl = source.paras.get(source.para_at(at)).is_some_and(|p| doc.styles.resolve_para(p).0.direction == designcraft_doc::TextDirection::RightToLeft);
            let x = if rtl { rect.x0 } else { rect.x1 - width * scale };
            let origin = Point::new(x, rect.y0);
            frame.catchwords.push(PlacedCatchword {
                series: *series,
                rect: *rect,
                origin,
                scale,
                text: Arc::new(text),
                source: Arc::from(word.as_str()),
            });
            // Keep auto-size accounting consistent with the caption row reserved by page fitting.
            frame.content_height = frame.content_height.max(rect.y1 - spec.area.y0);
        }
    }
}

fn compose(doc: &Document, source: &Story, at: usize, word: &str, spec: &FrameSpec) -> ComposedStory {
    let base = source.paras.get(source.para_at(at)).map(|p| doc.styles.resolve_para(p).1).unwrap_or_default();
    let chars = doc.styles.resolve_char(&base, source.char_format_at(at));
    let mut text = Story::new(StoryId(0));
    text.insert(0, word);
    text.format_chars(0..word.len(), |f| {
        f.over = CharAttrs {
            font_family: Some(chars.font_family.clone()),
            font_style: Some(chars.font_style.clone()),
            fill: Some(chars.fill.clone()),
            size: Some(9.0),
            leading: Some(Leading::Points(12.0)),
            no_break: Some(true),
            ..Default::default()
        };
    });
    text.format_paras(0..word.len(), |p| {
        p.style = designcraft_doc::styles::NO_PARA_STYLE.into();
        p.para.hyphenate = Some(false);
    });
    let frame = FrameSpec {
        area: Rect::new(0.0, 0.0, 1.0e6, 1.0e6),
        opts: TextFrameOptions { first_baseline: FirstBaseline::Ascent, first_baseline_min: 9.0, ..Default::default() },
        exclusions: Vec::new(),
        outline: None,
        grid: None,
        ..spec.clone()
    };
    crate::compose(doc, &text, &[frame], &ComposeOptions { inset_measure: true, ..Default::default() })
}
