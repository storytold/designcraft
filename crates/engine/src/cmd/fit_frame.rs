//! One-shot frame fitting. Composition, not character counts, determines the text's size.
use designcraft_compose::{ComposeOptions, ComposedStory, FrameSpec};
use designcraft_doc::{Content, Document, Item, Shape, Story};
use designcraft_geom::{Rect, shapes};
use serde_json::{Value, json};

use super::{bad, targets};
use crate::{Result, Session};

const ID: &str = "object.fit";
const LIMIT: f64 = 1_000_000.0;
const EPS: f64 = 0.01;

pub(super) fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    // Session::edit is transactional: a failure on a later frame changes none of the targets.
    s.edit(|doc, _| {
        let mut changed = 0;
        for id in &ids {
            let item = doc.item(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            if item.locked || doc.layer(item.layer).is_some_and(|l| l.locked) {
                continue;
            }
            let bounds = match &item.content {
                Content::Graphic(g) => Some(g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1))),
                Content::Text(_) => Some(text_bounds(doc, item)?),
                _ => None,
            };
            if let Some(r) = bounds {
                if !valid(r) {
                    return Err(bad(ID, "content has invalid or excessively large bounds"));
                }
                if r != item.inner_bounds() {
                    doc.item_mut(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?.path = shapes::rectangle(r);
                    changed += 1;
                }
            }
        }
        Ok(json!({"changed": changed}))
    })
}

fn valid(r: Rect) -> bool {
    [r.x0, r.y0, r.x1, r.y1].into_iter().all(f64::is_finite) && r.width() > 0.0 && r.height() > 0.0 && r.width() <= LIMIT && r.height() <= LIMIT
}

fn text_bounds(doc: &Document, item: &Item) -> Result<Rect> {
    let tf = item.text_frame().ok_or_else(|| bad(ID, "not a text frame"))?;
    if item.shape != Shape::Rectangle || tf.options.path.is_some() {
        return Err(bad(ID, "text fitting requires a rectangular text frame, not type on a path"));
    }
    let outer = item.inner_bounds();
    let inner = item.text_area();
    if !valid(outer) || !valid(inner) {
        return Err(bad(ID, "text frame has no usable text area"));
    }
    let story = doc.story(tf.story).ok_or_else(|| bad(ID, "text story is missing"))?;
    let mut frames = designcraft_compose::frame_specs(doc, story.id);
    let at = frames.iter().position(|f| f.id == item.id).ok_or_else(|| bad(ID, "frame is missing from its story"))?;
    let initial = designcraft_compose::compose(doc, story, &frames, &ComposeOptions::default());
    let original = initial.frame(item.id).ok_or_else(|| bad(ID, "frame could not be composed"))?;
    let last = at + 1 == frames.len();
    // A linked frame fits its existing text, not the following frame's entire story.
    let end = if last { story.len() } else { original.range.end };
    if !last && original.lines.is_empty() {
        return Ok(outer);
    }
    let mut fit = Probe { doc, story, frames: &mut frames, at, end, last, overset_floor: initial.overset_at };
    let mut height = fit.area()?.height();
    let mut composed = fit.compose();
    for _ in 0..21 {
        if fit.contains(&composed) {
            break;
        }
        height = (height * 2.0).min(LIMIT);
        fit.set_height(height)?;
        composed = fit.compose();
    }
    if !fit.contains(&composed) {
        return Err(bad(ID, "text cannot fit within the size limit; check forced breaks or oversized content"));
    }
    // A single standalone line can also hug its width. Paragraphs keep their wrapping width.
    let single = composed.frame(item.id).is_some_and(|f| {
        f.lines.len() == 1
            && f.tables.is_empty()
            && f.notes.is_empty()
            && f.objects.is_empty()
            && f.lines.first().is_some_and(|l| l.drop_cap.is_none())
    });
    if single
        && fit.frames.len() == 1
        && tf.options.columns == 1
        && tf.options.columns_kind != designcraft_doc::ColumnsKind::FixedWidth
        && fit.frames.get(at).is_some_and(|f| f.exclusions.is_empty())
        && !story.text.is_empty()
    {
        let mut lo = 0.0;
        let mut hi = fit.area()?.width();
        for _ in 0..30 {
            if hi - lo <= EPS {
                break;
            }
            let mid = (hi + lo) / 2.0;
            fit.set_width(mid)?;
            let c = fit.compose();
            let inside = c.frame(item.id).is_some_and(|f| {
                f.lines.len() == 1
                    && f.lines.iter().all(|l| l.glyphs.iter().all(|g| g.x >= l.x0 - EPS && g.x + g.adv <= l.x1 + EPS) && l.end_x <= l.x1 + EPS)
            });
            if fit.contains(&c) && inside {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        fit.set_width(hi)?;
    }
    let mut lo = 0.0;
    let mut hi = height;
    for _ in 0..30 {
        if hi - lo <= EPS {
            break;
        }
        let mid = (hi + lo) / 2.0;
        fit.set_height(mid)?;
        if fit.contains(&fit.compose()) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    // Stay just on the fitting side of floating-point line-break thresholds.
    fit.set_height(hi + EPS)?;
    if !fit.contains(&fit.compose()) {
        return Err(bad(ID, "could not find safe text bounds; frame left unchanged"));
    }
    let area = fit.area()?;
    let (w, h) = if fit.frames.get(at).is_some_and(|f| f.vertical) { (area.height(), area.width()) } else { (area.width(), area.height()) };
    Ok(Rect::new(outer.x0, outer.y0, outer.x0 + w + outer.width() - inner.width(), outer.y0 + h + outer.height() - inner.height()))
}

struct Probe<'a> {
    doc: &'a Document,
    story: &'a Story,
    frames: &'a mut [FrameSpec],
    at: usize,
    end: usize,
    last: bool,
    overset_floor: Option<usize>,
}

impl Probe<'_> {
    fn compose(&self) -> ComposedStory {
        designcraft_compose::compose(self.doc, self.story, self.frames, &ComposeOptions::default())
    }

    fn contains(&self, c: &ComposedStory) -> bool {
        self.frames.get(self.at).and_then(|frame| c.frame(frame.id)).is_some_and(|f| {
            !f.lines.is_empty()
                && f.range.end >= self.end
                && f.notes.iter().all(|n| !n.text.is_overset())
                && f.tables.iter().all(|t| t.cells.iter().all(|c| !c.overset))
        }) && (!self.last || !c.is_overset())
            && match (self.overset_floor, c.overset_at) {
                (_, None) => true,
                (Some(old), Some(new)) => new >= old,
                (None, Some(_)) => false,
            }
    }

    fn area(&self) -> Result<Rect> {
        self.frames.get(self.at).map(|f| f.area).ok_or_else(|| bad(ID, "frame is missing from its story"))
    }

    fn set_height(&mut self, h: f64) -> Result<()> {
        let frame = self.frames.get_mut(self.at).ok_or_else(|| bad(ID, "frame is missing from its story"))?;
        frame.area.y1 = frame.area.y0 + h;
        Ok(())
    }

    fn set_width(&mut self, w: f64) -> Result<()> {
        let frame = self.frames.get_mut(self.at).ok_or_else(|| bad(ID, "frame is missing from its story"))?;
        frame.area.x1 = frame.area.x0 + w;
        Ok(())
    }
}
