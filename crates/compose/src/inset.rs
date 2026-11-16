//! Geometry for several text streams sharing one frame. Text measurement owns source offsets,
//! paragraph state and baseline progression; only measurements clipped to a band are committed.

use designcraft_geom::Rect;
mod catchwords;
mod fitting;
pub(crate) use fitting::{Trace, compose_frames};

const MAX_STREAMS: usize = 64;
const EPS: f64 = 1e-7;

#[derive(Clone, Copy, Debug)]
pub struct MainPosition {
    /// Distance below the frame top, resolved from the main text's initial line metrics.
    pub top: f64,
    pub left: f64,
    pub right: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub bounds: Rect,
    pub gutter: f64,
    pub main: usize,
    /// Streams are ordered spine outward. Mirror only for PageSide::Left. Single pages use
    /// the unmirrored case. Paragraph direction does not affect region geometry.
    pub mirror: bool,
    pub position: Option<MainPosition>,
}

#[derive(Clone, Debug)]
pub struct Stream<C> {
    pub cursor: C,
    pub ratio: f64,
    pub finished: bool,
}

#[derive(Clone, Debug)]
pub struct Measurement<C, P> {
    pub cursor: C,
    /// Height actually used, including the last line's descent, relative to the offered top.
    pub height: f64,
    pub finished: bool,
    pub placement: P,
}

#[derive(Clone, Debug)]
pub struct Part<P> {
    pub stream: usize,
    pub rect: Rect,
    pub placement: P,
}

#[derive(Clone, Debug)]
pub struct Layout<C, P> {
    pub streams: Vec<Stream<C>>,
    pub parts: Vec<Part<P>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidGeometry,
    InvalidMeasurement,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidGeometry => "invalid inset-column geometry",
            Self::InvalidMeasurement => "invalid inset-column measurement",
        })
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy)]
struct Region {
    stream: usize,
    left: f64,
    right: f64,
}

fn split<C>(streams: &[Stream<C>], indices: &[usize], left: f64, right: f64, gutter: f64) -> Result<Vec<Region>, Error> {
    if indices.is_empty() {
        return Ok(Vec::new());
    }
    let width = right - left - gutter * indices.len().saturating_sub(1) as f64;
    let total: f64 = indices.iter().filter_map(|&i| streams.get(i)).map(|s| s.ratio).sum();
    if width <= 0.0 || !total.is_finite() {
        return Err(Error::InvalidGeometry);
    }
    let mut x = left;
    let mut regions = Vec::with_capacity(indices.len());
    for &stream in indices {
        let ratio = streams.get(stream).ok_or(Error::InvalidGeometry)?.ratio;
        let end = x + width * (ratio / total);
        if !end.is_finite() || end <= x {
            return Err(Error::InvalidGeometry);
        }
        regions.push(Region { stream, left: x, right: end });
        x = end + gutter;
    }
    Ok(regions)
}

/// Absorb holes between surviving neighbours, preserving every other live column. If the main
/// text touches a hole it gets that space. Otherwise the two neighbours share it by their ratios.
fn close_holes<C>(cfg: Config, streams: &[Stream<C>], regions: &mut [Region]) {
    if let Some(first) = regions.first_mut() {
        first.left = cfg.bounds.x0;
    }
    if let Some(last) = regions.last_mut() {
        last.right = cfg.bounds.x1;
    }
    for i in 1..regions.len() {
        let (before, after) = regions.split_at_mut(i);
        let (Some(left), Some(right)) = (before.last_mut(), after.first_mut()) else { continue };
        let extra = right.left - left.right - cfg.gutter;
        if extra <= EPS {
            continue;
        }
        let share = if left.stream == cfg.main {
            1.0
        } else if right.stream == cfg.main {
            0.0
        } else {
            let l = streams.get(left.stream).map_or(1.0, |s| s.ratio);
            let r = streams.get(right.stream).map_or(1.0, |s| s.ratio);
            l / (l + r)
        };
        left.right += extra * share;
        right.left = left.right + cfg.gutter;
    }
}

fn initial<C>(cfg: Config, streams: &[Stream<C>], with_main: bool) -> Result<Vec<Region>, Error> {
    let live: Vec<_> = streams.iter().enumerate().filter(|(i, s)| !s.finished && (with_main || *i != cfg.main)).map(|(i, _)| i).collect();
    let Some(pos) = cfg.position.filter(|_| with_main && streams.get(cfg.main).is_some_and(|s| !s.finished)) else {
        return split(streams, &live, cfg.bounds.x0, cfg.bounds.x1, cfg.gutter);
    };
    let left = cfg.bounds.x0 + pos.left;
    let right = cfg.bounds.x1 - pos.right;
    let inner: Vec<_> = live.iter().copied().filter(|&i| i < cfg.main).collect();
    let outer: Vec<_> = live.iter().copied().filter(|&i| i > cfg.main).collect();
    let mut regions = split(streams, &inner, cfg.bounds.x0, left - cfg.gutter, cfg.gutter)?;
    regions.push(Region { stream: cfg.main, left, right });
    regions.extend(split(streams, &outer, right + cfg.gutter, cfg.bounds.x1, cfg.gutter)?);
    close_holes(cfg, streams, &mut regions);
    Ok(regions)
}

fn rectangle(cfg: Config, region: Region, top: f64, bottom: f64) -> Rect {
    let (left, right) = if cfg.mirror {
        (cfg.bounds.x0 + (cfg.bounds.x1 - region.right), cfg.bounds.x0 + (cfg.bounds.x1 - region.left))
    } else {
        (region.left, region.right)
    };
    Rect::new(left, top, right, bottom)
}

fn valid_measurement<C, P>(m: &Measurement<C, P>, height: f64) -> bool {
    m.height.is_finite() && m.height >= 0.0 && m.height <= height + EPS
}

/// Lay out fixed page slices. `measure` must be deterministic and non-consuming: it preserves
/// the cursor's baseline and paragraph state, fits only whole lines, and returns a new cursor.
/// An offered top is a clipping boundary, not a request to restart first-baseline padding.
/// Speculative placements are discarded; only the clipped remeasurements appear in `parts`.
pub fn layout<C, P>(
    cfg: Config,
    mut streams: Vec<Stream<C>>,
    mut measure: impl FnMut(usize, &C, Rect) -> Measurement<C, P>,
) -> Result<Layout<C, P>, Error> {
    let b = cfg.bounds;
    if streams.len() > MAX_STREAMS
        || cfg.main >= streams.len()
        || ![b.x0, b.y0, b.x1, b.y1, b.width(), b.height(), cfg.gutter].iter().all(|n| n.is_finite())
        || b.width() <= 0.0
        || b.height() <= 0.0
        || cfg.gutter < 0.0
        || streams.iter().any(|s| !s.ratio.is_finite() || s.ratio <= 0.0)
        || cfg.position.is_some_and(|p| ![p.top, p.left, p.right].iter().all(|n| n.is_finite() && *n >= 0.0) || p.left + p.right >= b.width())
    {
        return Err(Error::InvalidGeometry);
    }
    let start = cfg.position.map_or(b.y0, |p| b.y0 + p.top);
    let mut started = start <= b.y0 || streams.get(cfg.main).is_some_and(|s| s.finished);
    let mut regions = initial(cfg, &streams, started)?;
    let mut top = b.y0;
    let mut parts = Vec::new();
    // Each pass ends at least one stream or starts main. No text-sized iteration or allocation.
    for _ in 0..streams.len().saturating_add(2) {
        if top >= b.y1 {
            break;
        }
        if !started && top >= start {
            started = true;
            regions = initial(cfg, &streams, true)?;
        }
        if regions.is_empty() {
            if !started && start < b.y1 {
                top = start;
                continue;
            }
            break;
        }
        let deadline = if started { b.y1 } else { start.min(b.y1) };
        let mut bottom = deadline;
        for region in &regions {
            let cursor = &streams.get(region.stream).ok_or(Error::InvalidGeometry)?.cursor;
            let rect = rectangle(cfg, *region, top, deadline);
            let m = measure(region.stream, cursor, rect);
            if !valid_measurement(&m, rect.height()) {
                return Err(Error::InvalidMeasurement);
            }
            if m.finished {
                bottom = bottom.min(top + m.height);
            }
        }
        let mut ended = false;
        for region in &regions {
            let stream = streams.get_mut(region.stream).ok_or(Error::InvalidGeometry)?;
            let rect = rectangle(cfg, *region, top, bottom);
            let m = measure(region.stream, &stream.cursor, rect);
            if !valid_measurement(&m, rect.height()) {
                return Err(Error::InvalidMeasurement);
            }
            stream.cursor = m.cursor;
            stream.finished = m.finished;
            ended |= m.finished;
            if m.height > 0.0 {
                parts.push(Part { stream: region.stream, rect: Rect::new(rect.x0, rect.y0, rect.x1, rect.y0 + m.height), placement: m.placement });
            }
        }
        regions.retain(|r| streams.get(r.stream).is_some_and(|s| !s.finished));
        close_holes(cfg, &streams, &mut regions);
        top = bottom;
        if !ended && (started || top < start) {
            break;
        }
    }
    Ok(Layout { streams, parts })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod composition_tests;
