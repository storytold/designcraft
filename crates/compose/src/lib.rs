//! DesignCraft text composition.
//!
//! [`compose_story`] lays a story out through its chain of threaded frames:
//! style resolution → shaping ([`shape`]) → line breaking ([`breaker`]: the paragraph composer is
//! Knuth–Plass total fit, the single-line composer is greedy) → placement in columns and frames
//! (first-baseline offset, leading, space before/after, baseline grid, text wrap, column/frame
//! breaks, vertical justification) → overset detection.
//!
//! Output coordinates are each frame's *inner* space (apply the frame item's `xf` for spread space).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod bidi;
pub mod breaker;
mod cache;
pub mod grep;
mod grid;
pub mod hyphen;
pub mod kenten;
mod mojikumi;
mod notes;
pub mod optical;
mod outline;
mod overlay;
mod parabox;
mod ruby;
pub mod shape;
pub mod shatai;
pub mod table;
pub mod vars;
mod warichu;
pub mod wrap;
pub mod xref;

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use designcraft_doc::{
    Align, Content, Document, FirstBaseline, GridAlign, Item, ItemId, ListAlign, ListCounter, ParaProps, SpanColumns, StartParagraph, Story, StoryId,
    TabAlign, TextFrameOptions, VerticalJustification, WrapMode, WrapSide, story,
};
use designcraft_fonts::{FontDb, ScopedFonts};
use designcraft_geom::{Affine, Point, Rect};

use crate::breaker::{Break, Spacing};
pub use crate::cache::{Cache, ShownOn};
use crate::notes::Notes;
pub use crate::notes::{find_note, hit_note, hit_note_with, note_caret};
pub use crate::outline::FrameOutline;
use crate::parabox::ParaBox;
use crate::shape::{Glyph, StyleTable, SubstCtx};
pub use crate::table::{BorderHit, PlacedCell, StrokeSeg, TableFrag, cell_caret, find_cell, hit_border, hit_cell, hit_cell_with};

/// Character appearance shared by many glyphs (indexed from [`PlacedGlyph::style`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RunStyle {
    pub fill: String,
    pub fill_tint: f32,
    pub stroke: String,
    pub stroke_tint: f32,
    pub stroke_weight: f64,
    /// The stroke goes under the fill at double width, so its full weight shows outside the glyphs.
    pub stroke_behind_fill: bool,
    pub underline: bool,
    pub strikethrough: bool,
    /// The underline and strikethrough bars (Underline / Strikethrough Options resolved).
    pub underline_rule: Rule,
    pub strike_rule: Rule,
    pub skew: f64,
    pub size: f64,
    /// The run's font isn't installed (shown in a substitute; highlighted on screen).
    pub missing_font: bool,
    /// Tracking or manual kerning (Highlight Custom Tracking/Kerning).
    pub custom_tracking: bool,
    /// The first condition applied (its indicator colour underlines the text on screen).
    pub condition: Option<String>,
    /// Added while tracking changes (marked on screen).
    pub inserted: bool,
    /// XML element the text is tagged with (tag markers on screen).
    pub xml_tag: Option<String>,
    /// Ruby over the run.
    pub ruby: Option<String>,
    /// The ruby's settings (set with `ruby`), and for per-character ruby the parent character's
    /// place in its run (see `ruby::reserve`).
    pub ruby_spec: Option<designcraft_doc::ruby::RubySpec>,
    pub ruby_unit: Option<u32>,
    /// Overprint the fill and stroke (ruby glyphs set to overprint).
    pub overprint_fill: bool,
    pub overprint_stroke: bool,
    /// Warichu: stack this run in smaller lines inside the parent em.
    pub warichu: bool,
    pub warichu_lines: u32,
    /// Percentage of the parent size.
    pub warichu_size: f64,
    /// Extra points between warichu baselines. 0 keeps baselines one small em apart. A negative
    /// value tightens that em, down to a shared baseline.
    pub warichu_line_spacing: f64,
    pub warichu_align: designcraft_doc::cjk::WarichuAlignment,
    pub warichu_chars_before: u32,
    pub warichu_chars_after: u32,
    /// Kenten Settings and Kenten Color resolved (`None` without kenten); see [`kenten`].
    pub kenten_mark: Option<kenten::KentenMark>,
    /// Shatai (`None` when off); see [`shatai`] and [`RunStyle::shatai_xf`].
    pub shatai: Option<shatai::Shatai>,
}

/// An underline or strikethrough bar: its centre `offset` below the baseline (negative =
/// above), thickness and colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub offset: f64,
    pub weight: f64,
    pub color: String,
    pub tint: f32,
}

impl Rule {
    /// The stroke is centred on the offset, matching Underline / Strikethrough Options.
    pub fn rect(&self, x0: f64, x1: f64, baseline: f64) -> Rect {
        let center = baseline + self.offset;
        Rect::new(x0, center - self.weight / 2.0, x1, center + self.weight / 2.0)
    }
}

impl RunStyle {
    /// The bars this run draws between `x0` and `x1` on a line at `baseline`.
    pub fn rules(&self, x0: f64, x1: f64, baseline: f64) -> impl Iterator<Item = (&Rule, Rect)> {
        [(self.underline, &self.underline_rule), (self.strikethrough, &self.strike_rule)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(move |(_, r)| (r, r.rect(x0, x1, baseline)))
    }
}

/// A positioned glyph. `x` is absolute in frame inner space; `y` is relative to the line baseline.
#[derive(Clone, Debug)]
pub struct PlacedGlyph {
    /// Actual character drawn, including generated labels.
    pub rendered_char: char,
    pub face: designcraft_fonts::FaceRef,
    pub gid: u32,
    pub x: f64,
    pub y: f64,
    pub adv: f64,
    pub sx: f64,
    pub sy: f64,
    pub style: u32,
    pub byte: usize,
    pub len: usize,
    /// Text of a generated glyph (see [`shape::Glyph::generated_text`]).
    pub generated_text: Option<Box<str>>,
    /// Control characters (tabs, breaks, markers' carriers) are not drawn.
    pub visible: bool,
    /// Stays upright in vertical frames (CJK ideographs, kana, hangul, full-width forms).
    pub upright: bool,
    /// Tate-chu-yoko placement (see [`shape::Glyph::tcy`]).
    pub tcy: Option<[f64; 3]>,
    /// Set right to left (an odd bidi level): the caret before it is at its right edge.
    pub rtl: bool,
    /// How far after its pen position the glyph is drawn (CJK aki, ruby parent spacing): its
    /// advance starts at `x - dx`.
    pub dx: f64,
}

impl PlacedGlyph {
    /// Whether an underline or a strikethrough runs across this glyph.
    ///
    /// A tab is not drawn, but it is a character with an advance, and the rule runs across its
    /// whole width as InDesign draws it: forms rule their fill-in lines with underlined tabs, and
    /// without this they lost every writing line (#1466). The spaces a line break trimmed off the
    /// end, the break characters themselves and the markers' carriers are not part of the line's
    /// text and are not spanned.
    pub fn spans_rules(&self) -> bool {
        if self.visible {
            return true;
        }
        self.adv > 0.0 && matches!(self.rendered_char, '\t' | designcraft_doc::story::RIGHT_INDENT_TAB)
    }

    /// In a vertical frame, the turn that sets this glyph upright, applied after drawing it at `x`
    /// on `baseline`: it hangs from its vertical origin ([`designcraft_fonts::FontFace::v_origin`],
    /// centred across) at `x` on the line's centre, the middle of the font's em box; a tate-chu-yoko
    /// group sits across that centre.
    pub fn vertical_xf(&self, baseline: f64) -> Option<designcraft_geom::Affine> {
        let turn = -std::f64::consts::FRAC_PI_2;
        // The em box centre above the baseline, in ems.
        let (top, bottom) = self.face.em_box();
        let centre = (top + bottom) / 2.0 / self.face.units_per_em();
        if let Some([along, across, em]) = self.tcy {
            let c = Point::new(self.x + along, baseline + self.y - em * centre);
            return Some(designcraft_geom::Affine::rotate_about(turn, c) * designcraft_geom::Affine::translate((c.x + across - self.x, 0.0)));
        }
        if !self.upright {
            return None;
        }
        // Turning about c takes the glyph's vertical origin (half across, `origin` up) to the
        // centre point at `x`: c is where the two points' perpendicular bisector meets the turn.
        let half = self.face.advance(self.gid) / 2.0 * self.sx;
        let origin = self.face.v_origin(self.gid) * self.sy;
        let mid = centre * self.face.units_per_em() * self.sy;
        let c = Point::new(self.x + (origin - mid + half) / 2.0, baseline + self.y + (half - mid - origin) / 2.0);
        Some(designcraft_geom::Affine::rotate_about(turn, c))
    }
}

/// Characters set upright (unrotated) in vertical text.
pub fn upright_in_vertical(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF | 0x2E80..=0x2FFF | 0x3000..=0x30FF | 0xFE10..=0xFE4F | 0x3100..=0x31FF | 0x3200..=0x9FFF
        | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xF900..=0xFAFF | 0xFF01..=0xFF60 | 0xFFE0..=0xFFE6
        | 0x20000..=0x3FFFF)
}

#[derive(Clone, Debug)]
pub struct Line {
    pub column: u32,
    /// Baseline y in frame inner space.
    pub baseline: f64,
    /// Line slot (column minus wrap) horizontal extent.
    pub x0: f64,
    pub x1: f64,
    pub ascent: f64,
    pub descent: f64,
    pub leading: f64,
    /// Story byte range (excluding the paragraph separator).
    pub range: Range<usize>,
    pub para: usize,
    pub glyphs: Vec<PlacedGlyph>,
    pub hyphenated: bool,
    pub first_in_para: bool,
    pub last_in_para: bool,
    /// x after the last glyph (caret at line end).
    pub end_x: f64,
    /// Word-space ratio actually used vs desired (H&J violation highlighting), 1.0 = desired.
    pub spacing: f64,
    /// H&J violation: 0 = within the paragraph's minimum/maximum word spacing, 1–3 = how far out
    /// (Preferences › Composition › Highlight H&J Violations shades).
    pub hj: u8,
    /// The paragraph's Keep Options couldn't be honoured here (Highlight Keep Violations).
    pub keep_violation: bool,
    /// The drop cap a paragraph's first line starts with (its first glyphs, set down to a later
    /// line's baseline).
    pub drop_cap: Option<DropCapBox>,
}

/// Where a drop cap sits (frame inner space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropCapBox {
    /// Story bytes from its line's start up to here are the drop cap's.
    pub end: usize,
    /// Its baseline.
    pub baseline: f64,
    /// The area it covers: from its line's top down to the text below its baseline.
    pub rect: Rect,
}

/// A paragraph rule, border or shading in frame inner space: `rect` filled, or `path` (non-zero
/// winding) when it has one, with `rect` its bounds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deco {
    pub rect: Rect,
    pub color: String,
    pub tint: f32,
    pub path: Option<designcraft_geom::BezPath>,
    /// Shown on screen only (not printed or exported).
    pub nonprinting: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FrameText {
    pub frame: ItemId,
    /// Vertical Type frame (upright glyphs turn back in it).
    pub vertical: bool,
    pub lines: Vec<Line>,
    pub decos: Vec<Deco>,
    /// Byte range of the story shown in this frame.
    pub range: Range<usize>,
    /// Column rectangles (inner space) for drawing column guides / hit testing.
    pub columns: Vec<Rect>,
    /// Height the text needs (for auto-sizing), from the text area top.
    pub content_height: f64,
    /// Table fragments placed in this frame (each also has a glyph-less line in `lines`).
    pub tables: Vec<TableFrag>,
    /// Footnotes placed at the bottom of this frame's columns.
    pub notes: Vec<PlacedNote>,
    /// Anchored objects placed in this frame's lines.
    pub objects: Vec<PlacedObject>,
}

/// An anchored object placed in the text.
#[derive(Clone, Debug)]
pub struct PlacedObject {
    /// Index in the story's [`Story::objects`].
    pub index: usize,
    /// Source item retained for anonymous footnote and cell stories.
    pub object: Arc<designcraft_doc::AnchoredObject>,
    /// Top-left of the object in frame inner space.
    pub origin: Point,
    pub size: (f64, f64),
    /// The line it sits on (or above).
    pub line: usize,
    x: f64,
    text_ascent: f64,
}

/// A footnote composed at the bottom of a column.
#[derive(Clone, Debug)]
pub struct PlacedNote {
    /// Footnote id (in the story's [`Story::notes`]) and its index there.
    pub id: u64,
    pub index: usize,
    pub column: u32,
    /// The number shown.
    pub label: String,
    /// Where the footnote's composed text (its frame 0) sits in the frame.
    pub origin: Point,
    /// Area of the footnote in frame inner space.
    pub rect: Rect,
    pub text: Arc<ComposedStory>,
    /// The footnote's story text (exporters map glyphs back to text).
    pub source: Arc<str>,
}

#[derive(Clone, Debug, Default)]
pub struct ComposedStory {
    pub story: StoryId,
    pub rev: u64,
    pub frames: Vec<FrameText>,
    /// First byte that didn't fit (overset text), if any.
    pub overset_at: Option<usize>,
    pub styles: Vec<RunStyle>,
    pub text_len: usize,
}

impl ComposedStory {
    pub fn is_overset(&self) -> bool {
        self.overset_at.is_some()
    }
    pub fn frame(&self, id: ItemId) -> Option<&FrameText> {
        self.frames.iter().find(|f| f.frame == id)
    }
    /// Number of lines in all frames.
    pub fn line_count(&self) -> usize {
        self.frames.iter().map(|f| f.lines.len()).sum()
    }
}

/// A text-wrap exclusion in a frame's inner space.
#[derive(Clone, Debug, PartialEq)]
pub struct Exclusion {
    /// Bounding box, offsets included.
    pub rect: Rect,
    pub mode: WrapMode,
    /// Where text may go beside the object. Spine sides are resolved to `LeftSide`/`RightSide`.
    pub side: WrapSide,
    /// Contour wrap, or a custom boundary: the outline the text follows (None wraps around `rect`).
    pub contour: Option<Contour>,
    /// An anchored object in the frame's own story: it wraps only lines that start after this
    /// story byte (its anchor).
    pub after: Option<usize>,
}

impl Exclusion {
    /// Does the exclusion wrap a line starting at story byte `line_start`? An own-story anchored
    /// object wraps only the lines after its anchor.
    pub fn wraps_line(&self, line_start: usize) -> bool {
        self.after.is_none_or(|a| a < line_start)
    }
}

/// An object outline for contour wrap: closed polylines in the frame's inner space and the
/// distance text keeps from them.
#[derive(Clone, Debug, PartialEq)]
pub struct Contour {
    pub rings: Vec<Vec<Point>>,
    /// The frame path of an outline taken from a placed graphic: the frame clips the graphic, so
    /// the outline counts only within it (empty: no clip).
    pub clip: Vec<Vec<Point>>,
    pub offset: f64,
}

/// Points a contour outline may have; a larger outline wraps around its bounding box.
const MAX_CONTOUR_POINTS: usize = 8192;

impl Contour {
    /// The outline of `path` (after `to_inner`), flattened; None if it is empty or too detailed.
    fn of(path: &designcraft_geom::BezPath, to_inner: designcraft_geom::Affine, offset: f64) -> Option<Contour> {
        let rings = flatten_rings(path, to_inner)?;
        offset.is_finite().then_some(Contour { rings, clip: Vec::new(), offset: offset.max(0.0) })
    }

    /// This outline counted only within `frame` (after `to_inner`); unclipped if the frame is
    /// empty or too detailed.
    fn within(self, frame: &designcraft_geom::BezPath, to_inner: designcraft_geom::Affine) -> Contour {
        match flatten_rings(frame, to_inner) {
            Some(clip) => Contour { clip, ..self },
            None => self,
        }
    }

    /// The outline mapped by `a`.
    fn transformed(self, a: designcraft_geom::Affine) -> Contour {
        let map = |rings: Vec<Vec<Point>>| rings.into_iter().map(|r| r.into_iter().map(|p| a * p).collect()).collect();
        Contour { rings: map(self.rings), clip: map(self.clip), offset: self.offset }
    }

    fn bounds(&self) -> Rect {
        let Some(r) = rings_bounds(&self.rings) else { return Rect::ZERO };
        let r = match rings_bounds(&self.clip) {
            Some(c) => r.intersect(c),
            None => r,
        };
        r.inflate(self.offset, self.offset)
    }

    /// Horizontal extent of the outline (offset included) within the band `y0..y1`: the band is
    /// widened by the offset, the outline's x-range there is limited to the clip's x-range there,
    /// then widened by the offset too.
    fn band_extent(&self, y0: f64, y1: f64) -> Option<(f64, f64)> {
        let (y0, y1) = (y0 - self.offset, y1 + self.offset);
        let (mut lo, mut hi) = rings_band_extent(&self.rings, y0, y1)?;
        if !self.clip.is_empty() {
            let (a, b) = rings_band_extent(&self.clip, y0, y1)?;
            (lo, hi) = (lo.max(a), hi.min(b));
        }
        (lo <= hi).then_some((lo - self.offset, hi + self.offset))
    }
}

/// The closed polylines of `path` (after `to_inner`), flattened; None if it is empty or too detailed.
fn flatten_rings(path: &designcraft_geom::BezPath, to_inner: designcraft_geom::Affine) -> Option<Vec<Vec<Point>>> {
    let mut rings: Vec<Vec<Point>> = Vec::new();
    let mut n = 0usize;
    let mut too_many = false;
    designcraft_geom::kurbo::flatten(to_inner * path.clone(), 0.25, |el| {
        n += 1;
        too_many |= n > MAX_CONTOUR_POINTS;
        match el {
            designcraft_geom::PathEl::MoveTo(p) => rings.push(vec![p]),
            designcraft_geom::PathEl::LineTo(p) => match rings.last_mut() {
                Some(r) => r.push(p),
                None => rings.push(vec![p]),
            },
            _ => {}
        }
    });
    rings.retain(|r| r.len() >= 2 && r.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
    (!too_many && !rings.is_empty()).then_some(rings)
}

fn rings_bounds(rings: &[Vec<Point>]) -> Option<Rect> {
    let mut pts = rings.iter().flatten();
    let &p0 = pts.next()?;
    Some(pts.fold(Rect::from_points(p0, p0), |r, &p| r.union_pt(p)))
}

/// Horizontal extent of closed polylines within the band `y0..y1`.
fn rings_band_extent(rings: &[Vec<Point>], y0: f64, y1: f64) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for ring in rings {
        let Some(&last) = ring.last() else { continue };
        let mut p = last;
        for &q in ring {
            let (a, b) = (p, q);
            p = q;
            if a.y.max(b.y) < y0 || a.y.min(b.y) > y1 {
                continue;
            }
            let dy = b.y - a.y;
            let (t0, t1) = if dy.abs() < 1e-12 {
                (0.0, 1.0)
            } else {
                let (ta, tb) = ((y0 - a.y) / dy, (y1 - a.y) / dy);
                (ta.min(tb).clamp(0.0, 1.0), ta.max(tb).clamp(0.0, 1.0))
            };
            for t in [t0, t1] {
                let x = a.x + t * (b.x - a.x);
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// The wrap exclusion of `other` (an item whose inner space maps to the spread by `xf`) for a
/// frame whose inner space is reached from the spread by `inv`. `left_page` places the object on
/// a left page (for the spine sides).
pub fn exclusion(other: &Item, xf: Affine, inv: Affine, left_page: bool) -> Exclusion {
    let group = other.shape == designcraft_doc::Shape::Group;
    let b =
        if group { (xf * other.xf.inverse()).transform_rect_bbox(other.bounds()) } else { other.path.transformed(xf).bounds().unwrap_or(Rect::ZERO) };
    wrap_exclusion(&other.wrap, b, || (!group).then(|| contour_path(other, other.wrap.contour)), xf, inv, left_page)
}

/// The wrap exclusion of the graphic placed in frame `it` (arguments as for [`exclusion`]): around
/// the part of the graphic the frame shows. None if there is no graphic or it is not shown.
pub fn graphic_exclusion(it: &Item, xf: Affine, inv: Affine, left_page: bool) -> Option<Exclusion> {
    let g = it.graphic()?;
    let shown = g.shown_bounds(it.inner_bounds())?;
    Some(wrap_exclusion(&g.wrap, xf.transform_rect_bbox(shown), || Some(contour_path(it, g.wrap.contour)), xf, inv, left_page))
}

/// The exclusion of `wrap` around an object with spread-space bounds `b`; `outline` gives its
/// contour in the inner space `xf` maps to the spread (None: around the bounds).
fn wrap_exclusion(
    wrap: &designcraft_doc::TextWrap,
    b: Rect,
    outline: impl FnOnce() -> Option<(designcraft_geom::BezPath, Option<designcraft_geom::BezPath>)>,
    xf: Affine,
    inv: Affine,
    left_page: bool,
) -> Exclusion {
    let o = wrap.offsets;
    let r = Rect::new(b.x0 - o[1], b.y0 - o[0], b.x1 + o[3], b.y1 + o[2]);
    let side = match wrap.side {
        WrapSide::TowardsSpine if left_page => WrapSide::RightSide,
        WrapSide::TowardsSpine => WrapSide::LeftSide,
        WrapSide::AwayFromSpine if left_page => WrapSide::LeftSide,
        WrapSide::AwayFromSpine => WrapSide::RightSide,
        s => s,
    };
    // A custom boundary already includes the offsets.
    if let Some(custom) = wrap.path.as_ref().and_then(|p| Contour::of(&p.to_bezpath(), inv * xf, 0.0)) {
        let rect = custom.bounds();
        let contour = matches!(wrap.mode, WrapMode::BoundingBox | WrapMode::Contour).then_some(custom);
        return Exclusion { rect, mode: wrap.mode, side, contour, after: None };
    }
    let contour = if wrap.mode == WrapMode::Contour { outline() } else { None }.and_then(|(path, frame)| {
        let c = Contour::of(&path, inv * xf, o[0])?;
        Some(match frame {
            Some(frame) => c.within(&frame, inv * xf),
            None => c,
        })
    });
    let rect = contour.as_ref().map_or_else(|| inv.transform_rect_bbox(r), Contour::bounds);
    Exclusion { rect, mode: wrap.mode, side, contour, after: None }
}

/// The outline a contour wrap of `kind` around frame `it` follows, in the frame's inner space, and
/// for an outline taken from the placed graphic, the frame path that clips it. Bounding Box
/// follows the part of the placed graphic the frame shows; Same As Clipping follows the graphic's
/// clipping path when it has one; detected edges, alpha channels and image paths need the image,
/// so they follow the frame's path (as other frames do).
fn contour_path(it: &Item, kind: designcraft_doc::ContourType) -> (designcraft_geom::BezPath, Option<designcraft_geom::BezPath>) {
    let frame = designcraft_geom::corners::apply(&it.path, &it.corners);
    if kind == designcraft_doc::ContourType::SameAsClipping
        && let Some((path, _)) = it.graphic().and_then(|g| g.clip_region())
    {
        return (path, Some(frame));
    }
    if kind == designcraft_doc::ContourType::BoundingBox
        && let Some(r) = it.graphic().and_then(|g| g.shown_bounds(it.inner_bounds()))
    {
        return (designcraft_geom::kurbo::Shape::to_path(&r, 0.1), Some(frame));
    }
    (frame, None)
}

/// One frame of the thread, ready for composition.
#[derive(Clone, Debug)]
pub struct FrameSpec {
    pub id: ItemId,
    /// Text area (inner space, after inset).
    pub area: Rect,
    pub opts: TextFrameOptions,
    /// Lines run top to bottom and follow each other right to left (the story is vertical); the
    /// area is the turned box.
    pub vertical: bool,
    pub exclusions: Vec<Exclusion>,
    /// The frame's outline when it isn't its rectangle (area space): each line's slot is the part
    /// of its band the outline covers, minus the inset.
    pub outline: Option<Arc<FrameOutline>>,
    pub page_name: Option<String>,
    /// Marker text of the section the frame's page is in (None on parent pages).
    pub section_marker: Option<String>,
    /// Absolute document page the frame is on (None on parent pages).
    pub page: Option<usize>,
    /// Baseline grid in inner space: (first grid line y, increment).
    pub grid: Option<(f64, f64)>,
    /// The frame is on a left page (for towards/away-from-spine alignment).
    pub left_page: bool,
    /// The frame is on a right page of a facing spread (spine-relative anchored objects mirror
    /// there; a single page has no spine).
    pub right_page: bool,
    /// The page and its margins in the frame's inner space (custom anchored objects).
    pub page_rect: Option<(Rect, Rect)>,
}

impl FrameSpec {
    pub fn columns(&self) -> Vec<Rect> {
        let a = self.area;
        // Primary frames inherit the document limit (216), beyond the interactive frame editor's 40.
        let n = self.opts.columns.clamp(1, 216);
        match self.opts.columns_kind {
            designcraft_doc::ColumnsKind::FixedWidth if self.opts.column_width > 0.0 => (0..n)
                .map(|i| {
                    let x = a.x0 + i as f64 * (self.opts.column_width + self.opts.gutter);
                    Rect::new(x, a.y0, x + self.opts.column_width, a.y1)
                })
                .collect(),
            _ => designcraft_doc::page::column_rects(a, n, self.opts.gutter),
        }
    }
}

/// The marker text of the section absolute page `abs` is in (empty when it has none).
pub fn section_marker(doc: &Document, abs: usize) -> String {
    doc.section_of(abs).map(|s| s.marker.clone()).unwrap_or_default()
}

/// Composition inputs that are not in the story itself.
#[derive(Clone, Debug, Default)]
pub struct ComposeOptions {
    /// Page name for page-number markers when the story is composed for a specific page (parent items).
    pub page_name: Option<String>,
    /// Section marker text when the story is composed for a specific page (parent items).
    pub section_marker: Option<String>,
    /// Absolute page for text variables when composed for a specific page (parent items).
    pub page: Option<usize>,
    /// Running-header index (needed when the story contains running-header variables).
    pub running: Option<std::sync::Arc<vars::RunningIndex>>,
    /// Generated text before the first paragraph (a footnote's number and separator).
    pub label: Option<String>,
    /// Anchor pages for cross-references (needed when the story contains cross-references).
    pub xrefs: Option<Arc<xref::XrefIndex>>,
}

/// Nesting depth past which wrap obstacles inside groups are not collected.
const MAX_WRAP_DEPTH: usize = 64;

/// The text wrap exclusions of a page item, the graphic it holds and the items in it, for a frame
/// whose inner space is reached from the spread by `inv`. `parent` maps the item's parent space to
/// the spread; `skip` is the text frame being wrapped; `left_page` tells whether a spread x is on
/// a left page.
#[allow(clippy::too_many_arguments)]
fn wrap_obstacles(it: &Item, parent: Affine, inv: Affine, skip: ItemId, left_page: &dyn Fn(f64) -> bool, depth: usize, out: &mut Vec<Exclusion>) {
    if it.id == skip || it.hidden || depth > MAX_WRAP_DEPTH {
        return;
    }
    let xf = parent * it.xf;
    let left = || left_page(parent.transform_rect_bbox(it.bounds()).center().x);
    if it.wrap.mode != WrapMode::None {
        out.push(exclusion(it, xf, inv, left()));
    }
    match &it.content {
        Content::Graphic(g) if g.wrap.mode != WrapMode::None => out.extend(graphic_exclusion(it, xf, inv, left())),
        Content::Group { .. } => {
            for c in it.shown_children() {
                wrap_obstacles(c, xf, inv, skip, left_page, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// Build the frame specs of a story from the document (geometry, wrap, page names, grid).
pub fn frame_specs(doc: &Document, sid: StoryId) -> Vec<FrameSpec> {
    let Some(st) = doc.story(sid) else { return vec![] };
    let mut out = Vec::with_capacity(st.frames.len());
    for &fid in &st.frames {
        let loc = doc.find(fid);
        let Some(item) = loc.as_ref().and_then(|l| doc.item_at(l)).or_else(|| doc.anchored_item(fid)) else { continue };
        let Some(tf) = item.text_frame() else { continue };
        let parent = loc.as_ref().map(|l| doc.parent_xf(l)).unwrap_or(designcraft_geom::Affine::IDENTITY);
        let xf = parent * item.xf;
        // Where the frame is on the spread (a group's transform included), to find its page.
        let center_x = parent.transform_rect_bbox(item.bounds()).center().x;
        let inv = xf.inverse();
        let mut exclusions = Vec::new();
        let spread_ref = loc.as_ref().map(|l| l.spread);
        let spread = spread_ref.and_then(|r| doc.spread(r));
        if !tf.options.ignore_wrap
            && let Some(sp) = spread
        {
            let left = |x: f64| sp.page_at_x(x).and_then(|pi| sp.pages.get(pi)).is_some_and(|p| p.side == designcraft_doc::PageSide::Left);
            for other in &sp.items {
                if doc.layer(other.layer).is_some_and(|l| !l.visible) {
                    continue;
                }
                wrap_obstacles(other, Affine::IDENTITY, inv, item.id, &left, 0, &mut exclusions);
            }
            // Only objects that reach the frame's text area wrap its text: one elsewhere on the
            // spread would otherwise cost a narrow column its room (the 1.5 em minimum segment of
            // `free_slots` applies once an object cuts the column).
            let a = item.text_area();
            exclusions.retain(|e| {
                let r = e.rect;
                r.is_finite() && r.x0 < a.x1 && r.x1 > a.x0 && r.y0 < a.y1 && r.y1 > a.y0
            });
        }
        let (page_name, page, side) = match (spread_ref, spread) {
            (Some(designcraft_doc::SpreadRef::Doc(si)), Some(sp)) => {
                let pi = sp.page_at_x(center_x).unwrap_or(0);
                let abs = doc.first_page_of_spread(si) + pi;
                (Some(doc.page_name(abs)), Some(abs), sp.pages.get(pi).map(|p| p.side))
            }
            (Some(designcraft_doc::SpreadRef::Parent(_)), Some(sp)) => {
                let prefix = sp.parent.as_ref().map(|p| p.prefix.clone()).unwrap_or_else(|| "A".into());
                // The side of the parent page under the frame, so a folio there aligns to the spine of its own page.
                let under = sp.page_at_x(center_x).and_then(|pi| sp.pages.get(pi));
                (Some(prefix), None, under.map(|p| p.side))
            }
            _ => (None, None, None),
        };
        let left_page = side == Some(designcraft_doc::PageSide::Left);
        let right_page = side == Some(designcraft_doc::PageSide::Right);
        let section_marker = page.map(|abs| section_marker(doc, abs));
        // The page under the frame and its margins, in inner space (bounding boxes).
        let page_rect = match (spread_ref, spread) {
            (_, Some(sp)) => sp
                .page_at_x(center_x)
                .and_then(|pi| sp.pages.get(pi))
                .map(|pg| (inv.transform_rect_bbox(pg.bounds()), inv.transform_rect_bbox(pg.margin_rect()))),
            _ => None,
        };
        let g = &doc.settings.baseline_grid;
        let (inc, start) = tf.options.baseline_grid.unwrap_or((g.increment, g.start));
        // Grid lines are at spread y = start + n·inc (page tops are y = 0); map into inner space (translation only).
        let ty = xf.translation().y;
        let grid = (inc > 0.0).then_some((start - ty, inc));
        // Type on a path: one line as long as the path (from the start offset).
        let (area, opts) = match &tf.options.path {
            Some(pt) => {
                let len = designcraft_geom::warp::PathWarp::new(&item.path.to_bezpath(), false).length();
                let size = st
                    .paras
                    .first()
                    .map(|p| doc.styles.resolve_para(p).1)
                    .map(|base| st.runs().map(|(_, f)| doc.styles.resolve_char(&base, f).size).fold(base.size, f64::max))
                    .unwrap_or(12.0);
                let opts = TextFrameOptions {
                    columns: 1,
                    inset: [0.0; 4],
                    first_baseline: designcraft_doc::FirstBaseline::Ascent,
                    // Keep the path identity: its synthetic area uses full line extents.
                    ..tf.options.clone()
                };
                let end = pt.end.map_or(len, |e| e.max(0.0).min(len));
                (Rect::new(0.0, 0.0, (end - pt.start).max(1.0), size * 1.6), opts)
            }
            None => (item.text_area(), tf.options.clone()),
        };
        // A non-rectangular frame keeps the top inset inward from its outline; its rectangular text
        // area (all four insets) still bounds the lines.
        let outline = match &tf.options.path {
            Some(_) => None,
            None => {
                let bp = if item.corners.is_none() { item.path.to_bezpath() } else { designcraft_geom::corners::apply(&item.path, &item.corners) };
                FrameOutline::new(&bp, tf.options.inset[0])
            }
        };
        // Vertical type: composed in the turned box; wraps and page rects turned with it.
        let vertical = doc.frame_vertical(item);
        let (area, exclusions, grid, page_rect, outline) = if vertical {
            let v = designcraft_doc::vertical_text_xf(area).inverse();
            let ex = exclusions
                .into_iter()
                .map(|e| Exclusion { rect: v.transform_rect_bbox(e.rect), contour: e.contour.map(|c| c.transformed(v)), ..e })
                .collect();
            let pr = page_rect.map(|(a, b)| (v.transform_rect_bbox(a), v.transform_rect_bbox(b)));
            let outline = outline.map(|o| o.transformed(v));
            (Rect::new(0.0, 0.0, area.height(), area.width()), ex, None, pr, outline)
        } else {
            (area, exclusions, grid, page_rect, outline)
        };
        out.push(FrameSpec {
            id: fid,
            area,
            opts,
            vertical,
            exclusions: if tf.options.path.is_some() { vec![] } else { exclusions },
            outline: outline.map(Arc::new),
            page_name,
            section_marker,
            page,
            grid,
            left_page,
            right_page,
            page_rect,
        });
    }
    out
}

/// Compose a story of `doc` through its frames, with the text wrap of page items and of anchored
/// objects ([`wrap`]).
pub fn compose_story(doc: &Document, sid: StoryId, opts: &ComposeOptions) -> ComposedStory {
    let others: Vec<ComposedStory> =
        wrap::neighbours(doc, sid).into_iter().map(|o| wrap::compose_among(doc, o, &ComposeOptions::default(), &[])).collect();
    wrap::compose_among(doc, sid, opts, &others.iter().collect::<Vec<_>>())
}

const DEFAULT_TAB: f64 = 36.0;

/// Compose `story` into `frames`.
pub fn compose(doc: &Document, story: &Story, frames: &[FrameSpec], opts: &ComposeOptions) -> ComposedStory {
    compose_with_db(doc, story, frames, opts, FontDb::global())
}

fn compose_with_db(doc: &Document, story: &Story, frames: &[FrameSpec], opts: &ComposeOptions, db: &FontDb) -> ComposedStory {
    let db = &db.scoped(doc.font_scope);
    let mut out = ComposedStory { story: story.id, rev: story.rev, text_len: story.text.len(), ..Default::default() };
    let mut styles_tab: Vec<RunStyle> = Vec::new();
    let mut missing_fonts: HashMap<String, bool> = HashMap::new();
    let cols: Vec<Vec<Rect>> = frames
        .iter()
        .map(|f| {
            let mut columns = f.columns();
            if story.direction == designcraft_doc::TextDirection::RightToLeft && !f.vertical {
                columns.reverse();
            }
            columns
        })
        .collect();
    out.frames = frames
        .iter()
        .zip(&cols)
        .map(|(f, columns)| FrameText { frame: f.id, vertical: f.vertical, columns: columns.clone(), ..Default::default() })
        .collect();
    let mut cur = Cursor {
        fi: 0,
        col: 0,
        last_baseline: None,
        last_descent: 0.0,
        last_reference: 0.0,
        aki_below: None,
        pending: 0.0,
        pi: 0,
        band: Band::default(),
        limits: Rc::default(),
        split: None,
        split_limits: Rc::default(),
        grid_next: None,
    };
    let para_ranges = story.para_ranges();
    let np = para_ranges.len();
    let deleted = deleted_paras(story, &para_ranges);
    let hyph_exceptions = doc.hyphenation_exception_map();
    let mut list_counter = ListCounter::default();
    // Named lists number independently of layout: one pass up front, for the labels.
    let named_labels: Vec<Option<String>> = {
        let mut counters: std::collections::HashMap<String, ListCounter> = std::collections::HashMap::new();
        story
            .paras
            .iter()
            .zip(&para_ranges)
            .zip(&deleted)
            .map(|((p, r), del)| {
                let (pp, _) = doc.styles.resolve_para(p);
                if pp.list_type != designcraft_doc::ListType::Numbers || pp.list_name.is_empty() || r.is_empty() || *del {
                    return None;
                }
                let c = counters.entry(pp.list_name.clone()).or_insert_with(|| doc.list_start(story.id, &pp.list_name));
                c.advance(&pp);
                Some(pp.number_label_in(c))
            })
            .collect()
    };
    // Border and shading of each paragraph set, drawn once the lines are in place.
    let mut boxes: Vec<Option<ParaBox>> = vec![None; np];
    // Keep options: paragraphs are re-laid from a snapshot when a keep is violated, either forced
    // into the next column or with a cap on the lines set before moving on (widow control).
    let mut snaps: Vec<Snapshot> = Vec::with_capacity(np);
    let mut info: Vec<ParaInfo> = Vec::with_capacity(np);
    let mut force_col = vec![false; np];
    let mut line_cap: Vec<Option<usize>> = vec![None; np];
    let mut restores = 0usize;
    let mut pi = 0;
    let var_story = vars::has_vars(story);
    let mut notes = Notes::new(doc, story);
    let mut note_snaps: Vec<(u32, Option<usize>, usize)> = Vec::with_capacity(np);
    let mut var_cache: std::collections::HashMap<Option<usize>, std::sync::Arc<Vec<String>>> = Default::default();
    // Column balancing above spanning paragraphs: settled and trial bottoms by band, the search
    // in progress, and a budget for the re-lays.
    let mut limits: Rc<Vec<(BandKey, f64)>> = Rc::default();
    let mut trial: Option<Trial> = None;
    let mut balance_runs = 0usize;
    let balance_frames = frames.iter().zip(&cols).filter(|(f, c)| f.opts.balance_columns && c.len() > 1).count();
    let balance_budget = 20
        * (story.paras.iter().filter(|p| matches!(doc.styles.resolve_para(p).0.span_columns, SpanColumns::Span(_))).count()
            + usize::from(balance_frames > 0));
    // Balancing split-column blocks that text follows.
    let mut split_limits: Rc<Vec<(SplitKey, f64)>> = Rc::default();
    let mut split_trial: Option<Trial<SplitKey>> = None;
    let mut split_runs = 0usize;
    let split_budget = 20 * story.paras.iter().filter(|p| matches!(doc.styles.resolve_para(p).0.span_columns, SpanColumns::Split(_))).count();
    // Paragraphs set across columns (vertical justification moves their frames' lines together),
    // and in split columns.
    let mut span_paras = vec![false; np];
    let mut split_paras = vec![false; np];
    'paras: while pi <= np {
        cur.pi = pi;
        cur.limits = Rc::clone(&limits);
        cur.split_limits = Rc::clone(&split_limits);
        let snap = Snapshot::take(&out, &cur, &list_counter);
        snaps.truncate(pi);
        snaps.push(snap);
        note_snaps.truncate(pi);
        note_snaps.push((notes.num, notes.key, notes.placed.len()));
        info.truncate(pi);
        // A balancing trial whose band ran past its frame: re-lay the band with more room.
        if trial.as_ref().is_some_and(|t| cur.fi > t.key.0)
            && let Some(j) = trial_failed(&mut trial, &mut limits, balance_runs < balance_budget)
        {
            balance_runs += 1;
            rewind(j, &snaps, &note_snaps, &mut notes, &mut out, &mut cur, &mut list_counter, &mut force_col, &mut line_cap);
            pi = j;
            continue 'paras;
        }
        if pi >= np {
            // The story ends here: Balance Columns evens out the columns of its last frame.
            if balance_frames > 0
                && let Some(j) = balance_end(&mut trial, &mut limits, &cur, &cols, frames, &out, np, balance_runs < balance_budget)
            {
                balance_runs += 1;
                rewind(j, &snaps, &note_snaps, &mut notes, &mut out, &mut cur, &mut list_counter, &mut force_col, &mut line_cap);
                pi = j;
                continue 'paras;
            }
            break 'paras;
        }
        if deleted.get(pi).copied().unwrap_or(false) {
            // A deleted paragraph takes no room; for keeps it stands in for the one before it.
            let filler = info.last().map_or(
                ParaInfo {
                    start: (cur.fi, cur.col),
                    at_top: cur.last_baseline.is_none(),
                    end: (cur.fi, cur.col),
                    lines: 0,
                    lines_in_end_col: 0,
                    keep_with_next: 0,
                    keep_together: false,
                    keep_all: false,
                    keep_first: 1,
                    keep_last: 1,
                },
                |p| ParaInfo { lines_in_end_col: 0, ..p.clone() },
            );
            info.push(filler);
            pi += 1;
            continue;
        }
        let prange = para_ranges[pi].clone();
        let pf = &story.paras[pi];
        let (mut pp, base_chars) = doc.styles.resolve_para(pf);
        // A break character that ends the previous paragraph sends this one on, like a start option.
        // Deleted paragraphs in between don't count, nor does a deleted or hidden break.
        let after_break = (0..pi).rev().find(|p| !deleted.get(*p).copied().unwrap_or(false)).and_then(|p| {
            let r = para_ranges.get(p)?;
            let (i, c) = story.text.get(r.clone())?.char_indices().next_back()?;
            let start = break_start(c)?;
            let (_, chars) = doc.styles.resolve_para(story.paras.get(p)?);
            let props = doc.styles.resolve_char(&chars, story.char_format_at(r.start + i + c.len_utf8()));
            let gone = props.change == designcraft_doc::ChangeMark::Deleted || doc.conditions_hide(&props.conditions);
            (!gone).then_some(start)
        });
        if let Some(t) = story.para_table(pi) {
            let start = after_break.unwrap_or(pp.start_paragraph);
            if cur.last_baseline.is_some() {
                cur.start(start, &cols, frames, doc);
            } else {
                cur.start_at_top(start, frames, doc);
            }
            let at_top = cur.last_baseline.is_none();
            let start_at = (cur.fi, cur.col);
            let anchor = prange.start + story.text[prange.clone()].find(story::TABLE_ANCHOR).unwrap_or(0);
            if !table::place_table(doc, t, anchor, pi, &pp, frames, &cols, &mut cur, &mut out, opts) {
                if let Some(j) = trial_failed(&mut trial, &mut limits, balance_runs < balance_budget) {
                    balance_runs += 1;
                    rewind(j, &snaps, &note_snaps, &mut notes, &mut out, &mut cur, &mut list_counter, &mut force_col, &mut line_cap);
                    pi = j;
                    continue 'paras;
                }
                out.overset_at = Some(prange.start);
                break 'paras;
            }
            info.push(ParaInfo {
                start: start_at,
                at_top,
                end: (cur.fi, cur.col),
                lines: 1,
                lines_in_end_col: 1,
                keep_with_next: 0,
                keep_together: false,
                keep_all: false,
                keep_first: 1,
                keep_last: 1,
            });
            list_counter = ListCounter::default();
            boxes[pi] = None;
            cur.pending += pp.space_after;
            pi += 1;
            continue;
        }
        // Bullets & numbering: generated prefix (shaped as its own glyphs, mapped to the paragraph start).
        let cur_frame = frames.get(cur.fi.min(frames.len().saturating_sub(1)));
        let var_values = if var_story {
            let page = opts.page.or_else(|| cur_frame.and_then(|f| f.page));
            var_cache.entry(page).or_insert_with(|| std::sync::Arc::new(vars::values(doc, page, opts.running.as_deref()))).clone()
        } else {
            Default::default()
        };
        let mut sub = SubstCtx {
            page_name: opts.page_name.clone().or_else(|| cur_frame.and_then(|f| f.page_name.clone())),
            section_marker: opts.section_marker.clone().or_else(|| cur_frame.and_then(|f| f.section_marker.clone())),
            vars: var_values,
            hidden_conditions: doc.conditions.iter().filter(|c| !c.visible).map(|c| c.name.clone()).collect(),
            vertical: cur_frame.is_some_and(|f| f.vertical),
            auto_tcy: pp.auto_tcy.min(shape::AUTO_TCY_MAX),
            auto_tcy_roman: pp.auto_tcy_include_roman,
            rotate_roman: pp.rotate_roman,
            ..Default::default()
        };
        if !story.endnotes.is_empty() {
            // Endnote reference numbers continue through the document.
            let first = doc.endnote_start(story.id) + story.text[..prange.start].matches(designcraft_doc::ENDNOTE_REF).count() as u32;
            for (k, (i, _)) in story.text[prange.clone()].match_indices(designcraft_doc::ENDNOTE_REF).enumerate() {
                sub.notes.insert(prange.start + i, doc.endnote_options.label(first + k as u32));
            }
        }
        if !story.objects.is_empty() {
            let before = story.text[..prange.start].matches(designcraft_doc::OBJECT_MARK).count();
            for (k, (i, _)) in story.text[prange.clone()].match_indices(designcraft_doc::OBJECT_MARK).enumerate() {
                let Some(o) = story.objects.get(before + k) else { continue };
                let (w, h) = o.size();
                let (y_offset, space, custom) = match &o.position {
                    designcraft_doc::AnchorPosition::Inline { y_offset } => (Some(*y_offset), 0.0, false),
                    designcraft_doc::AnchorPosition::AboveLine { space_before, space_after, .. } => (None, space_before + space_after, false),
                    designcraft_doc::AnchorPosition::Custom { .. } => (Some(0.0), 0.0, true),
                };
                sub.objects.insert(prange.start + i, shape::ObjectSpec { index: before + k, w, h, y_offset, space, custom });
            }
        }
        if !story.xrefs.is_empty() {
            sub.xrefs = xref::texts_in(doc, story, prange.clone(), opts.xrefs.as_deref());
        }
        if notes.active() {
            notes.number_refs(doc, prange.clone(), cur_frame.and_then(|f| f.page), &mut sub);
        }
        let sub_objects = sub.objects.clone();
        let drop_end = drop_cap_end(&story.text, prange.clone(), &pp, sub.vertical);
        let mut table = StyleTable { styles: &mut styles_tab, missing: &mut missing_fonts };
        let env = shape::TypeEnv { auto_leading: pp.auto_leading, adv: doc.settings.advanced_type, glyph_fallback: doc.settings.glyph_fallback };
        let mut sp = shape::shape_para(
            db,
            &doc.styles,
            story,
            pi,
            prange.clone(),
            &base_chars,
            env,
            &sub,
            &mut table,
            &pp.nested_styles,
            &pp.grep_styles,
            &[],
            (drop_end, &pp.drop_cap_style),
        );
        // An empty paragraph has no glyphs of its own, so its only addressable character is its
        // hidden paragraph mark (the `\n` just past `prange.end`). A leading/size override applied
        // there (e.g. selecting the pilcrow to adjust line height) still needs to set the line's
        // height, so resolve the mark's own run instead of the paragraph's plain default when present.
        let mark_chars = prange
            .is_empty()
            .then(|| story.runs().find(|(r, _)| r.contains(&prange.end)))
            .flatten()
            .map(|(_, fmt)| doc.styles.resolve_char(&base_chars, fmt));
        let leading_chars = mark_chars.as_ref().unwrap_or(&base_chars);
        // Drop cap: the size and indent come from the text beside it (`base_*` when there's none).
        let base_leading = match leading_chars.leading {
            designcraft_doc::Leading::Auto => leading_chars.size * pp.auto_leading,
            designcraft_doc::Leading::Points(v) => v,
        };
        let base_cap = {
            let face = db.face(&base_chars.font_family, &base_chars.font_style);
            face.cap_height * base_chars.size / face.upem * base_chars.v_scale
        };
        let drop_grid = cur_frame.and_then(|f| f.grid).filter(|_| pp.grid_align == GridAlign::AllLines).map(|g| g.1);
        // Align Left Edge (left-to-right paragraphs).
        let drop_align = pp.drop_cap_align_left && pp.direction == designcraft_doc::TextDirection::LeftToRight;
        // Tab stops for the drop cap, with the start-side indent (the right one, right to left).
        let drop_tabs = (&pp.tabs[..], if pp.direction == designcraft_doc::TextDirection::RightToLeft { pp.right_indent } else { pp.left_indent });
        if !pp.nested_line_styles.is_empty() && cur.fi < frames.len() {
            // Nested line styles: find where the first lines end in this column, restyle them, and
            // look again (the style changes the widths) until the lines settle.
            let cols_here = &cols[cur.fi];
            let col = span_rect(cols_here, cols_here[cur.col.min(cols_here.len() - 1)], pp.span_columns);
            let col = SplitCfg::of(&pp).map_or(col, |c| c.sub_rect(col, 0));
            let spacing = spacing_for(&pp, base_chars.size);
            let mut lines: Vec<(std::ops::Range<usize>, String)> = Vec::new();
            for _ in 0..3 {
                // The same drop cap, spacing, hyphenation and breaker as the layout below.
                let mut gl = sp.glyphs.clone();
                let dc = split_drop_cap(db, &mut gl, drop_end, pp.drop_cap_lines, base_leading, base_cap, drop_grid, drop_align, drop_tabs);
                let width = |j: usize| {
                    let ind = match &dc {
                        Some(dc) if j < dc.lines => dc.indent(),
                        _ if j == 0 => pp.first_line_indent,
                        _ => 0.0,
                    };
                    (col.width() - pp.left_indent - pp.right_indent - ind).max(1.0)
                };
                apply_desired_spacing(&mut gl, &pp);
                mojikumi::apply(&mut gl, &doc.styles, &pp.mojikumi);
                let hy = hyphenation_points(&story.text, &gl, &pp, &hyph_exceptions, &foreign_ranges(doc, story, prange.clone(), &base_chars));
                let breaks = if pp.composer.single_line() || gl.iter().any(|g| g.ch == '\t') || gl.len() > 4000 {
                    let tab = |j: usize, x: f64, i: usize| {
                        let ind = pp.left_indent + if j == 0 { pp.first_line_indent } else { 0.0 };
                        tab_advance(&pp.tabs, pp.left_indent, ind + x, gl.get(i + 1..).unwrap_or_default()).0
                    };
                    breaker::greedy(&gl, &hy, &spacing, &width, &tab)
                } else if pp.balance_ragged && !spacing.justify {
                    breaker::balanced(&gl, &hy, &spacing, &width)
                } else {
                    breaker::knuth_plass(&gl, &hy, &spacing, &width)
                };
                // Byte where line `k` starts (the paragraph end past the last line).
                let byte_at =
                    |k: usize| if k == 0 { prange.start } else { breaks.get(k - 1).and_then(|b| gl.get(b.next)).map_or(prange.end, |g| g.byte) };
                let mut want = Vec::new();
                let mut line = 0usize;
                for nl in &pp.nested_line_styles {
                    if line >= breaks.len() || nl.lines == 0 {
                        break;
                    }
                    let end = line + nl.lines as usize;
                    if !nl.style.is_empty() && nl.style != designcraft_doc::NO_CHAR_STYLE {
                        want.push((byte_at(line)..byte_at(end).max(byte_at(line)), nl.style.clone()));
                    }
                    line = end;
                }
                if want == lines {
                    break;
                }
                lines = want;
                sp = shape::shape_para(
                    db,
                    &doc.styles,
                    story,
                    pi,
                    prange.clone(),
                    &base_chars,
                    env,
                    &sub,
                    &mut table,
                    &pp.nested_styles,
                    &pp.grep_styles,
                    &lines,
                    (drop_end, &pp.drop_cap_style),
                );
            }
        }
        // Rules in Text Color: the colour of the paragraph's first character (rule above) and last
        // character (rule below).
        let text_color = |g: Option<&Glyph>| {
            g.and_then(|g| table.styles.get(g.style as usize))
                .map_or((base_chars.fill.clone(), base_chars.fill_tint), |s| (s.fill.clone(), s.fill_tint))
        };
        let (above, below) = (text_color(sp.glyphs.iter().find(|g| g.len > 0)), text_color(sp.glyphs.iter().rev().find(|g| g.len > 0)));
        let rule_color = |r: &designcraft_doc::Rule, text: &(String, f32)| {
            if r.color == designcraft_doc::TEXT_COLOR { text.clone() } else { (r.color.clone(), r.tint) }
        };
        let drop_cap = split_drop_cap(db, &mut sp.glyphs, drop_end, pp.drop_cap_lines, base_leading, base_cap, drop_grid, drop_align, drop_tabs);
        // List labels take the default super/subscript settings.
        let label_env = shape::TypeEnv { adv: Default::default(), ..env };
        let list_label = match pp.list_type {
            // An empty paragraph has no bullet or number, and the numbering carries on past it.
            designcraft_doc::ListType::Numbers | designcraft_doc::ListType::Bullets if prange.is_empty() => None,
            // A named list: carries on past other paragraphs (and from earlier stories).
            designcraft_doc::ListType::Numbers if !pp.list_name.is_empty() => {
                Some(named_labels.get(pi).cloned().flatten().unwrap_or_else(|| pp.number_label(1)))
            }
            designcraft_doc::ListType::Numbers => {
                list_counter.advance(&pp);
                Some(pp.number_label_in(&list_counter))
            }
            designcraft_doc::ListType::Bullets => Some(pp.bullet_label()),
            designcraft_doc::ListType::None => {
                list_counter = ListCounter::default();
                None
            }
        };
        // The label's glyphs at the paragraph start, and how many of them (up to its first tab)
        // the list alignment places.
        let mut label_aligned = 0usize;
        let mut label_tab = false;
        if let Some(label) = &list_label {
            let mut chars = label_chars(doc, &pp, &base_chars);
            // The number is drawn in the Digits of the paragraph's first run, so Persian text
            // gets Persian numbers whether Digits is set on the paragraph style or on the text
            // (unless the number's character style sets its own).
            if chars.digits == base_chars.digits
                && let Some((_, fmt)) = story.runs().find(|(rr, _)| rr.end > prange.start && rr.start < prange.end)
            {
                chars.digits = doc.styles.resolve_char(&base_chars, fmt).digits;
            }
            let n = prepend_label(db, &mut sp.glyphs, label, prange.start, &chars, label_env, &mut table);
            for g in sp.glyphs.iter_mut().take(n) {
                g.list_label = true;
            }
            label_aligned = sp.glyphs.get(..n).unwrap_or_default().iter().position(|g| g.ch == '\t').unwrap_or(n);
            label_tab = label_aligned < n;
            // Tab Position: a stop for the text after the label.
            if let Some(t) = pp.list_tab.filter(|t| t.is_finite() && *t >= 0.0)
                && !pp.tabs.iter().any(|s| (s.position - t).abs() < 1e-6)
            {
                pp.tabs.push(designcraft_doc::TabStop { position: t, align: TabAlign::Left, leader: String::new(), align_on: String::new() });
                pp.tabs.sort_by(|a, b| a.position.total_cmp(&b.position));
            }
        }
        if pi == 0
            && let Some(label) = &opts.label
        {
            prepend_label(db, &mut sp.glyphs, label, prange.start, &base_chars, label_env, &mut table);
            label_aligned = 0;
        }
        let list_align = match pp.list_type {
            _ if label_aligned == 0 || pp.direction == designcraft_doc::TextDirection::RightToLeft => designcraft_doc::ListAlign::Left,
            designcraft_doc::ListType::Bullets => pp.bullet_align,
            _ => pp.number_align,
        };
        boxes[pi] = ParaBox::of(doc, &pp);
        let mut glyphs = sp.glyphs;
        if !pp.composer.japanese() {
            // Every IDML's root style carries a leading model and character alignment, which only
            // the Japanese composers use.
            for g in &mut glyphs {
                g.leading_model = designcraft_doc::cjk::LeadingModel::Roman;
                g.character_alignment = designcraft_doc::cjk::CharacterAlignment::Baseline;
            }
        }
        let bidi_text = bidi::paragraph_text(&mut glyphs, &story.text);
        let bidi_info = unicode_bidi::BidiInfo::new(
            &bidi_text,
            Some(if pp.direction == designcraft_doc::TextDirection::RightToLeft { unicode_bidi::Level::rtl() } else { unicode_bidi::Level::ltr() }),
        );
        bidi::resolve_mirroring(&mut glyphs, &bidi_info);
        apply_desired_spacing(&mut glyphs, &pp);
        mojikumi::apply(&mut glyphs, &doc.styles, &pp.mojikumi);
        let hyph_after = hyphenation_points(&story.text, &glyphs, &pp, &hyph_exceptions, &foreign_ranges(doc, story, prange.clone(), &base_chars));
        let base_size = base_chars.size;
        let spacing = spacing_for(&pp, base_size);
        // An empty paragraph takes its height from its paragraph mark's character format.
        let mark_chars = if glyphs.is_empty() {
            std::borrow::Cow::Owned(doc.styles.resolve_char(&base_chars, story.char_format_of(prange.start)))
        } else {
            std::borrow::Cow::Borrowed(&base_chars)
        };
        let mark_leading = match mark_chars.leading {
            designcraft_doc::Leading::Auto => mark_chars.size * pp.auto_leading,
            designcraft_doc::Leading::Points(v) => v,
        };
        // The paragraph mark is a character of the last line: its leading counts there like any
        // other character's. It doesn't set the line's ascent or descent. A hidden or deleted
        // mark takes no room.
        let last_line_mark = story.text.get(prange.end..).filter(|t| t.starts_with('\n')).and_then(|_| {
            let props = doc.styles.resolve_char(&base_chars, story.format_after(prange.end));
            let hidden = props.change == designcraft_doc::ChangeMark::Deleted
                || (!props.conditions.is_empty() && props.conditions.iter().all(|c| sub.hidden_conditions.contains(c)));
            (!hidden).then_some(match props.leading {
                designcraft_doc::Leading::Auto => props.size * pp.auto_leading,
                designcraft_doc::Leading::Points(v) => v,
            })
        });
        // A paragraph set across the columns moves on to the next frame where others move on to
        // the next column.
        let spanning = matches!(pp.span_columns, SpanColumns::Span(n) if n != 1);
        span_paras[pi] = spanning;
        // Span space applies where the paragraph spans more than one column; in a one-column frame
        // a span changes nothing.
        let spans_in = |fi: usize| spanning && cols.get(fi).is_some_and(|c| c.len() > 1);
        // Consecutive paragraphs with the same Split Columns settings share a split block; within
        // it, a paragraph moves on to the next sub-column.
        let split_cfg = SplitCfg::of(&pp);
        split_paras[pi] = split_cfg.is_some();
        if cur.split.is_some_and(|s| Some(s.cfg) != split_cfg) {
            cur.split = None;
        }
        let next_column = |cur: &mut Cursor| {
            if spanning {
                cur.next_frame()
            } else if cur.can_next_sub() {
                cur.next_sub()
            } else {
                cur.next_column(&cols)
            }
        };
        // A paragraph start or a break character; Next Column while a paragraph spans columns is
        // the next frame, as above.
        let go = |cur: &mut Cursor, start: StartParagraph| match start {
            StartParagraph::NextColumn => {
                let (fi, col) = (cur.fi, cur.col);
                next_column(cur);
                if cur.fi == fi && cur.col != col {
                    cur.band.tail = Some((cur.pi, cur.col));
                }
            }
            other => cur.start(other, &cols, frames, doc),
        };
        // Paragraph start options. At the top of a frame there is nothing to break away from, but
        // an odd or even page start still has to land on a page of that parity.
        if let Some(start) = after_break {
            if cur.last_baseline.is_some() {
                go(&mut cur, start);
            } else {
                cur.start_at_top(start, frames, doc);
            }
        }
        if force_col[pi] {
            next_column(&mut cur);
        }
        if cur.last_baseline.is_some() {
            go(&mut cur, pp.start_paragraph);
        } else {
            cur.start_at_top(pp.start_paragraph, frames, doc);
        }
        // Text in the columns above a spanning paragraph, or a split block starting below text.
        let mut text_above = cur.split.is_none() && split_cfg.is_some();
        if spanning && cur.fi < frames.len() {
            // Close the band: the paragraph goes below the deepest of its columns, which are
            // balanced first.
            let key = cur.band_key();
            let deepest = out.frames[cur.fi]
                .lines
                .get(cur.band.line0..)
                .unwrap_or_default()
                .iter()
                .map(|l| (l.baseline, l.descent))
                .max_by(|a, b| (a.0 + a.1).total_cmp(&(b.0 + b.1)));
            if let Some((baseline, descent)) = deepest {
                let bottom = baseline + descent;
                let multi = cols[cur.fi].len() > 1;
                let active = trial.as_ref().is_some_and(|t| t.key == key && t.span == pi);
                if !active && let Some(t) = trial.take() {
                    set_limit(&mut limits, t.key, None);
                }
                if multi && (active || limit_of(&limits, key).is_none() && balance_runs < balance_budget) {
                    let top = cur.clip(cols[cur.fi][0]).y0;
                    let mut t = trial.take().unwrap_or(Trial { key, span: pi, lo: top, hi: bottom, tries: 0, done: false });
                    // This layout fits: its bottom is the best so far.
                    t.hi = t.hi.min(bottom);
                    let next = if t.done { t.hi } else { t.next() };
                    set_limit(&mut limits, key, Some(next));
                    if !t.done {
                        trial = Some(t);
                        balance_runs += 1;
                        rewind(key.1, &snaps, &note_snaps, &mut notes, &mut out, &mut cur, &mut list_counter, &mut force_col, &mut line_cap);
                        pi = key.1;
                        continue 'paras;
                    }
                }
                cur.col = 0;
                cur.last_baseline = Some(baseline);
                cur.last_descent = descent;
                text_above = multi;
            } else if cur.col != 0 {
                cur.col = 0;
                cur.resume();
            }
        }
        if cur.last_baseline.is_some() {
            // Same style as the previous paragraph with a same-style spacing: it replaces space after + before.
            let same_style = pi.checked_sub(1).and_then(|p| story.paras.get(p)).is_some_and(|prev| prev.style == pf.style && prev.table.is_none());
            match pp.same_para_style_spacing {
                Some(gap) if same_style => cur.pending = gap,
                _ => cur.pending += if text_above { pp.space_before.max(pp.span_space_before) } else { pp.space_before },
            }
        }
        if let Some(cfg) = split_cfg
            && cur.split.is_none()
        {
            let start = cur.state();
            cur.split = Some(SplitBlock { cfg, sub: 0, sub_lines: 0, part_lines: 0, line0: None, para: pi, start, top: None });
            cur.split_part();
        }
        let at_top = cur.last_baseline.is_none();
        let start_at = (cur.fi, cur.col);
        // Rule above / shading track the paragraph's first line.
        let mut g0 = 0usize;
        let mut line_no = 0usize;
        // Paragraph line number of the first line in the current column.
        let mut col_first_line = 0usize;
        let has_tabs = glyphs.iter().any(|g| g.ch == '\t');
        let mut first_line_rect: Option<(usize, f64, f64, f64)> = None; // frame, baseline, ascent, x-span
        let rtl = pp.direction == designcraft_doc::TextDirection::RightToLeft;
        // Paragraph gyoudori: the (frame, column) of the block and the grid line after it.
        let mut para_block: Option<((usize, usize), f64)> = None;
        // Segments of a line already placed (beside an object with text on both sides) that the
        // next lines of the paragraph fill before a new line starts, and that line's baseline.
        let mut carry: Vec<(f64, f64)> = Vec::new();
        let mut carry_baseline = 0.0;
        // The grid line after the carried line's band (see `grid_end`).
        let mut carry_grid: Option<f64> = None;
        // Re-breaks after a line's segment turned out other than the breaker assumed.
        let mut rebreaks = 0usize;
        loop {
            if cur.fi >= frames.len() {
                if let Some(j) = trial_failed(&mut trial, &mut limits, balance_runs < balance_budget) {
                    balance_runs += 1;
                    rewind(j, &snaps, &note_snaps, &mut notes, &mut out, &mut cur, &mut list_counter, &mut force_col, &mut line_cap);
                    pi = j;
                    continue 'paras;
                }
                // An empty last paragraph (the story ends with a return) isn't overset text, as in
                // InDesign: nothing of it is left to show.
                if prange.is_empty() && prange.start >= story.text.len() {
                    break 'paras;
                }
                out.overset_at = Some(glyphs.get(g0).map(|g| g.byte).unwrap_or(prange.start));
                break 'paras;
            }
            let f = &frames[cur.fi];
            let base = cols[cur.fi][cur.col.min(cols[cur.fi].len() - 1)];
            let col = cur.clip(base);
            // A spanning paragraph isn't held to the balanced bottom of the columns above it.
            let col = if spanning { span_rect(&cols[cur.fi], Rect::new(col.x0, col.y0, col.x1, base.y1), pp.span_columns) } else { col };
            let col = cur.split_rect(col);
            // Estimate slots for the breaker with the paragraph's base leading.
            // A shaped frame's slots change from line to line, so they are estimated with the
            // band the lines are placed with (or a taller one, so no line outgrows its slot).
            let (est_asc, est_band) = if f.outline.is_some() {
                let (a, _, d, _) = line_metrics(&[], &glyphs, g0, mark_leading, mark_chars.size, db, &mark_chars);
                (a, (a.max(base_chars.size * 0.8), d.max(base_chars.size * 0.25)))
            } else {
                (base_chars.size * 0.75, (base_chars.size * 0.8, 0.0))
            };
            let est_tops = Tops { typo_ascent: est_asc, cap: est_asc * 0.72, xh: est_asc * 0.5, em: base_chars.size * 0.88 };
            let est_first =
                if carry.is_empty() { cur.next_baseline(f, col, base_leading, est_asc, est_tops, &pp) } else { carry_baseline + base_leading };
            let est_jump = cur.jump(f, base_leading, est_asc, est_tops);
            let slots = estimate_slots(f, col, est_first, est_jump, base_leading, base_chars.size, est_band, &glyphs[g0..], rtl, &carry);
            let width = |j: usize| -> f64 {
                let (x0, x1) = slots.get(j).copied().unwrap_or((col.x0, col.x1));
                // Beside the drop cap (in the column the paragraph starts in), its width; else the
                // first-line indent.
                let ind = pp.left_indent
                    + pp.right_indent
                    + match &drop_cap {
                        Some(dc) if line_no + j < dc.lines && col_first_line == 0 => dc.indent(),
                        _ if line_no + j == 0 => pp.first_line_indent,
                        _ => 0.0,
                    };
                (x1 - x0 - ind).max(1.0)
            };
            let rest = &glyphs[g0..];
            let rest_h = &hyph_after[g0..];
            let breaks: Vec<Break> = if pp.composer.single_line() || has_tabs || rest.len() > 4000 {
                // Where each line starts relative to the tab origin, as `layout_line` places it.
                let tab = |j: usize, x: f64, i: usize| {
                    let x0 = slots.get(j).map_or(col.x0, |s| s.0);
                    let ind = pp.left_indent
                        + match &drop_cap {
                            // Beside a drop cap: at its start-side indent (the right side in RTL).
                            Some(dc) if line_no + j < dc.lines && col_first_line == 0 => {
                                if rtl {
                                    0.0
                                } else {
                                    dc.indent()
                                }
                            }
                            _ if line_no + j == 0 => pp.first_line_indent,
                            _ => 0.0,
                        };
                    tab_advance(&pp.tabs, pp.left_indent, x0 + ind + x - col.x0, rest.get(i + 1..).unwrap_or_default()).0
                };
                breaker::greedy(rest, rest_h, &spacing, &width, &tab)
            } else if pp.balance_ragged && !spacing.justify {
                breaker::balanced(rest, rest_h, &spacing, &width)
            } else {
                breaker::knuth_plass(rest, rest_h, &spacing, &width)
            };
            let mut moved = false;
            for (k, b) in breaks.iter().enumerate() {
                let (s, e) = (g0 + b.start, g0 + b.end);
                let line_glyphs = &glyphs[s..e.max(s)];
                let (asc, tops, desc, lead) = line_metrics(line_glyphs, &glyphs, s, mark_leading, mark_chars.size, db, &mark_chars);
                let lead = match last_line_mark {
                    Some(m) if k + 1 == breaks.len() => lead.max(m),
                    _ => lead,
                };
                let (reference, model) = cjk_line_reference(line_glyphs);
                let advance = if cur.last_baseline.is_some() { cur.aki_below.unwrap_or(lead) } else { lead };
                let (mut x0, mut x1) = (col.x0, col.x1);
                // A wrap segment that can't hold the line's first word whole stays empty and the
                // text goes on in the next segment or line (a word wider than the column is split
                // wherever it lands).
                let ind = pp.left_indent + pp.right_indent + if line_no == 0 { pp.first_line_indent } else { 0.0 };
                let need = if f.exclusions.is_empty() && f.outline.is_none() {
                    0.0
                } else {
                    let w = breaker::first_word_width(glyphs.get(s..).unwrap_or_default(), hyph_after.get(s..).unwrap_or_default(), &spacing);
                    if w > col.width() - ind { 0.0 } else { w }
                };
                let holds = |&(a, b): &(f64, f64)| b - a - ind + 0.01 >= need;
                // The line takes the next segment of a line already placed, the one the breaker
                // set it for.
                let mut continues = false;
                let mut baseline = carry_baseline;
                let mut grid_end = None;
                if let Some(i) = carry.iter().position(holds) {
                    continues = i == 0;
                    grid_end = carry_grid;
                    (x0, x1) = carry.get(i).copied().unwrap_or((x0, x1));
                    carry.drain(..=i);
                } else {
                    carry.clear();
                    baseline = cur.next_baseline(f, col, advance, asc, tops, &pp);
                    if cur.last_baseline.is_some() {
                        baseline += cur.last_reference - reference;
                    }
                    // Baseline grid: grid alignment and gyoudori (see `grid`).
                    let grid = f.grid.filter(|&(_, inc)| {
                        inc > 0.0
                            && inc.is_finite()
                            && (pp.grid_align == GridAlign::AllLines || (pp.grid_align == GridAlign::FirstLineOnly && line_no == 0))
                    });
                    if let Some((g_start, inc)) = grid {
                        let n = pp.grid_gyoudori.min(grid::MAX_GYOUDORI);
                        match para_block.filter(|(at, _)| line_no > 0 && *at == (cur.fi, cur.col)) {
                            // Inside a paragraph-gyoudori block, lines after the first keep their leading.
                            Some((_, end)) => grid_end = Some(end),
                            None => {
                                let r = grid::reference_offset(line_glyphs, pp.grid_reference);
                                let block = n > 0 && pp.paragraph_gyoudori && line_no == 0;
                                let span = if block {
                                    // From this line's reference point to the paragraph's last line's.
                                    let aki_below = |model, lead| (model == designcraft_doc::cjk::LeadingModel::AkiBelow).then_some(lead);
                                    let (mut down, mut prev, mut prev_aki, mut last_r) = (0.0, reference, aki_below(model, lead), r);
                                    for (j, nb) in breaks.iter().enumerate().skip(k + 1) {
                                        let (ns, ne) = (g0 + nb.start, g0 + nb.end);
                                        let lg = glyphs.get(ns..ne.max(ns)).unwrap_or_default();
                                        let (_, _, _, nlead) = line_metrics(lg, &glyphs, ns, mark_leading, mark_chars.size, db, &mark_chars);
                                        let nlead = match last_line_mark {
                                            Some(m) if j + 1 == breaks.len() => nlead.max(m),
                                            _ => nlead,
                                        };
                                        let (nref, nmodel) = cjk_line_reference(lg);
                                        down += prev_aki.unwrap_or(nlead) + prev - nref;
                                        prev = nref;
                                        prev_aki = aki_below(nmodel, nlead);
                                        last_r = grid::reference_offset(lg, pp.grid_reference);
                                    }
                                    (down + last_r - r).max(0.0)
                                } else {
                                    0.0
                                };
                                let lines = if block { grid::lines_for(span, inc, n) } else { n.max(1) };
                                let free = cur.grid_next.filter(|(b, _)| cur.last_baseline == Some(*b)).map(|(_, y)| y);
                                let band = grid::Band { natural: baseline + r, free, pending: cur.pending, fixed: n > 0, lines, span };
                                let (y, end) = grid::place((g_start, inc), pp.grid_reference, band);
                                baseline = y - r;
                                grid_end = Some(end);
                                if block {
                                    para_block = Some(((cur.fi, cur.col), end));
                                }
                            }
                        }
                    }
                    // A line a wrap pushes down moves by whole grid lines, keeping its place in its band.
                    let on_grid = baseline;
                    let snap = |y: f64| grid.map_or(y, |(_, inc)| on_grid + ((y - on_grid) / inc - 1e-6).ceil().max(0.0) * inc);
                    // Wrap: push the line down to the first position with a slot (back on the grid).
                    // Segments after the first are carried to the next lines of the paragraph.
                    if !f.exclusions.is_empty() || f.outline.is_some() {
                        let jump = cur.jump(f, advance, asc, tops);
                        let line_start = glyphs.get(s).map_or(prange.start, |g| g.byte);
                        let mut tries = 0;
                        loop {
                            let Some((nb, segs)) = wrap_slot(f, col, baseline, asc, desc, jump, base_size, rtl, line_start, &holds) else {
                                // No room in this column.
                                baseline = baseline.max(col.y1) + 1.0;
                                carry.clear();
                                break;
                            };
                            let pushed = nb > baseline;
                            baseline = nb;
                            if let Some((&first, rest)) = segs.split_first() {
                                (x0, x1) = first;
                                carry = rest.to_vec();
                            }
                            tries += 1;
                            if !pushed || grid.is_none() || tries > 4000 {
                                break;
                            }
                            baseline = snap(baseline);
                        }
                    }
                    grid_end = grid_end.map(|end| end + baseline - on_grid);
                    carry_baseline = baseline;
                    carry_grid = grid_end;
                }
                // The breaker set this line for another segment width: re-break the rest of the
                // paragraph from here, starting with this line's segments as they are. A pass
                // re-breaks at its first line only for a new line, whose segments it carries, so
                // every re-break makes progress; their number is capped too.
                // Past the estimated slots the breaker assumes the whole column. Check that
                // fallback too: narrow wrap segments may need more lines than the estimate.
                let (estimated_x0, estimated_x1) = slots.get(k).copied().unwrap_or((col.x0, col.x1));
                if (k > 0 || !continues) && rebreaks < 256 && ((estimated_x1 - estimated_x0) - (x1 - x0)).abs() > 0.5 {
                    rebreaks += 1;
                    carry.insert(0, (x0, x1));
                    g0 = s;
                    moved = true;
                    break;
                }
                let capped = cur.split.is_none() && line_cap[pi] == Some(line_no) && line_no > col_first_line;
                // Footnotes referenced on this line need room at the bottom of the column too
                // (a line at the top of a column is set anyway).
                let line_notes = if notes.active() { notes.refs_in(line_glyphs) } else { Vec::new() };
                let col_w = notes.area(&cols[cur.fi], cur.col).map_or(0.0, |r| r.width());
                let reserve = notes.reserve(doc, cur.fi, cur.col, &line_notes, col_w, f, opts);
                let reserve = if cur.last_baseline.is_none() { notes.reserve(doc, cur.fi, cur.col, &[], col_w, f, opts) } else { reserve };
                // A horizontal text frame's bottom inset limits the last baseline, not
                // the font's descender box. Descenders remain in the line metrics and may
                // hang below that edge. Paths, vertical text, inline objects and text above
                // reserved footnotes keep the full extent to prevent collisions.
                let fit_descent = if f.opts.path.is_some()
                    || f.vertical
                    || reserve > 0.0
                    || line_glyphs.iter().any(|g| sub_objects.get(&g.byte).is_some_and(|o| !o.custom))
                {
                    desc
                } else {
                    0.0
                };
                let fits = baseline + fit_descent <= col.y1 - reserve + 0.01 && !capped;
                if !fits {
                    if cur.can_next_sub() {
                        g0 = s;
                        cur.next_sub();
                        col_first_line = line_no;
                        carry.clear();
                        moved = true;
                        break;
                    }
                    // A split block that doesn't fit above its trial bottom: re-lay it lower.
                    if let Some(key) = cur.split_key()
                        && limit_of(&split_limits, key).is_some_and(f64::is_finite)
                    {
                        let j = if split_trial.as_ref().is_some_and(|t| t.key == key) {
                            trial_failed(&mut split_trial, &mut split_limits, split_runs < split_budget)
                        } else {
                            set_limit(&mut split_limits, key, Some(f64::INFINITY));
                            Some(key.2)
                        };
                        if let Some(j) = j {
                            split_runs += 1;
                            rewind(j, &snaps, &note_snaps, &mut notes, &mut out, &mut cur, &mut list_counter, &mut force_col, &mut line_cap);
                            pi = j;
                            continue 'paras;
                        }
                    }
                    // Keep options don't apply between a split block's lines.
                    if !capped && cur.split.is_none_or(|s| s.part_lines == 0) {
                        let ctx = KeepCtx {
                            pi,
                            line_no,
                            in_col: line_no - col_first_line,
                            started_here: col_first_line == 0,
                            at_top,
                            remaining: breaks.len() - k,
                            here: (cur.fi, cur.col),
                        };
                        let action = if restores < (4 * np + 64) * (balance_runs + 1) {
                            keep_violation(&ctx, &pp, &info, &force_col, &line_cap)
                        } else {
                            None
                        };
                        if let Some(action) = action {
                            restores += 1;
                            let j = match action {
                                KeepAction::Force(j) => {
                                    force_col[j] = true;
                                    j
                                }
                                KeepAction::Cap(j, n) => {
                                    line_cap[j] = Some(n);
                                    j
                                }
                            };
                            rewind(j, &snaps, &note_snaps, &mut notes, &mut out, &mut cur, &mut list_counter, &mut force_col, &mut line_cap);
                            pi = j;
                            continue 'paras;
                        }
                    }
                    // Next column / frame; re-break the rest of the paragraph there.
                    g0 = s;
                    next_column(&mut cur);
                    col_first_line = line_no;
                    carry.clear();
                    moved = true;
                    break;
                }
                // A drop cap sits at the start-side indent; the first-line indent doesn't move it.
                let beside = drop_cap.as_ref().filter(|dc| line_no < dc.lines && col_first_line == 0).map(DropCap::indent);
                let first_line = if line_no == 0 { pp.first_line_indent } else { 0.0 };
                let (ind_l, ind_r) = match beside {
                    Some(w) if rtl => (pp.left_indent, pp.right_indent + w),
                    Some(w) => (pp.left_indent + w, pp.right_indent),
                    // The first-line indent is at the paragraph's start side, which is the right
                    // one in a right-to-left paragraph: applied on the left it moved the line the
                    // wrong way, out of the frame for the hanging indent a numbered list uses
                    // (#1448).
                    None if rtl => (pp.left_indent, pp.right_indent + first_line),
                    None => (pp.left_indent + first_line, pp.right_indent),
                };
                let lx0 = x0 + ind_l;
                let lx1 = x1 - ind_r;
                let last = k + 1 == breaks.len();
                // The break character that ends this line, if any (it is the line's last glyph).
                let brk = if b.forced && e > s {
                    e.checked_sub(1).and_then(|i| glyphs.get(i)).map(|g| g.ch).filter(|&c| breaker::is_forced(c))
                } else {
                    None
                };
                // A column, frame or page break ends the line as a paragraph's last line (last-line
                // alignment); a justified line ended by a forced line break stays justified.
                let ends_para = last || brk.is_some_and(story::is_break_char);
                let forced_mid = brk == Some(story::FORCED_LINE_BREAK) && !last;
                let (mut placed, end_x, ratio) =
                    layout_line(&glyphs, s, e, b.hyphen, lx0, lx1, col.x0, &pp, &spacing, ends_para, forced_mid, f.left_page, &bidi_info);
                let end_x = if line_no == 0 && s == 0 { align_label(&mut placed, label_aligned, label_tab, list_align, end_x) } else { end_x };
                // Warichu runs before ruby so a reading is placed over the stacked note, and before
                // the trimmed spaces and the drop cap join the line, so neither is stacked.
                let end_x = end_x + warichu::place(&styles_tab, &mut placed);
                // The spaces the break trimmed off the end follow the line undrawn, so the caret
                // and the selection reach them; alignment and justification ignore them.
                if pp.direction != designcraft_doc::TextDirection::RightToLeft && !placed.iter().any(|g| g.rtl) {
                    let mut x = end_x;
                    for g in glyphs.get(e..(g0 + b.next).min(glyphs.len())).unwrap_or(&[]).iter().filter(|g| g.is_space()) {
                        placed.push(PlacedGlyph { visible: false, ..place(g, x) });
                        x += g.adv;
                    }
                }
                let mut drop_box = None;
                if line_no == 0
                    && let Some(dc) = &drop_cap
                {
                    let at = if rtl { x1 - pp.right_indent - dc.width } else { x0 + pp.left_indent - dc.lsb };
                    placed.splice(0..0, place_drop_cap(dc, at, rtl));
                    drop_box = Some(DropCapBox {
                        end: dc.end,
                        baseline: baseline + dc.drop,
                        // From its line's top, or its own top when it rises above that line.
                        rect: Rect::new(at, (baseline - asc).min(baseline + dc.drop - dc.cap), at + dc.width, baseline + dc.drop + desc),
                    });
                }
                ruby::annotate(db, &mut styles_tab, &story.text, &mut placed, doc.settings.glyph_fallback, f.vertical);
                kenten::annotate(db, &styles_tab, &mut placed, doc.settings.glyph_fallback);
                let range_end = if last { prange.end } else { glyphs.get(g0 + b.next).map(|g| g.byte).unwrap_or(prange.end) };
                let range_start = glyphs.get(s).map(|g| g.byte).unwrap_or(prange.start).min(range_end);
                let range_start = if line_no == 0 { prange.start } else { range_start };
                let ft = &mut out.frames[cur.fi];
                if line_no == 0 {
                    first_line_rect = Some((cur.fi, baseline, asc, x0));
                }
                if let Some(sb) = cur.split.as_mut() {
                    sb.line0.get_or_insert(ft.lines.len());
                    sb.sub_lines += 1;
                    sb.part_lines += 1;
                }
                ft.lines.push(Line {
                    column: cur.col as u32,
                    baseline,
                    x0,
                    x1,
                    ascent: asc,
                    descent: desc,
                    leading: lead,
                    range: range_start..range_end,
                    para: pi,
                    glyphs: placed,
                    hyphenated: b.hyphen,
                    first_in_para: line_no == 0,
                    last_in_para: last,
                    end_x,
                    spacing: ratio,
                    hj: hj_severity(ratio, pp.word_space_min, pp.word_space_max),
                    keep_violation: false,
                    drop_cap: drop_box,
                });
                let placed_notes = !line_notes.is_empty();
                for k in line_notes {
                    notes.place(doc, k, cur.fi, cur.col, col_w, f, opts);
                }
                // Notes spanning the columns take room from the columns before this one too: re-lay
                // the frame with that room left.
                if placed_notes {
                    let r = notes.reserve(doc, cur.fi, cur.col, &[], col_w, f, opts);
                    if notes.earlier_columns_collide(&out.frames[cur.fi].lines, cur.fi, cur.col, &cols[cur.fi], r)
                        && let Some(j) = out.frames[cur.fi].lines.first().map(|l| l.para)
                    {
                        rewind(j, &snaps, &note_snaps, &mut notes, &mut out, &mut cur, &mut list_counter, &mut force_col, &mut line_cap);
                        pi = j;
                        continue 'paras;
                    }
                }
                if !sub_objects.is_empty() {
                    let ft = &mut out.frames[cur.fi];
                    let li = ft.lines.len() - 1;
                    let text_ascent = line_glyphs.iter().filter(|g| g.ch != designcraft_doc::OBJECT_MARK).map(|g| g.ascent).fold(0.0, f64::max);
                    let text_ascent = if text_ascent > 0.0 { text_ascent } else { base_chars.size * 0.75 };
                    for g in ft.lines[li].glyphs.iter().filter(|g| g.len > 0) {
                        if let Some(o) = sub_objects.get(&g.byte)
                            && let Some(source) = story.objects.get(o.index)
                        {
                            ft.objects.push(PlacedObject {
                                index: o.index,
                                object: Arc::clone(source),
                                origin: Point::ZERO,
                                size: (o.w, o.h),
                                line: li,
                                x: g.x,
                                text_ascent,
                            });
                        }
                    }
                }
                cur.last_baseline = Some(baseline);
                cur.last_reference = reference;
                cur.aki_below = (model == designcraft_doc::cjk::LeadingModel::AkiBelow).then_some(lead);
                cur.last_descent = desc;
                cur.pending = 0.0;
                cur.grid_next = grid_end.map(|end| (baseline, end));
                line_no += 1;
                // A column / frame / page break inside the paragraph: the rest of it continues in the
                // new column/frame/page, re-broken there. (A break that ends the paragraph moves the
                // next paragraph instead.)
                if b.forced
                    && k + 1 < breaks.len()
                    && let Some(start) = brk.and_then(break_start)
                {
                    go(&mut cur, start);
                    col_first_line = line_no;
                    g0 += b.next;
                    carry.clear();
                    moved = true;
                    break;
                }
                // An own-story anchored object anchored in this line wraps the lines after it,
                // which were broken without it: break them again.
                if k + 1 < breaks.len()
                    && f.exclusions.iter().any(|ex| {
                        ex.after.is_some_and(|a| (range_start..range_end).contains(&a))
                            && ex.rect.x0 < col.x1
                            && ex.rect.x1 > col.x0
                            && ex.rect.y1 > baseline
                            && ex.rect.y0 < col.y1
                    })
                {
                    g0 += b.next;
                    moved = true;
                    break;
                }
            }
            if !moved {
                break;
            }
        }
        info.push(ParaInfo {
            start: start_at,
            at_top,
            end: (cur.fi, cur.col),
            lines: line_no,
            lines_in_end_col: line_no - col_first_line,
            keep_with_next: pp.keep_with_next,
            keep_together: pp.keep_lines_together,
            keep_all: pp.keep_all_lines,
            keep_first: pp.keep_first as usize,
            keep_last: pp.keep_last as usize,
        });
        // The columns below a spanning paragraph start under it.
        if spanning
            && let Some(ft) = out.frames.get(cur.fi)
            && let Some(l) = ft.lines.last().filter(|l| l.para == pi)
        {
            let above = Above {
                top: l.baseline + l.descent,
                baseline: l.baseline,
                descent: l.descent,
                reference: cur.last_reference,
                aki_below: cur.aki_below,
                pending: if spans_in(cur.fi) { pp.space_after.max(pp.span_space_after) } else { pp.space_after },
            };
            cur.band = Band { para: pi + 1, line0: ft.lines.len(), above: Some(above), tail: None };
        }
        // Rules and shading for the paragraph.
        if let Some((fi, bl, _, _)) = first_line_rect {
            let ft = &mut out.frames[fi];
            if pp.rule_above.on {
                let r = &pp.rule_above;
                // The offset runs from the first line's baseline up to the rule's bottom edge.
                let y = bl - r.offset;
                let col = ft
                    .lines
                    .iter()
                    .find(|l| l.para == pi && l.first_in_para)
                    .map(|l| if r.column_width { (l.x0, l.x1) } else { (l.x0, l.end_x) })
                    .unwrap_or((0.0, 0.0));
                let (color, tint) = rule_color(r, &above);
                ft.decos.push(Deco {
                    rect: Rect::new(col.0 + r.left_indent, y - r.weight, col.1 - r.right_indent, y),
                    color,
                    tint,
                    ..Deco::default()
                });
            }
        }
        if pp.rule_below.on
            && let Some(ft) = out.frames.get_mut(cur.fi.min(frames.len().saturating_sub(1)))
            && let Some(l) = ft.lines.iter().rev().find(|l| l.para == pi)
        {
            let r = &pp.rule_below;
            let y = l.baseline + r.offset;
            let (x0, x1) = if r.column_width { (l.x0, l.x1) } else { (l.x0, l.end_x) };
            let (color, tint) = rule_color(r, &below);
            ft.decos.push(Deco { rect: Rect::new(x0 + r.left_indent, y, x1 - r.right_indent, y + r.weight), color, tint, ..Deco::default() });
        }
        // The end of a split block: text after it continues below its deepest sub-column, which
        // are balanced first. A block that ends the story fills its sub-columns in turn.
        let mut ends_split = false;
        if let Some(sb) = cur.split {
            let next =
                story.paras.get(pi + 1).filter(|_| story.para_table(pi + 1).is_none()).and_then(|p| SplitCfg::of(&doc.styles.resolve_para(p).0));
            if next != Some(sb.cfg) {
                cur.split = None;
                ends_split = true;
                let part = out.frames.get(cur.fi).and_then(|ft| {
                    let lines = ft.lines.get(sb.line0?..)?;
                    let top = lines.first().map(|l| l.baseline - l.ascent)?;
                    let deepest = lines.iter().map(|l| (l.baseline, l.descent)).max_by(|a, b| (a.0 + a.1).total_cmp(&(b.0 + b.1)))?;
                    Some((sb.top.unwrap_or(top), deepest))
                });
                if pi + 1 < np
                    && let Some((top, (baseline, descent))) = part
                {
                    let key = (cur.fi, cur.col, sb.para);
                    let active = split_trial.as_ref().is_some_and(|t| t.key == key);
                    if !active && let Some(t) = split_trial.take() {
                        set_limit(&mut split_limits, t.key, None);
                    }
                    if sb.part_lines > 1 && (active || limit_of(&split_limits, key).is_none() && split_runs < split_budget) {
                        let bottom = baseline + descent;
                        let mut t = split_trial.take().unwrap_or(Trial { key, span: pi, lo: top, hi: bottom, tries: 0, done: false });
                        // This layout fits: its bottom is the best so far.
                        t.hi = t.hi.min(bottom);
                        let next = if t.done { t.hi } else { t.next() };
                        set_limit(&mut split_limits, key, Some(next));
                        if !t.done {
                            split_trial = Some(t);
                            split_runs += 1;
                            rewind(key.2, &snaps, &note_snaps, &mut notes, &mut out, &mut cur, &mut list_counter, &mut force_col, &mut line_cap);
                            pi = key.2;
                            continue 'paras;
                        }
                    }
                    cur.last_baseline = Some(baseline);
                    cur.last_descent = descent;
                }
            }
        }
        cur.pending += if spans_in(cur.fi) || ends_split { pp.space_after.max(pp.span_space_after) } else { pp.space_after };
        pi += 1;
    }
    notes.finish(doc, story, frames, &cols, &mut out);
    // Ranges, content heights and vertical justification.
    for (fi, ft) in out.frames.iter_mut().enumerate() {
        let f = &frames[fi];
        if let (Some(a), Some(z)) = (ft.lines.first(), ft.lines.last()) {
            ft.range = a.range.start..z.range.end;
            // The deepest line of the last column (split columns can end higher up).
            let bottom = ft.lines.iter().filter(|l| l.column == z.column).map(|l| l.baseline + l.descent).fold(z.baseline + z.descent, f64::max);
            ft.content_height = bottom - f.area.y0;
        } else {
            let p = ft_prev_end(&out.overset_at, story.text.len());
            ft.range = p..p;
        }
        let before: Vec<f64> = ft.tables.iter().map(|t| ft.lines.get(t.line).map_or(0.0, |l| l.baseline)).collect();
        let spanned = ft.columns.len() > 1 && ft.lines.iter().any(|l| span_paras.get(l.para) == Some(&true))
            || ft.lines.iter().any(|l| split_paras.get(l.para) == Some(&true));
        vertical_justify(ft, f, spanned);
        for (t, b) in ft.tables.iter_mut().zip(before) {
            let now = ft.lines.get(t.line).map_or(b, |l| l.baseline);
            t.shift(now - b);
        }
        // Anchored objects follow their (finally placed) lines.
        if !ft.objects.is_empty() {
            for o in &mut ft.objects {
                let Some(l) = ft.lines.get(o.line) else { continue };
                let Some(obj) = story.objects.get(o.index) else { continue };
                let (w, h) = o.size;
                o.origin = match &obj.position {
                    designcraft_doc::AnchorPosition::Inline { y_offset } => Point::new(o.x, l.baseline - y_offset - h),
                    designcraft_doc::AnchorPosition::AboveLine { align, space_after, .. } => {
                        let x = match align {
                            designcraft_doc::anchored::AnchorAlign::Left => l.x0,
                            designcraft_doc::anchored::AnchorAlign::Center => (l.x0 + l.x1 - w) / 2.0,
                            designcraft_doc::anchored::AnchorAlign::Right => l.x1 - w,
                        };
                        Point::new(x, l.baseline - o.text_ascent - space_after - h)
                    }
                    designcraft_doc::AnchorPosition::Custom {
                        x_relative,
                        y_relative,
                        x_offset,
                        y_offset,
                        object_point,
                        ref_point,
                        keep_within_column,
                        spine_relative,
                    } => {
                        use designcraft_doc::anchored::AnchorRelative as R;
                        let col = ft.columns.get(l.column as usize).copied().unwrap_or(f.area);
                        let area = |r: R| match r {
                            R::Anchor => None,
                            R::TextFrame => Some(f.area),
                            R::ColumnEdge => Some(col),
                            R::PageMargin => Some(f.page_rect.map_or(f.area, |p| p.1)),
                            R::PageEdge => Some(f.page_rect.map_or(f.area, |p| p.0)),
                        };
                        let frac = |i: u8| ([0.0, 0.5, 1.0][(i % 3) as usize], [0.0, 0.5, 1.0][(i / 3).min(2) as usize]);
                        let (mut rfx, rfy) = frac(*ref_point);
                        let (mut ofx, ofy) = frac(*object_point);
                        let mut dx = *x_offset;
                        // The settings describe a left-hand page; a right-hand page mirrors them
                        // across the spine.
                        if *spine_relative && f.right_page {
                            (rfx, ofx, dx) = (1.0 - rfx, 1.0 - ofx, -dx);
                        }
                        let rx = area(*x_relative).map_or(o.x, |a| a.x0 + a.width() * rfx);
                        let ry = area(*y_relative).map_or(l.baseline, |a| a.y0 + a.height() * rfy);
                        let mut y = ry + y_offset - h * ofy;
                        if *keep_within_column {
                            y = y.clamp(col.y0, (col.y1 - h).max(col.y0));
                        }
                        Point::new(rx + dx - w * ofx, y)
                    }
                };
            }
        }
    }
    // Frame ranges for empty frames after the text: start at the end of the text shown.
    let mut last_end = 0;
    for ft in &mut out.frames {
        if ft.lines.is_empty() {
            let p = out.overset_at.unwrap_or(last_end).max(last_end);
            ft.range = p..p;
        } else {
            last_end = ft.range.end;
        }
    }
    // Paragraph borders and shading (the part of a paragraph that fits keeps its box), and column
    // rules, once vertical justification has put the lines in place.
    parabox::draw(&mut out, &boxes, frames);
    for (ft, f) in out.frames.iter_mut().zip(frames) {
        if f.opts.column_rule {
            let rects = column_rule_rects(&ft.columns, &ft.lines, &f.opts);
            let tint = if f.opts.column_rule_tint.is_finite() { f.opts.column_rule_tint.clamp(0.0, 1.0) } else { 1.0 };
            ft.decos.extend(rects.into_iter().map(|rect| Deco { rect, color: f.opts.column_rule_color.clone(), tint, ..Deco::default() }));
        }
    }
    out.styles = styles_tab;
    mark_keep_violations(doc, story, &mut out);
    out
}

/// The columns a spanning paragraph is set across, from `col`'s top to its bottom.
fn span_rect(cols: &[Rect], col: Rect, span: SpanColumns) -> Rect {
    let SpanColumns::Span(n) = span else { return col };
    let n = if n == 0 { cols.len() } else { (n as usize).min(cols.len()) };
    let (Some(a), Some(b)) = (cols.first(), n.checked_sub(1).and_then(|i| cols.get(i))) else { return col };
    Rect::new(a.x0.min(b.x0), col.y0, a.x1.max(b.x1), col.y1)
}

/// Flag the lines of paragraphs whose Keep Options the layout couldn't honour: lines kept
/// together that split, too few lines at the bottom (orphans) or top (widows) of a column, or a
/// keep-with-next paragraph separated from the next one.
fn mark_keep_violations(doc: &Document, story: &Story, out: &mut ComposedStory) {
    // Per paragraph, the columns its lines sit in: ((frame, column), line count) in order.
    let mut cols: HashMap<usize, Vec<((usize, u32), usize)>> = HashMap::new();
    for (fi, ft) in out.frames.iter().enumerate() {
        for l in &ft.lines {
            let v = cols.entry(l.para).or_default();
            match v.last_mut() {
                Some((c, n)) if *c == (fi, l.column) => *n += 1,
                _ => v.push(((fi, l.column), 1)),
            }
        }
    }
    let mut bad: Vec<usize> = Vec::new();
    for (&pi, v) in &cols {
        let Some(pf) = story.paras.get(pi) else { continue };
        let (pp, _) = doc.styles.resolve_para(pf);
        if pp.keep_lines_together && v.len() > 1 {
            let (first, last) = (v[0].1, v[v.len() - 1].1);
            if pp.keep_all_lines || first < pp.keep_first.max(1) as usize || last < pp.keep_last.max(1) as usize {
                bad.push(pi);
                continue;
            }
        }
        if pp.keep_with_next > 0
            && let (Some(end), Some(next)) = (v.last(), cols.get(&(pi + 1)))
            && next.first().is_some_and(|(c, n)| *c != end.0 || (*n < pp.keep_with_next as usize && next.len() > 1))
        {
            bad.push(pi);
        }
    }
    if bad.is_empty() {
        return;
    }
    for ft in &mut out.frames {
        for l in &mut ft.lines {
            if bad.contains(&l.para) {
                l.keep_violation = true;
            }
        }
    }
}

/// Thickest column rule drawn (points).
const MAX_COLUMN_RULE_WEIGHT: f64 = 1000.0;

/// Text Frame Options › Column Rules: one bar per gutter, `column_rule_weight` wide, centred
/// between the facing edges of the two columns and moved by `column_rule_offset`, from the
/// columns' top to their bottom (the text area, inside the insets) shortened by the rule's top
/// and bottom insets. Works in the composed space, so right-to-left and vertical frames (whose
/// columns are laid out in the turned box) get their rules between the columns too.
///
/// A paragraph that spans columns interrupts the rule: the bar stops at the top of its first line
/// and resumes below its last line. Nothing is drawn for a single column or a weight that is not a
/// positive finite number.
pub fn column_rule_rects(columns: &[Rect], lines: &[Line], opts: &TextFrameOptions) -> Vec<Rect> {
    let weight = opts.column_rule_weight;
    if !weight.is_finite() || weight <= 0.0 || columns.len() < 2 {
        return Vec::new();
    }
    let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
    let (offset, top_inset, bottom_inset) =
        (finite(opts.column_rule_offset), finite(opts.column_rule_top_inset), finite(opts.column_rule_bottom_inset));
    let half = weight.min(MAX_COLUMN_RULE_WEIGHT) / 2.0;
    let mut cols: Vec<Rect> =
        columns.iter().copied().filter(|c| c.x0.is_finite() && c.x1.is_finite() && c.y0.is_finite() && c.y1.is_finite()).collect();
    cols.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let mut out = Vec::new();
    for pair in cols.windows(2) {
        let [a, b] = [pair[0], pair[1]];
        let cx = (a.x1 + b.x0) / 2.0;
        let (top, bottom) = (a.y0.max(b.y0) + top_inset, a.y1.min(b.y1) - bottom_inset);
        if bottom <= top {
            continue;
        }
        // Vertical bands of the paragraphs whose lines cross this gutter.
        let mut spans: Vec<(usize, f64, f64)> = Vec::new();
        for l in lines.iter().filter(|l| l.x0 < cx && l.x1 > cx) {
            let (t, z) = (l.baseline - l.ascent, l.baseline + l.descent);
            match spans.iter_mut().find(|s| s.0 == l.para) {
                Some(s) => {
                    s.1 = s.1.min(t);
                    s.2 = s.2.max(z);
                }
                None => spans.push((l.para, t, z)),
            }
        }
        spans.sort_by(|p, q| p.1.total_cmp(&q.1));
        let x = cx + offset;
        let mut y = top;
        for (_, t, z) in spans {
            if t > y {
                out.push(Rect::new(x - half, y, x + half, t.min(bottom)));
            }
            y = y.max(z);
            if y >= bottom {
                break;
            }
        }
        if y < bottom {
            out.push(Rect::new(x - half, y, x + half, bottom));
        }
    }
    out
}

/// Breaker parameters from the paragraph's settings.
fn spacing_for(pp: &ParaProps, base_size: f64) -> Spacing {
    Spacing {
        justify: pp.align.is_justified(),
        word_min: pp.word_space_min,
        word_desired: pp.word_space_desired,
        word_max: pp.word_space_max,
        letter_min: pp.letter_space_min.min(pp.letter_space_desired),
        letter_desired: pp.letter_space_desired,
        letter_max: pp.letter_space_max.max(pp.letter_space_desired),
        glyph_min: pp.glyph_scale_min.min(pp.glyph_scale_desired).max(0.01),
        glyph_desired: pp.glyph_scale_desired.max(0.01),
        glyph_max: pp.glyph_scale_max.max(pp.glyph_scale_desired),
        hyphen_penalty: 50.0 + 450.0 * pp.hyph_weight,
        hyphen_limit: pp.hyph_limit,
        ragged_stretch: base_size * 2.0,
        // The zone is the Single-line Composer's (unjustified text only), as in InDesign: the
        // Paragraph Composer weighs every break of the paragraph instead.
        hyph_zone: if pp.align.is_justified() || !pp.composer.single_line() { 0.0 } else { pp.hyph_zone },
        optical: pp.optical_margin,
        korean_char_breaks: pp.korean_char_breaks,
        kinsoku_priority: match pp.kinsoku_type.as_str() {
            "KinsokuPushInFirst" => 1,
            "KinsokuPushOutFirst" => 2,
            "KinsokuPushOutOnly" => 3,
            _ => 0,
        },
    }
}

/// Desired word spacing, letter spacing and glyph scaling apply to every line (any alignment).
fn apply_desired_spacing(glyphs: &mut [Glyph], pp: &ParaProps) {
    for i in 0..glyphs.len() {
        let (left, right) = glyphs.split_at_mut(i + 1);
        let Some(g) = left.last_mut() else { continue };
        if pp.kinsoku_hang != designcraft_doc::cjk::KinsokuHang::None && pp.kinsoku.as_ref().is_some_and(|k| k.hanging.contains(g.ch)) {
            g.cjk_hang = g.adv;
        }
        g.ideographic_space_elastic = pp.treat_ideographic_space_as_space;
        if g.ch == '\u{3000}' && pp.treat_ideographic_space_as_space {
            g.space = g.adv;
        }
        if let Some(next) = right.first() {
            let a = g.ch;
            let b = next.ch;
            // A kinsoku set rules where CJK text may break; Korean still breaks at spaces.
            let cjk = breaker::cjk_pair(g, next, pp.korean_char_breaks);
            if g.break_after != Some(false) {
                if let Some(set) = &pp.kinsoku {
                    if !set.allows(a, b) {
                        g.break_after = Some(false);
                    } else if cjk {
                        g.break_after = Some(true);
                    }
                }
                if (pp.bunri_kinshi && a == b && matches!(a, '.' | '-' | '…' | '‥' | '—' | '―')) || (pp.rensuuji && a.is_numeric() && b.is_numeric())
                {
                    g.break_after = Some(false);
                }
                // Roman word break: a roman word may break between any two of its letters.
                if pp.roman_word_break && g.break_after.is_none() && roman_letter(a) && roman_letter(b) {
                    g.break_after = Some(true);
                }
            }
        }
    }
    let ws = pp.word_space_desired;
    let ls = pp.letter_space_desired;
    let gs = pp.glyph_scale_desired.max(0.01);
    if (ws - 1.0).abs() < 1e-9 && ls.abs() < 1e-9 && (gs - 1.0).abs() < 1e-9 {
        return;
    }
    for g in glyphs {
        if g.locked_advance {
            continue;
        }
        if g.is_word_space() {
            g.adv += g.space * (ws - 1.0);
        } else if !g.is_space() && g.adv > 0.0 {
            g.adv = g.adv * gs + ls * g.space;
            g.sx *= gs;
            g.dx *= gs;
        }
    }
}

/// Letters of the roman scripts (Latin, Greek, Cyrillic), which Roman Word Break lets break.
fn roman_letter(c: char) -> bool {
    c.is_alphabetic() && matches!(c as u32, 0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x24F | 0x370..=0x3FF | 0x400..=0x4FF | 0x1E00..=0x1EFF)
}

/// Lengths of the composed output and the cursor before a paragraph (keep resolution re-lays
/// paragraphs from here). Only the cursor's frame and later ones can change afterwards.
struct Snapshot {
    cur: Cursor,
    list_counter: ListCounter,
    lines: usize,
    decos: usize,
    tables: usize,
}

impl Snapshot {
    fn take(out: &ComposedStory, cur: &Cursor, list_counter: &ListCounter) -> Snapshot {
        let (lines, decos, tables) = out.frames.get(cur.fi).map_or((0, 0, 0), |f| (f.lines.len(), f.decos.len(), f.tables.len()));
        Snapshot { cur: cur.clone(), list_counter: list_counter.clone(), lines, decos, tables }
    }
    fn restore(&self, out: &mut ComposedStory, cur: &mut Cursor, list_counter: &mut ListCounter) {
        let fi = self.cur.fi;
        for (i, f) in out.frames.iter_mut().enumerate().skip(fi) {
            let (l, d, t) = if i == fi { (self.lines, self.decos, self.tables) } else { (0, 0, 0) };
            f.lines.truncate(l);
            f.decos.truncate(d);
            f.tables.truncate(t);
            f.objects.retain(|o| o.line < l);
        }
        *cur = self.cur.clone();
        *list_counter = self.list_counter.clone();
        out.overset_at = None;
    }
}

/// Re-lay from paragraph `j`: back to its snapshot and footnote numbering, with fresh keep
/// decisions for the paragraphs after it.
#[allow(clippy::too_many_arguments)]
fn rewind(
    j: usize,
    snaps: &[Snapshot],
    note_snaps: &[(u32, Option<usize>, usize)],
    notes: &mut Notes,
    out: &mut ComposedStory,
    cur: &mut Cursor,
    list_counter: &mut ListCounter,
    force_col: &mut [bool],
    line_cap: &mut [Option<usize>],
) {
    force_col.iter_mut().skip(j + 1).for_each(|f| *f = false);
    line_cap.iter_mut().skip(j + 1).for_each(|c| *c = None);
    if let Some(s) = snaps.get(j) {
        s.restore(out, cur, list_counter);
    }
    if let Some(&(num, key, placed)) = note_snaps.get(j) {
        (notes.num, notes.key) = (num, key);
        notes.placed.truncate(placed);
    }
}

/// Paragraphs that tracked changes delete: all their text is deleted, and so is their return or
/// the return before them (then they join a paragraph and add nothing to it).
fn deleted_paras(story: &Story, ranges: &[Range<usize>]) -> Vec<bool> {
    let mut spans: Vec<Range<usize>> = Vec::new();
    for (r, f) in story.runs() {
        if f.over.change != Some(designcraft_doc::ChangeMark::Deleted) {
            continue;
        }
        match spans.last_mut() {
            Some(l) if l.end == r.start => l.end = r.end,
            _ => spans.push(r),
        }
    }
    if spans.is_empty() {
        return vec![false; ranges.len()];
    }
    let covered = |r: Range<usize>| {
        r.is_empty() || spans.get(spans.partition_point(|s| s.end <= r.start)).is_some_and(|s| s.start <= r.start && s.end >= r.end)
    };
    let return_deleted = |r: &Range<usize>| r.end < story.text.len() && covered(r.end..r.end + 1);
    ranges
        .iter()
        .enumerate()
        .map(|(i, r)| covered(r.clone()) && (return_deleted(r) || i.checked_sub(1).and_then(|p| ranges.get(p)).is_some_and(return_deleted)))
        .collect()
}

/// Where a placed paragraph sits (for keep resolution).
#[derive(Clone, Debug)]
struct ParaInfo {
    start: (usize, usize),
    /// Started at the top of a column (moving it can't help).
    at_top: bool,
    end: (usize, usize),
    lines: usize,
    lines_in_end_col: usize,
    keep_with_next: u32,
    keep_together: bool,
    keep_all: bool,
    keep_first: usize,
    keep_last: usize,
}

/// The paragraph being placed when a line doesn't fit its column.
struct KeepCtx {
    pi: usize,
    /// Paragraph line number of the line that doesn't fit.
    line_no: usize,
    /// Lines of this paragraph already in the column.
    in_col: usize,
    /// The paragraph started in this column.
    started_here: bool,
    at_top: bool,
    /// Lines left to set, including the one that doesn't fit.
    remaining: usize,
    here: (usize, usize),
}

enum KeepAction {
    /// Re-lay from paragraph `j`, starting it in the next column.
    Force(usize),
    /// Re-lay from paragraph `j`, moving to the next column after `n` of its lines.
    Cap(usize, usize),
}

/// Keep Options (plan: typography §8): keep with next, keep all lines together, keep first/last
/// lines. Returns how to re-lay, or None to just continue in the next column (also when a keep
/// can't be satisfied, e.g. a paragraph already at the top of a column).
fn keep_violation(ctx: &KeepCtx, pp: &ParaProps, info: &[ParaInfo], force_col: &[bool], line_cap: &[Option<usize>]) -> Option<KeepAction> {
    // The first paragraph of the chain of keep-with-next paragraphs ending in `pi` that sit in this column.
    let chain_start = |pi: usize| {
        let mut j = pi;
        while j > 0 {
            let p = &info[j - 1];
            let starts_here = if j == ctx.pi { ctx.started_here } else { info[j].start == ctx.here };
            if p.keep_with_next > 0 && p.end == ctx.here && starts_here {
                j -= 1;
            } else {
                break;
            }
        }
        j
    };
    let force = |j: usize| -> Option<KeepAction> {
        let top = if j == ctx.pi { ctx.at_top } else { info[j].at_top };
        // Keep the top-of-column fallback even when this paragraph has continued
        // into a later column. Rewinding and forcing its original start would leave
        // that earlier column empty when a spanning keep chain cannot be satisfied.
        (!(force_col[j] || top)).then_some(KeepAction::Force(j))
    };
    let total_lines = ctx.line_no + ctx.remaining;
    // Keep with next: the previous paragraph's last line must share a column with our first lines.
    if ctx.started_here && ctx.pi > 0 {
        let prev = &info[ctx.pi - 1];
        let need = (prev.keep_with_next as usize).min(total_lines);
        if prev.keep_with_next > 0 && prev.end == ctx.here && ctx.line_no < need {
            let j = chain_start(ctx.pi);
            if j == ctx.pi - 1 && prev.lines_in_end_col > 1 && !(prev.keep_together && prev.keep_all) && line_cap[j].is_none() {
                // Move only the previous paragraph's last lines (respecting its widow control).
                let keep_last = if prev.keep_together { prev.keep_last.max(1) } else { 1 };
                let stay = prev.lines_in_end_col.saturating_sub(keep_last);
                let min_stay = if prev.start == ctx.here { if prev.keep_together { prev.keep_first.max(1) } else { 1 } } else { 1 };
                if stay >= min_stay {
                    return Some(KeepAction::Cap(j, prev.lines - keep_last));
                }
            }
            if let Some(a) = force(j) {
                return Some(a);
            }
        }
    }
    if !pp.keep_lines_together || ctx.in_col == 0 {
        return None;
    }
    if pp.keep_all_lines {
        return if ctx.started_here { force(chain_start(ctx.pi)) } else { None };
    }
    // Orphan control: at least `keep_first` lines at the bottom of the column.
    if ctx.started_here && ctx.in_col < (pp.keep_first as usize).max(1) {
        return force(chain_start(ctx.pi));
    }
    // Widow control: at least `keep_last` lines at the top of the next column.
    let keep_last = (pp.keep_last as usize).max(1);
    if ctx.remaining < keep_last {
        let need = keep_last - ctx.remaining;
        let min_stay = if ctx.started_here { (pp.keep_first as usize).max(1) } else { 1 };
        if ctx.in_col >= need + min_stay && line_cap[ctx.pi].is_none() {
            return Some(KeepAction::Cap(ctx.pi, ctx.line_no - need));
        }
        if ctx.started_here {
            return force(chain_start(ctx.pi));
        }
    }
    None
}

/// The start option a column, frame or page break character stands for.
fn break_start(c: char) -> Option<StartParagraph> {
    match c {
        story::COLUMN_BREAK => Some(StartParagraph::NextColumn),
        story::FRAME_BREAK => Some(StartParagraph::NextFrame),
        story::PAGE_BREAK => Some(StartParagraph::NextPage),
        story::ODD_PAGE_BREAK => Some(StartParagraph::NextOddPage),
        story::EVEN_PAGE_BREAK => Some(StartParagraph::NextEvenPage),
        _ => None,
    }
}

fn ft_prev_end(overset: &Option<usize>, len: usize) -> usize {
    overset.unwrap_or(len)
}

#[derive(Clone)]
struct Cursor {
    fi: usize,
    col: usize,
    last_baseline: Option<f64>,
    last_descent: f64,
    last_reference: f64,
    /// The line above's leading when it sets the advance to the next line: under the Japanese
    /// composers' Aki Below model the leading is the space below its line.
    aki_below: Option<f64>,
    /// Space before/after waiting to be added to the next line.
    pending: f64,
    /// The paragraph being laid.
    pi: usize,
    /// The band of columns the cursor is in.
    band: Band,
    /// Column bottoms that balance bands, by band key (kept by the compose loop).
    limits: Rc<Vec<(BandKey, f64)>>,
    /// The split-column block the text is in.
    split: Option<SplitBlock>,
    /// Bottoms that balance split blocks, by block part (kept by the compose loop).
    split_limits: Rc<Vec<(SplitKey, f64)>>,
    /// The first grid line after the last grid-aligned line's band, with that line's baseline (it
    /// holds while `last_baseline` is still that baseline).
    grid_next: Option<(f64, f64)>,
}

/// A band's frame and first paragraph.
type BandKey = (usize, usize);

/// A split block's part in one column: frame, column and the paragraph it starts in.
type SplitKey = (usize, usize, usize);

/// What a balancing trial re-lays from.
trait TrialKey: Copy + PartialEq {
    fn para(&self) -> usize;
}

impl TrialKey for BandKey {
    fn para(&self) -> usize {
        self.1
    }
}

impl TrialKey for SplitKey {
    fn para(&self) -> usize {
        self.2
    }
}

/// Split Columns settings: sub-columns, inside and outside gutters.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SplitCfg {
    n: usize,
    inside: f64,
    outside: f64,
}

impl SplitCfg {
    fn of(pp: &ParaProps) -> Option<SplitCfg> {
        let SpanColumns::Split(n) = pp.span_columns else { return None };
        let gutter = |g: f64| if g.is_finite() { g.max(0.0) } else { 0.0 };
        Some(SplitCfg { n: (n as usize).clamp(2, 64), inside: gutter(pp.split_inside_gutter), outside: gutter(pp.split_outside_gutter) })
    }
    /// Sub-column `sub` of `col`.
    fn sub_rect(&self, col: Rect, sub: usize) -> Rect {
        let (x0, x1) = (col.x0 + self.outside, col.x1 - self.outside);
        let w = ((x1 - x0 - self.inside * (self.n - 1) as f64) / self.n as f64).max(1.0);
        let x = x0 + sub.min(self.n - 1) as f64 * (w + self.inside);
        Rect::new(x, col.y0, x + w, col.y1)
    }
}

/// Where the cursor resumes: the line above (if any) and the space waiting below it.
#[derive(Clone, Copy, Debug)]
struct Resume {
    last_baseline: Option<f64>,
    last_descent: f64,
    last_reference: f64,
    aki_below: Option<f64>,
    pending: f64,
}

/// Consecutive paragraphs with the same Split Columns settings, set in sub-columns of their
/// column: text fills the sub-columns one after the other from the block's top, then moves on to
/// the next column. Text after the block continues below its deepest sub-column.
#[derive(Clone, Copy, Debug)]
struct SplitBlock {
    cfg: SplitCfg,
    /// The sub-column the text is in.
    sub: usize,
    /// Lines set in the current sub-column, and in the block's part in this column.
    sub_lines: usize,
    part_lines: usize,
    /// The part's first line in its frame.
    line0: Option<usize>,
    /// The paragraph the part starts in (balancing re-lays from there).
    para: usize,
    /// Every sub-column starts from here.
    start: Resume,
    /// The part's top.
    top: Option<f64>,
}

/// A run of the frame's columns: the whole frame, or the part between spanning paragraphs.
/// Text fills its columns one after the other; a spanning paragraph closes it.
#[derive(Clone, Copy, Debug, Default)]
struct Band {
    /// First paragraph laid (at least in part) in the band: balancing re-lays from there.
    para: usize,
    /// Lines of the frame above the band.
    line0: usize,
    /// Below a spanning paragraph, its last line: every column of the band continues from it.
    above: Option<Above>,
    /// After the band's last explicit column break: the paragraph after it and the column it
    /// starts. Balance Columns evens out only the columns from there on.
    tail: Option<(usize, usize)>,
}

#[derive(Clone, Copy, Debug)]
struct Above {
    /// Bottom of the spanning paragraph's last line (the band's top).
    top: f64,
    baseline: f64,
    descent: f64,
    reference: f64,
    aki_below: Option<f64>,
    /// The spanning paragraph's space after.
    pending: f64,
}

/// Balancing the columns above a spanning paragraph: a search for the lowest column bottom at
/// which the band's text still fits its frame, re-laying the band at each trial bottom.
#[derive(Debug)]
struct Trial<K = BandKey> {
    key: K,
    /// The spanning paragraph that closes the band.
    span: usize,
    /// Highest bottom known not to fit / lowest known to fit.
    lo: f64,
    hi: f64,
    tries: u32,
    /// The band is laid at `hi` for good.
    done: bool,
}

impl<K> Trial<K> {
    /// The next bottom to try: half way, or `hi` once settled.
    fn next(&mut self) -> f64 {
        self.tries += 1;
        if self.hi - self.lo > 0.5 && self.tries < 16 {
            (self.lo + self.hi) / 2.0
        } else {
            self.done = true;
            self.hi
        }
    }
}

/// A balancing trial whose band didn't fit its frame: records the next bottom to try in
/// `limits` (or, when even the settled bottom fails or the budget is spent, leaves the band
/// unbalanced) and returns the paragraph to re-lay from.
fn trial_failed<K: TrialKey>(trial: &mut Option<Trial<K>>, limits: &mut Rc<Vec<(K, f64)>>, budget_left: bool) -> Option<usize> {
    let t = trial.as_mut()?;
    let key = t.key;
    let bottom = if t.done || !budget_left {
        *trial = None;
        f64::INFINITY
    } else {
        t.lo = limit_of(limits, key).unwrap_or(t.lo);
        t.next()
    };
    set_limit(limits, key, Some(bottom));
    Some(key.para())
}

/// At the end of the story, Balance Columns in the frame the text ends in: lowers the bottom of
/// the frame's last band of columns (from its last explicit column break on) as far as the text
/// still fits the frame, so the columns end at about the same depth. Returns the paragraph to
/// re-lay from while the search goes on; None once the layout is final.
#[allow(clippy::too_many_arguments)]
fn balance_end(
    trial: &mut Option<Trial>,
    limits: &mut Rc<Vec<(BandKey, f64)>>,
    cur: &Cursor,
    cols: &[Vec<Rect>],
    frames: &[FrameSpec],
    out: &ComposedStory,
    np: usize,
    budget_left: bool,
) -> Option<usize> {
    let fi = cur.fi;
    let (para, col0) = cur.band.tail.unwrap_or((cur.band.para, 0));
    let key = (fi, para);
    let active = trial.as_ref().is_some_and(|t| t.key == key && t.span == np);
    if !active && let Some(t) = trial.take() {
        set_limit(limits, t.key, None);
    }
    let columns = cols.get(fi)?;
    if !frames.get(fi)?.opts.balance_columns || col0.saturating_add(1) >= columns.len() {
        return None;
    }
    let lines: Vec<&Line> =
        out.frames.get(fi)?.lines.get(cur.band.line0..).unwrap_or_default().iter().filter(|l| l.column as usize >= col0).collect();
    let bottom = lines.iter().map(|l| l.baseline + l.descent).filter(|b| b.is_finite()).max_by(f64::total_cmp)?;
    if lines.len() < 2 || !(active || limit_of(limits, key).is_none() && budget_left) {
        return None;
    }
    let top = cur.clip(*columns.get(col0)?).y0;
    if !top.is_finite() || bottom <= top {
        return None;
    }
    let mut t = trial.take().unwrap_or(Trial { key, span: np, lo: top, hi: bottom, tries: 0, done: false });
    // This layout fits: its bottom is the best so far.
    t.hi = t.hi.min(bottom);
    let next = if t.done { t.hi } else { t.next() };
    set_limit(limits, key, Some(next));
    if t.done {
        return None;
    }
    *trial = Some(t);
    Some(key.1)
}

fn set_limit<K: TrialKey>(limits: &mut Rc<Vec<(K, f64)>>, key: K, bottom: Option<f64>) {
    let v = Rc::make_mut(limits);
    v.retain(|(k, _)| *k != key);
    if let Some(b) = bottom {
        v.push((key, b));
    }
}

fn limit_of<K: TrialKey>(limits: &[(K, f64)], key: K) -> Option<f64> {
    limits.iter().find(|(k, _)| *k == key).map(|&(_, b)| b)
}

impl Cursor {
    fn next_column(&mut self, cols: &[Vec<Rect>]) {
        self.col += 1;
        if self.fi < cols.len() && self.col >= cols[self.fi].len() {
            self.next_frame();
        } else {
            self.resume();
            self.split_part();
        }
    }
    fn next_frame(&mut self) {
        self.fi += 1;
        self.col = 0;
        self.band = Band { para: self.pi, ..Band::default() };
        self.resume();
        self.split_part();
    }
    fn state(&self) -> Resume {
        Resume {
            last_baseline: self.last_baseline,
            last_descent: self.last_descent,
            last_reference: self.last_reference,
            aki_below: self.aki_below,
            pending: self.pending,
        }
    }
    /// Start a split block (or its part in a new column) here.
    fn split_part(&mut self) {
        let start = self.state();
        let top = start.last_baseline.map(|b| b + start.last_descent);
        let para = self.pi;
        if let Some(s) = self.split.as_mut() {
            *s = SplitBlock { sub: 0, sub_lines: 0, part_lines: 0, line0: None, para, start, top, ..*s };
        }
    }
    /// Move on to the next sub-column of the split block, back at the block's top.
    fn next_sub(&mut self) {
        let Some(s) = self.split.as_mut() else { return };
        s.sub += 1;
        s.sub_lines = 0;
        let r = s.start;
        self.last_baseline = r.last_baseline;
        self.last_descent = r.last_descent;
        self.last_reference = r.last_reference;
        self.aki_below = r.aki_below;
        self.pending = r.pending;
    }
    /// Within a split block, the next sub-column when there's one to go to.
    fn can_next_sub(&self) -> bool {
        self.split.is_some_and(|s| s.sub_lines > 0 && s.sub + 1 < s.cfg.n)
    }
    fn split_key(&self) -> Option<SplitKey> {
        self.split.map(|s| (self.fi, self.col, s.para))
    }
    /// The sub-column the split block's text is in, within `col` and above its balanced bottom.
    fn split_rect(&self, col: Rect) -> Rect {
        let Some(s) = self.split else { return col };
        let r = s.cfg.sub_rect(col, s.sub);
        let y1 = self.split_key().and_then(|k| limit_of(&self.split_limits, k)).map_or(col.y1, |b| b.min(col.y1).max(col.y0));
        Rect::new(r.x0, r.y0, r.x1, y1)
    }
    /// Start the current column at the top of the band.
    fn resume(&mut self) {
        match self.band.above {
            Some(a) => {
                self.last_baseline = Some(a.baseline);
                self.last_descent = a.descent;
                self.last_reference = a.reference;
                self.aki_below = a.aki_below;
                self.pending = a.pending;
            }
            None => {
                self.last_baseline = None;
                self.aki_below = None;
                self.pending = 0.0;
            }
        }
    }
    /// Move on as a paragraph start option (or a break character) asks.
    fn start(&mut self, start: StartParagraph, cols: &[Vec<Rect>], frames: &[FrameSpec], doc: &Document) {
        match start {
            StartParagraph::Anywhere => {}
            StartParagraph::NextColumn => self.next_column(cols),
            StartParagraph::NextFrame => self.next_frame(),
            StartParagraph::NextPage => self.next_page(frames, doc, None),
            StartParagraph::NextOddPage => self.next_page(frames, doc, Some(true)),
            StartParagraph::NextEvenPage => self.next_page(frames, doc, Some(false)),
        }
    }
    /// A start option for a paragraph at the top of a frame: there is nothing to break away from,
    /// but an odd or even page start still has to land on a page of that parity, so a paragraph
    /// at the top of an even page that asks for the next odd page moves on (and vice versa).
    fn start_at_top(&mut self, start: StartParagraph, frames: &[FrameSpec], doc: &Document) {
        let want_odd = match start {
            StartParagraph::NextOddPage => true,
            StartParagraph::NextEvenPage => false,
            _ => return,
        };
        let Some(page) = frames.get(self.fi).and_then(|f| f.page) else { return };
        if (doc.page_number(page) % 2 == 1) != want_odd {
            self.next_page(frames, doc, Some(want_odd));
        }
    }
    /// Move to the next frame on a later page, one with an odd page number (`odd` true) or an
    /// even one (false) when asked. Frames without a page (parent pages) qualify; with no frame
    /// left, the rest of the story is overset.
    fn next_page(&mut self, frames: &[FrameSpec], doc: &Document, odd: Option<bool>) {
        let from = frames.get(self.fi).and_then(|f| f.page);
        let skip = |f: &FrameSpec| f.page.is_some_and(|p| Some(p) == from || odd.is_some_and(|o| (doc.page_number(p) % 2 == 1) != o));
        let mut fi = self.fi.saturating_add(1);
        while frames.get(fi).is_some_and(skip) {
            fi += 1;
        }
        self.next_frame();
        self.fi = fi;
    }
    fn band_key(&self) -> BandKey {
        (self.fi, self.band.para)
    }
    /// The current column of the frame (`col`) limited to the band: below the spanning paragraph
    /// above it and, while balancing, above the trial bottom (Balance Columns limits only the
    /// columns after the band's last column break).
    fn clip(&self, col: Rect) -> Rect {
        let y0 = self.band.above.map_or(col.y0, |a| a.top.max(col.y0).min(col.y1.max(col.y0)));
        let tail = self.band.tail.filter(|&(_, c)| self.col >= c).map(|(p, _)| (self.fi, p));
        let limit = limit_of(&self.limits, self.band_key()).or_else(|| tail.and_then(|k| limit_of(&self.limits, k)));
        let y1 = limit.filter(|b| !b.is_nan()).map_or(col.y1, |b| b.min(col.y1).max(y0));
        Rect::new(col.x0, y0, col.x1, y1)
    }
    /// Baseline for the next line with leading `lead`, ascent `asc` and first baseline heights
    /// `tops`.
    fn next_baseline(&self, f: &FrameSpec, col: Rect, lead: f64, asc: f64, tops: Tops, _pp: &ParaProps) -> f64 {
        match self.last_baseline {
            Some(b) => b + lead + self.pending,
            None => col.y0 + first_baseline_offset(f, lead, asc, tops),
        }
    }
    /// How the next line clears a Jump Object wrap: by whole leadings below the line above, or, first
    /// in its column, as at the top of a frame whose top is the wrap bottom.
    fn jump(&self, f: &FrameSpec, lead: f64, asc: f64, tops: Tops) -> Jump {
        match self.last_baseline {
            Some(_) => Jump { top: lead, step: lead },
            None => Jump { top: first_baseline_offset(f, lead, asc, tops), step: 0.0 },
        }
    }
}

/// The frame's first baseline offset below the top of a column, for a line with leading `lead`,
/// ascent `asc` and first baseline heights `tops`.
fn first_baseline_offset(f: &FrameSpec, lead: f64, asc: f64, tops: Tops) -> f64 {
    let off = match f.opts.first_baseline {
        // Vertical frames measure from the ascent.
        FirstBaseline::Ascent | FirstBaseline::EmboxHeight if f.vertical => asc,
        FirstBaseline::CapHeight if f.vertical => asc * 0.72,
        FirstBaseline::XHeight if f.vertical => asc * 0.5,
        FirstBaseline::Ascent => tops.typo_ascent,
        FirstBaseline::CapHeight => tops.cap,
        FirstBaseline::XHeight => tops.xh,
        FirstBaseline::EmboxHeight => tops.em,
        FirstBaseline::Leading => lead,
        FirstBaseline::Fixed => 0.0,
    };
    off.max(f.opts.first_baseline_min)
}

/// A line's tallest typographic ascender, cap height, x height and em box top above its baseline:
/// the first baseline offsets.
#[derive(Clone, Copy, Default)]
struct Tops {
    typo_ascent: f64,
    cap: f64,
    xh: f64,
    em: f64,
}

/// The em box top above the baseline at `size` ([`designcraft_fonts::FontFace::em_box`]).
fn em_box_top(face: &designcraft_fonts::FontFace, size: f64) -> f64 {
    let em = Some(face.em_box().0 / face.upem).filter(|e| e.is_finite()).unwrap_or(0.88);
    em * size
}

/// Em box relative to the baseline. Font ascender/descender proportions locate it;
/// the box's total extent is the em, rather than the font's optional line gap.
fn cjk_em_box(g: &Glyph) -> (f64, f64) {
    let height = g.face.units_per_em() * g.sy.abs();
    let top = -height * g.ascent / (g.ascent + g.descent).max(1e-9);
    (top, top + height)
}
/// The ideographic character face (ICF) box relative to the baseline: the em box of
/// [`cjk_em_box`] inset by the font's own ICF insets (`BASE` `icft`/`icfb`, else 5% of the em).
fn cjk_icf_box(g: &Glyph) -> (f64, f64) {
    let (top, bottom) = cjk_em_box(g);
    let (em_top, em_bottom) = g.face.em_box();
    let (icf_top, icf_bottom) = g.face.icf_box();
    let em = (em_top - em_bottom).max(1e-9);
    let height = bottom - top;
    let inset = |v: f64| if v.is_finite() { v.clamp(0.0, 0.5) } else { 0.05 };
    (top + inset((em_top - icf_top) / em) * height, bottom - inset((icf_bottom - em_bottom) / em) * height)
}
/// Em box that character alignment lines up, relative to the baseline (y down) at the glyph's
/// size: the font's `BASE` em box, else 0.88 em above and 0.12 em below the baseline. A font's
/// ascender/descender proportions put a Latin em box's centre too low for this.
fn alignment_em_box(g: &Glyph) -> (f64, f64) {
    let upem = g.face.units_per_em();
    let (top, bottom) = g.face.declared_em_box().unwrap_or((upem * 0.88, upem * -0.12));
    let k = g.sy.abs();
    (-top * k, -bottom * k)
}
/// Upward shift that puts `g`'s em box (or ICF) edge or centre where the reference glyph's is.
fn cjk_alignment_shift(g: &Glyph, reference: &Glyph) -> f64 {
    use designcraft_doc::cjk::CharacterAlignment as A;
    let (t, b) = alignment_em_box(g);
    let (rt, rb) = alignment_em_box(reference);
    match g.character_alignment {
        A::Baseline => 0.0,
        A::EmTop => t - rt,
        A::EmCenter => (t + b - rt - rb) / 2.0,
        A::EmBottom => b - rb,
        A::IcfTop => cjk_icf_box(g).0 - cjk_icf_box(reference).0,
        A::IcfBottom => cjk_icf_box(g).1 - cjk_icf_box(reference).1,
    }
}
/// The point leading measures a line from, relative to its baseline, and the leading model of
/// the line's largest character, which sets both.
fn cjk_line_reference(line: &[Glyph]) -> (f64, designcraft_doc::cjk::LeadingModel) {
    use designcraft_doc::cjk::LeadingModel as L;
    let Some(g) = line.iter().max_by(|a, b| a.size.total_cmp(&b.size)) else { return (0.0, L::Roman) };
    let (top, bottom) = cjk_em_box(g);
    let reference = match g.leading_model {
        L::Roman => 0.0,
        // "Space below" measures from the em box top (Em Box Top/Right), "space above" from its
        // bottom (Em Box Bottom/Left).
        L::AkiBelow => top,
        L::AkiAbove => bottom,
        L::Center | L::CenterDown => (top + bottom) / 2.0,
    };
    (reference, g.leading_model)
}

fn line_metrics(
    line: &[Glyph],
    all: &[Glyph],
    s: usize,
    base_leading: f64,
    base_size: f64,
    db: &ScopedFonts<'_>,
    base: &designcraft_doc::CharProps,
) -> (f64, Tops, f64, f64) {
    let src: &[Glyph] = if line.is_empty() { all.get(s..(s + 1).min(all.len())).unwrap_or(&[]) } else { line };
    let face = || db.face(&base.font_family, &base.font_style);
    if src.is_empty() {
        let face = face();
        let k = base_size / face.upem;
        let tops = Tops { typo_ascent: face.typo_ascent * k, cap: face.cap_height * k, xh: face.x_height * k, em: em_box_top(&face, base_size) };
        return (face.ascent * k, tops, face.descent * k, base_leading);
    }
    let mut asc: f64 = 0.0;
    let mut tops = Tops::default();
    let mut desc: f64 = 0.0;
    let mut lead: f64 = 0.0;
    let reference = src.iter().max_by(|a, b| a.size.total_cmp(&b.size));
    let mut own_text = false;
    for g in src {
        let align = reference.map_or(0.0, |r| cjk_alignment_shift(g, r));
        let shift = g.shift + align;
        let up = shift.max(0.0);
        asc = asc.max(g.ascent + up);
        // A list's bullet or number doesn't raise the Ascent offset: only the line's own text does.
        if !g.list_label {
            own_text = true;
            tops.typo_ascent = tops.typo_ascent.max(g.typo_ascent + up);
            // The em box sits at the unshifted baseline: a baseline shift moves only the drawn glyphs.
            tops.em = tops.em.max(em_box_top(&g.face, g.size) + align.max(0.0));
        }
        tops.cap = tops.cap.max(g.cap + up);
        tops.xh = tops.xh.max(g.xh + up);
        desc = desc.max(g.descent - shift.min(0.0));
        lead = lead.max(g.leading);
    }
    if !own_text {
        // Only a label (an empty list paragraph): as an empty paragraph.
        let face = face();
        tops.typo_ascent = face.typo_ascent * base_size / face.upem;
        tops.em = em_box_top(&face, base_size);
    }
    (asc, tops, desc, lead)
}

/// How a line clears Jump Object wraps. The line occupies `[baseline − top, baseline]` (its leading,
/// or its first baseline offset at the top of a column) and must not overlap the wrap; it may hang its
/// descenders into the wrap. Below a wrap it moves down from its place in whole `step`s until it clears
/// the wrap bottom (`step` 0: its top lands on the wrap bottom).
#[derive(Clone, Copy, Debug)]
struct Jump {
    top: f64,
    step: f64,
}

/// Slack for float error when a line is tested against a Jump Object wrap's edges.
const JUMP_EPS: f64 = 1e-6;

/// The first baseline at or below `baseline` where a line with ascent `asc` has free segments in
/// `col`, from its top to its baseline, one of which `holds` the line (its first word fits), and
/// the segments from that one on (see [`free_slots`], never empty); None if there are none above
/// the column bottom. `line_start` is the story byte the line starts at (see
/// [`Exclusion::wraps_line`]).
///
/// A line under a Jump Object wrap moves below it as `jump` says. Around a rectangular wrap the
/// free space only grows where one in the line's band ends: the search jumps from one such bottom
/// edge to the next, and a line that clears a wrap has its top exactly on the wrap's bottom. A
/// contour's width changes within its box, and a shaped frame's along its outline, so beside one
/// the search moves down 1 pt at a time.
#[allow(clippy::too_many_arguments)]
fn wrap_slot(
    f: &FrameSpec,
    col: Rect,
    baseline: f64,
    asc: f64,
    desc: f64,
    jump: Jump,
    size: f64,
    rtl: bool,
    line_start: usize,
    holds: &dyn Fn(&(f64, f64)) -> bool,
) -> Option<(f64, Vec<(f64, f64)>)> {
    let top = jump.top.max(0.0);
    let mut b = baseline;
    // Each step passes at least one wrap; the cap bounds hostile geometry.
    for _ in 0..4000 {
        let jumped = f
            .exclusions
            .iter()
            .filter(|e| e.mode == WrapMode::JumpObject && e.wraps_line(line_start))
            .map(|e| e.rect)
            .filter(|r| r.x0 < col.x1 && r.x1 > col.x0 && b > r.y0 + JUMP_EPS && b - top < r.y1 - JUMP_EPS)
            .fold(f64::NEG_INFINITY, |m, r| m.max(r.y1));
        b = if jumped > f64::NEG_INFINITY {
            if jump.step > 0.0 {
                // Whole steps from the line's own place (at least one step past the line above).
                baseline + ((jumped + top - baseline - JUMP_EPS / 2.0) / jump.step).ceil().max(0.0) * jump.step
            } else {
                jumped + top
            }
        } else {
            // Segments before the first that `holds` the line's first word stay empty.
            let segs: Vec<(f64, f64)> =
                free_slots(f, col, b - asc, b, desc, size, rtl, line_start).into_iter().skip_while(|seg| !holds(seg)).collect();
            if !segs.is_empty() {
                return Some((b, segs));
            }
            let (y0, y1) = (b - asc, b);
            let in_band = || {
                f.exclusions
                    .iter()
                    .filter(|e| e.mode != WrapMode::JumpObject && e.wraps_line(line_start))
                    .filter(|e| e.rect.y0 < y1 && e.rect.y1 > y0)
            };
            let edge = in_band().fold(f64::INFINITY, |m, e| m.min(e.rect.y1)) + asc;
            // A contour's or a shaped frame's width changes within the band: 1 pt at a time.
            let next = if f.outline.is_some() || in_band().any(|e| e.contour.is_some()) { edge.min(b + 1.0) } else { edge };
            // No progress (coordinates so large that a point no longer moves the line) or none left.
            if next <= b || !next.is_finite() {
                return None;
            }
            next
        };
        if b.is_nan() || b > col.y1 {
            return None;
        }
    }
    None
}

/// Free segments of `col` in the band `y0..y1` (a line's top to its baseline: descenders may reach
/// into the wrap; a shaped frame's outline holds them too, `desc` below `y1`), in the order text
/// fills them (left to right, or right to left for right-to-left paragraphs); empty if the band is blocked. A line beside an
/// object that lets text through on both sides has a segment on each side. When an object cuts
/// the column, segments narrower than 1.5 em of `size` are skipped (slivers beside it); a column
/// no object reaches is free however narrow (a one-character frame such as an article's closing
/// tombstone). `line_start` is the story byte the line starts at (see [`Exclusion::wraps_line`]).
#[allow(clippy::too_many_arguments)]
fn free_slots(f: &FrameSpec, col: Rect, y0: f64, y1: f64, desc: f64, size: f64, rtl: bool, line_start: usize) -> Vec<(f64, f64)> {
    // A shaped frame: the parts of the band its outline covers (a concave one can give several),
    // descenders included: they stay inside the frame.
    let mut free = match &f.outline {
        Some(o) => o.intervals(col.x0, col.x1, y0, y1 + desc.max(0.0)),
        None => vec![(col.x0, col.x1)],
    };
    let mut cut = f.outline.is_some();
    for ex in f.exclusions.iter().filter(|e| e.wraps_line(line_start)) {
        let r = ex.rect;
        let overlaps_band = r.y0 < y1 && r.y1 > y0;
        let blocked = match ex.mode {
            WrapMode::JumpToNextColumn if y1 > r.y0 && r.x0 < col.x1 && r.x1 > col.x0 => return vec![],
            WrapMode::BoundingBox | WrapMode::Contour if overlaps_band => match &ex.contour {
                Some(c) => c.band_extent(y0, y1),
                None => Some((r.x0, r.x1)),
            },
            _ => None,
        };
        let Some((a, b)) = blocked.filter(|&(a, b)| a < col.x1 && b > col.x0) else { continue };
        cut = true;
        // One-sided wraps also block the column on the other side of the object.
        let text_left = match ex.side {
            WrapSide::LeftSide => Some(true),
            WrapSide::RightSide => Some(false),
            WrapSide::LargestArea => Some(r.x0 - col.x0 >= col.x1 - r.x1),
            WrapSide::BothSides | WrapSide::TowardsSpine | WrapSide::AwayFromSpine => None,
        };
        let (a, b) = match text_left {
            Some(true) => (a, b.max(col.x1)),
            Some(false) => (a.min(col.x0), b),
            None => (a, b),
        };
        let mut next = Vec::with_capacity(free.len() + 1);
        for (fa, fb) in free {
            if b <= fa || a >= fb {
                next.push((fa, fb));
                continue;
            }
            if a > fa {
                next.push((fa, a));
            }
            if b < fb {
                next.push((b, fb));
            }
        }
        free = next;
    }
    if cut {
        free.retain(|(a, b)| b - a >= size * 1.5);
    }
    if rtl {
        free.reverse();
    }
    free
}

/// Line slots for the breaker: the free segments of each line from baseline `first` down, `lead`
/// apart, for lines with `band` (ascent, descent); `carry` holds segments left on the line above
/// (they come first).
#[allow(clippy::too_many_arguments)]
fn estimate_slots(
    f: &FrameSpec,
    col: Rect,
    first: f64,
    jump: Jump,
    lead: f64,
    size: f64,
    band: (f64, f64),
    glyphs: &[Glyph],
    rtl: bool,
    carry: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    if f.exclusions.is_empty() && f.outline.is_none() {
        return vec![];
    }
    // Rough upper bound on lines: total advance / column width × 2.
    let total: f64 = glyphs.iter().map(|g| g.adv).sum();
    let n = ((total / (col.width().max(10.0) * 0.5)).ceil() as usize + 2).min(2000);
    let mut v = Vec::with_capacity(n + carry.len());
    v.extend_from_slice(carry);
    let mut b = first;
    let mut jump = jump;
    // Own-story anchored objects whose anchor is in these lines wrap only the lines after it: the
    // rest is broken again once that line is set.
    let line_start = glyphs.first().map_or(0, |g| g.byte);
    while v.len() < n + carry.len() {
        let slots = match wrap_slot(f, col, b, band.0, band.1, jump, size, rtl, line_start, &|_| true) {
            Some((nb, s)) => {
                b = nb;
                s
            }
            None => {
                b = b.max(col.y1);
                vec![(col.x0, col.x1)]
            }
        };
        v.extend(slots);
        b += lead;
        jump = Jump { top: lead, step: lead };
    }
    v
}

/// How far a line's word spacing falls outside `[min, max]` (ratios of the space width): 0 inside,
/// then 1–3 for up to 10%, 25% and beyond.
pub fn hj_severity(ratio: f64, min: f64, max: f64) -> u8 {
    let r = if ratio > max + 1e-6 {
        ratio / max.max(1e-6)
    } else if ratio < min - 1e-6 {
        min / ratio.max(1e-6)
    } else {
        return 0;
    };
    if r < 1.1 {
        1
    } else if r < 1.25 {
        2
    } else {
        3
    }
}

/// Advance of a tab that starts `abs` from the tab origin and is followed by `after`, and the
/// explicit tab stop it reaches (none: the left indent or the next default stop, left aligned).
/// Right, centre and character alignment look at the text up to the next tab or forced break.
///
/// The left indent is a left stop of its own: in a hanging indent, the tab after a bullet or
/// number reaches it unless an explicit stop comes first.
fn tab_advance<'a>(tabs: &'a [designcraft_doc::TabStop], left_indent: f64, abs: f64, after: &[Glyph]) -> (f64, Option<&'a designcraft_doc::TabStop>) {
    let indent_ahead = left_indent > abs + 0.01;
    let stop = tabs.iter().find(|t| t.position > abs + 0.01).filter(|t| !indent_ahead || t.position <= left_indent);
    let (pos, align) = match stop {
        Some(t) => (t.position, t.align),
        None if indent_ahead => (left_indent, TabAlign::Left),
        None => (((abs / DEFAULT_TAB).floor() + 1.0) * DEFAULT_TAB, TabAlign::Left),
    };
    let seg = after.iter().take_while(|g| g.ch != '\t' && !breaker::is_forced(g.ch));
    let gap = pos - abs;
    let w = match align {
        TabAlign::Left => gap,
        TabAlign::Right => gap - seg.map(|g| g.adv).sum::<f64>(),
        TabAlign::Center => gap - seg.map(|g| g.adv).sum::<f64>() / 2.0,
        TabAlign::Char => {
            let ch = stop.and_then(|t| t.align_on.chars().next()).unwrap_or('.');
            gap - seg.take_while(|g| g.ch != ch).map(|g| g.adv).sum::<f64>()
        }
    };
    (w.max(0.0), stop)
}

/// Position glyphs `s..e` within `[x0, x1]`; returns (glyphs, end x, word-space ratio).
///
/// Justified lines distribute the difference to the measure in priority order: word spaces up to
/// their limit, then letter spacing, then glyph scaling, and anything left over to the word spaces
/// (an H&J violation). With optical margin alignment, edge punctuation hangs outside `[x0, x1]`.
#[allow(clippy::too_many_arguments)]
fn layout_line(
    glyphs: &[Glyph],
    s: usize,
    e: usize,
    hyphen: bool,
    x0: f64,
    x1: f64,
    tab_origin: f64,
    pp: &ParaProps,
    sp: &Spacing,
    last: bool,
    forced_mid: bool,
    left_page: bool,
    bidi_info: &unicode_bidi::BidiInfo<'_>,
) -> (Vec<PlacedGlyph>, f64, f64) {
    let mut line: Vec<Glyph> = glyphs[s..e.max(s)].to_vec();
    mojikumi::edges(&mut line);
    let reference = line.iter().max_by(|a, b| a.size.total_cmp(&b.size)).cloned();
    if let Some(reference) = reference {
        for g in &mut line {
            g.shift += cjk_alignment_shift(g, &reference);
        }
    }
    if hyphen && let Some(g) = line.last() {
        let h = shape::hyphen_after(g);
        line.push(h);
    }
    // Tabs: compute widths left to right.
    let measure = x1 - x0;
    let mut x = 0.0;
    let mut i = 0;
    let mut leaders: Vec<(usize, String)> = Vec::new();
    // The line's last tab and whether it is a left tab (only text after a left tab justifies).
    let mut last_tab: Option<(usize, bool)> = None;
    while i < line.len() {
        if line[i].ch == '\t' {
            let (w, stop) = tab_advance(&pp.tabs, pp.left_indent, x0 + x - tab_origin, line.get(i + 1..).unwrap_or_default());
            if let Some(t) = stop
                && !t.leader.is_empty()
            {
                leaders.push((i, t.leader.clone()));
            }
            line[i].adv = w;
            // A default tab stop is a left tab.
            last_tab = Some((i, stop.as_ref().is_none_or(|t| t.align == TabAlign::Left)));
        } else if line[i].ch == story::RIGHT_INDENT_TAB {
            // Use the first stop past the right margin, or the last stop at/before it.
            // Choose the stop before checking its leader: an empty leader takes precedence too.
            let right = x1 - tab_origin;
            let stop = pp
                .tabs
                .iter()
                .filter(|t| t.position > right)
                .min_by(|a, b| a.position.total_cmp(&b.position))
                .or_else(|| pp.tabs.iter().filter(|t| t.position <= right).max_by(|a, b| a.position.total_cmp(&b.position)));
            if let Some(t) = stop.filter(|t| !t.leader.is_empty()) {
                leaders.push((i, t.leader.clone()));
            }
            let rest: f64 = line[i + 1..].iter().map(|g| g.adv).sum();
            line[i].adv = (measure - x - rest).max(0.0);
            last_tab = Some((i, false));
        }
        x += line[i].adv;
        i += 1;
    }
    let has_tab = line.iter().any(|g| g.ch == '\t' || g.ch == story::RIGHT_INDENT_TAB);
    // Optical margin alignment: the measure grows by the hang of the edge glyphs.
    let (mut x0, mut measure) = (x0, measure);
    if (sp.optical || pp.kinsoku_hang != designcraft_doc::cjk::KinsokuHang::None) && !has_tab {
        let first = line.iter().find(|g| g.adv > 0.0 && !g.is_space());
        let lastg = line.iter().rev().find(|g| g.adv > 0.0 && !g.is_space());
        let hl = if sp.optical { first.map_or(0.0, |g| breaker::hang(g.ch).0 * g.adv) } else { 0.0 };
        let optical_right = if sp.optical { lastg.map_or(0.0, |g| breaker::hang(g.ch).1 * g.adv) } else { 0.0 };
        let cjk_right = lastg.map_or(0.0, |g| g.cjk_hang);
        let cjk_right = if pp.kinsoku_hang == designcraft_doc::cjk::KinsokuHang::Regular {
            cjk_right.min((line.iter().map(|g| g.adv).sum::<f64>() - measure).max(0.0))
        } else {
            cjk_right
        };
        let hr = optical_right.max(cjk_right);
        x0 -= hl;
        measure += hl + hr;
    }
    let natural: f64 = line.iter().map(|g| g.adv).sum();
    let mut extra = measure - natural;
    // Justification adjusts only the text after the last tab (InDesign keeps tab stops aligned);
    // a line whose last tab is a right, centre, character or right-indent tab is not justified.
    let seg = last_tab.map_or(0, |(i, _)| i + 1);
    let seg_justifies = last_tab.is_none_or(|(_, left)| left);
    // Spaces inside a jidori run keep the run's fitted width, as in the breaker.
    let spaces: Vec<usize> = line.iter().enumerate().skip(seg).filter(|(_, g)| g.is_justify_space() && !g.locked_advance).map(|(i, _)| i).collect();
    let align = match pp.align {
        Align::TowardsSpine => {
            if left_page {
                Align::Right
            } else {
                Align::Left
            }
        }
        Align::AwayFromSpine => {
            if left_page {
                Align::Left
            } else {
                Align::Right
            }
        }
        a => a,
    };
    let justify_this = align.is_justified() && (!last || align == Align::FullyJustified || forced_mid) && seg_justifies;
    // A justified paragraph's last line may have been composed with shrunk spaces: shrink it too.
    let squeeze_last = !justify_this
        && seg_justifies
        && extra < 0.0
        && (align.is_justified() || line.iter().any(|g| g.moji.shrink > 0.0 || mojikumi::start_elastic(g, false, true)[1] > 0.0));
    // Extra advance per glyph (word spaces and letter gaps) and horizontal scale per glyph.
    let mut add = vec![0.0; line.len()];
    let mut scale = vec![1.0; line.len()];
    let mut offset = 0.0;
    // Kashidas: in justified Arabic, the joins of words take the extra length first.
    let mut kashidas: Vec<(usize, f64)> = Vec::new();
    if justify_this && extra > 0.0 && pp.kashidas && pp.arabic_justification != "DefaultJustification" {
        let points: Vec<usize> = kashida_points(&line).into_iter().filter(|&i| i >= seg).collect();
        if !points.is_empty() {
            let total = extra.min(points.iter().map(|&i| line[i].size * 1.5).sum());
            let per = total / points.len() as f64;
            kashidas = points.into_iter().map(|i| (i, per)).collect();
            extra -= total;
            for &(i, l) in &kashidas {
                add[i] += l;
            }
        }
    }
    if (justify_this || squeeze_last) && (extra >= 0.0 || sp.kinsoku_priority != 3) {
        extra = mojikumi::distribute(line.get_mut(seg..).unwrap_or_default(), extra, add.get_mut(seg..).unwrap_or_default());
    }
    if (justify_this || squeeze_last) && !spaces.is_empty() {
        let rebased: Vec<usize> = spaces.iter().map(|&i| i - seg).collect();
        let (seg_line, seg_add, seg_scale) =
            (line.get(seg..).unwrap_or(&[]), add.get_mut(seg..).unwrap_or(&mut []), scale.get_mut(seg..).unwrap_or(&mut []));
        distribute(seg_line, &rebased, extra, sp, seg_add, seg_scale);
    } else if justify_this && spaces.is_empty() && line.len() > seg + 1 && !last && extra < 0.0 {
        // An overfull single word shrinks only as far as the minimum letter spacing and glyph scaling allow.
        let (seg_line, seg_add, seg_scale) =
            (line.get(seg..).unwrap_or(&[]), add.get_mut(seg..).unwrap_or(&mut []), scale.get_mut(seg..).unwrap_or(&mut []));
        distribute(seg_line, &[], extra, sp, seg_add, seg_scale);
    } else if justify_this && spaces.is_empty() && line.len() > seg + 1 && !last {
        // Single word: Single Word Justification.
        match pp.single_word_justify {
            Align::FullyJustified => {
                let gaps: Vec<_> = line
                    .iter()
                    .enumerate()
                    .take(line.len() - 1)
                    .skip(seg)
                    .filter(|(_, g)| !g.moji.active && (!g.locked_advance || g.break_after != Some(false)))
                    .map(|(i, _)| i)
                    .collect();
                if !gaps.is_empty() {
                    let per = extra / gaps.len() as f64;
                    for i in gaps {
                        add[i] += per;
                    }
                }
            }
            // After a tab the word moves by widening the tab, so text before it stays put.
            Align::Center | Align::Right => {
                let shift = if pp.single_word_justify == Align::Center { extra / 2.0 } else { extra };
                match seg.checked_sub(1).and_then(|t| add.get_mut(t)) {
                    Some(tab) => *tab += shift,
                    None => offset = shift,
                }
            }
            _ => {}
        }
    } else {
        let last_align = match align {
            Align::LeftJustified => Align::Left,
            Align::CenterJustified => Align::Center,
            Align::RightJustified => Align::Right,
            Align::FullyJustified => Align::Left,
            a => a,
        };
        offset = match last_align {
            Align::Center => extra / 2.0,
            Align::Right => extra,
            _ => 0.0,
        };
        // Overfull ragged lines are not shifted left of the slot.
        if offset < 0.0 {
            offset = 0.0;
        }
    }
    // Word-space ratio actually used (vs the space width), for H&J highlighting.
    let mut ratio_sum = 0.0;
    let mut ratio_n = 0usize;
    for &i in &spaces {
        let g = &line[i];
        if matches!(g.ch, ' ' | shape::NBSP) && g.space > 0.0 {
            ratio_sum += (g.adv + add[i]) / g.space;
            ratio_n += 1;
        }
    }
    let ratio = if ratio_n > 0 { ratio_sum / ratio_n as f64 } else { 1.0 };
    let mut out = Vec::with_capacity(line.len());
    let mut x = x0 + offset;
    let bidi = bidi_info.has_rtl();
    let mut pens = Vec::with_capacity(if bidi { line.len() } else { 0 });
    for (i, g) in line.iter().enumerate() {
        if bidi {
            pens.push(x);
        }
        let mut p = place(g, x);
        if scale[i] != 1.0 {
            p.sx *= scale[i];
            p.x = x + g.dx * scale[i];
            p.dx = g.dx * scale[i];
            p.adv *= scale[i];
        }
        p.adv += add[i];
        if let Some((_, l)) = kashidas.iter().find(|k| line[k.0].byte == g.byte) {
            // Move the whole cluster, including its attached marks, past the elongation.
            p.x += l;
        }
        x += p.adv;
        if let Some((_, l)) = kashidas.iter().find(|k| k.0 == i) {
            // The glyph keeps its own width; the gap before it is the tatweel's.
            p.adv -= l;
        }
        out.push(p);
    }
    if bidi {
        pens.push(x);
        reorder_visual(&line, &mut out, &pens, x0 + offset, bidi_info);
    }
    for (i, leader) in &leaders {
        if let Some((g, p)) = line.get(*i).zip(out.get(*i)) {
            tab_leader(g, leader, p.x, p.adv, tab_origin, &mut out);
        }
    }
    for (i, l) in kashidas {
        let first = (0..=i).rev().take_while(|&j| line[j].byte == line[i].byte).last().unwrap_or(i);
        let (g, at) = (&line[i], out[first].x - line[first].dx - l);
        let gid = designcraft_fonts::first_glyph(&g.face, &['\u{0640}']);
        let w = g.face.advance(gid) * g.sx;
        if gid == 0 || w <= 0.0 {
            continue;
        }
        // One tatweel stretched over the gap (overlapping its neighbours a little).
        let k = (l + 0.4) / w;
        out.push(PlacedGlyph {
            gid,
            x: at - 0.2,
            y: -g.shift,
            adv: 0.0,
            sx: g.sx * k,
            len: 0,
            generated_text: None,
            upright: false,
            tcy: None,
            dx: 0.0,
            ..place(g, at)
        });
    }
    (out, x, ratio)
}

/// Arabic letters that join the following letter (dual-joining).
fn joins_next(c: char) -> bool {
    matches!(c as u32,
        0x0626 | 0x0628 | 0x062A..=0x062E | 0x0633..=0x063F | 0x0641..=0x0647 | 0x0649 | 0x064A | 0x066E | 0x066F
        | 0x0678..=0x0687 | 0x069A..=0x06BF | 0x06C1 | 0x06CC | 0x06CE | 0x06D0 | 0x06D1 | 0x06FA..=0x06FC)
}

/// Arabic letters that join the preceding one (dual- or right-joining).
fn joins_prev(c: char) -> bool {
    joins_next(c)
        || matches!(c as u32,
            0x0622..=0x0625 | 0x0627 | 0x0629 | 0x062F..=0x0632 | 0x0648 | 0x0671..=0x0673 | 0x0675..=0x0677 | 0x0688..=0x0699
            | 0x06C0 | 0x06C3..=0x06CB | 0x06CD | 0x06CF | 0x06D2 | 0x06D3)
}

/// Where kashidas go on a line: one join per word, after a seen or sad when there is one, else the
/// word's last join (indices of the glyph before the join).
fn kashida_points(line: &[Glyph]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut best: Option<(bool, usize)> = None;
    for (i, g) in line.iter().enumerate() {
        if g.is_space() {
            if let Some((_, k)) = best.take() {
                out.push(k);
            }
            continue;
        }
        // Only consider boundaries between complete clusters. Marks with len=0
        // are part of their base cluster, not word boundaries or insertion sites.
        let Some(n) = line.get(i + 1) else { continue };
        if n.byte == g.byte || !g.allow_kashidas || !n.allow_kashidas || !n.safe_tatweel_before {
            continue;
        }
        if g.face.id() != n.face.id() || g.face.glyph_for('\u{0640}') == 0 {
            continue;
        }
        if !joins_next(g.ch) || !joins_prev(n.ch) || (g.ch == '\u{0644}' && matches!(n.ch, '\u{0622}' | '\u{0623}' | '\u{0625}' | '\u{0627}')) {
            continue;
        }
        let seen = matches!(g.ch as u32, 0x0633..=0x0636);
        if best.is_none_or(|(s, _)| !s || seen) {
            best = Some((seen, i));
        }
    }
    out.extend(best.map(|b| b.1));
    out
}

/// Bidi: put a laid-out line (glyphs in text order, `pens` their pen positions and then the end
/// position) into visual order from `start`, cluster by cluster (Unicode Bidirectional
/// Algorithm, rule L2).
fn reorder_visual(line: &[Glyph], out: &mut [PlacedGlyph], pens: &[f64], start: f64, info: &unicode_bidi::BidiInfo<'_>) {
    use unicode_bidi::{BidiInfo, Level};
    // Clusters: runs of glyphs from the same source character.
    let mut units: Vec<std::ops::Range<usize>> = Vec::new();
    for i in 0..line.len() {
        match units.last_mut() {
            Some(u) if line[u.start].bidi_offset == line[i].bidi_offset && line[u.start].byte == line[i].byte => u.end = i + 1,
            _ => units.push(i..i + 1),
        }
    }
    let Some(first) = line.first() else { return };
    let Some(para) = info.paragraphs.iter().find(|p| p.range.contains(&first.bidi_offset)) else { return };
    let end = line.iter().map(|g| g.bidi_end).max().unwrap_or(first.bidi_end).min(para.range.end);
    let resolved = info.reordered_levels(para, first.bidi_offset..end);
    let levels: Vec<Level> = units.iter().map(|u| resolved.get(line[u.start].bidi_offset).copied().unwrap_or(para.level)).collect();
    let order = BidiInfo::reorder_visual(&levels);
    let width = |u: &std::ops::Range<usize>| pens[u.end] - pens[u.start];
    let mut cur = start;
    let mut shifts = vec![0.0; line.len()];
    for &k in &order {
        let u = &units[k];
        for i in u.clone() {
            shifts[i] = cur - pens[u.start];
            out[i].rtl = levels[k].is_rtl();
        }
        cur += width(u);
    }
    for (p, d) in out.iter_mut().zip(shifts) {
        p.x += d;
    }
}

/// Fill a tab's gap with repeats of its leader, on a grid shared by all lines (so dots align).
fn tab_leader(tab: &Glyph, leader: &str, x: f64, w: f64, origin: f64, out: &mut Vec<PlacedGlyph>) {
    let glyphs: Vec<(u32, f64)> = leader
        .chars()
        .map(|c| {
            let gid = designcraft_fonts::first_glyph(&tab.face, &[c]);
            (gid, tab.face.advance(gid) * tab.sx)
        })
        .collect();
    let pw: f64 = glyphs.iter().map(|g| g.1).sum();
    if pw <= 0.01 || w < pw {
        return;
    }
    let mut at = origin + ((x - origin) / pw).ceil() * pw;
    let end = x + w;
    let mut n = 0;
    while at + pw <= end + 0.01 && n < 2000 {
        for &(gid, adv) in &glyphs {
            out.push(PlacedGlyph {
                rendered_char: '\t',
                face: tab.face,
                gid,
                x: at,
                y: -tab.shift + tab.dy,
                adv: 0.0,
                sx: tab.sx,
                sy: tab.sy,
                style: tab.style,
                byte: tab.byte,
                len: 0,
                generated_text: None,
                visible: true,
                upright: false,
                tcy: None,
                rtl: false,
                dx: 0.0,
            });
            at += adv;
        }
        n += 1;
    }
}

/// Share `extra` points among word spaces, letter gaps and glyph scaling (see [`layout_line`]).
fn distribute(line: &[Glyph], spaces: &[usize], extra: f64, sp: &Spacing, add: &mut [f64], scale: &mut [f64]) {
    let stretch = extra >= 0.0;
    let word: Vec<f64> = spaces
        .iter()
        .map(|&i| {
            let g = &line[i];
            if !g.is_word_space() {
                0.0
            } else if stretch {
                g.space * (sp.word_max - sp.word_desired).max(0.0)
            } else {
                g.space * (sp.word_desired - sp.word_min).max(0.0)
            }
        })
        .collect();
    // Letter gaps: between visible glyphs (not after the line's last glyph).
    let last_box = line.iter().rposition(|g| !g.is_justify_space() && g.adv > 0.0).unwrap_or(0);
    let is_box = |i: usize, g: &Glyph| i < last_box && !g.is_justify_space() && g.adv > 0.0 && (!g.locked_advance || g.break_after != Some(false));
    let letter: Vec<f64> = line
        .iter()
        .enumerate()
        .map(|(i, g)| {
            if !is_box(i, g) {
                0.0
            } else if stretch {
                g.space * (sp.letter_max - sp.letter_desired).max(0.0)
            } else {
                g.space * (sp.letter_desired - sp.letter_min).max(0.0)
            }
        })
        .collect();
    let glyph: Vec<f64> = line
        .iter()
        .map(|g| {
            if g.is_justify_space() || g.adv <= 0.0 || g.locked_advance {
                0.0
            } else {
                let natural = g.adv / sp.glyph_desired.max(0.01);
                natural * if stretch { (sp.glyph_max - sp.glyph_desired).max(0.0) } else { (sp.glyph_desired - sp.glyph_min).max(0.0) }
            }
        })
        .collect();
    let (yw, yl, yg) = (word.iter().sum::<f64>(), letter.iter().sum::<f64>(), glyph.iter().sum::<f64>());
    let mut rem = extra.abs();
    let tw = rem.min(yw);
    rem -= tw;
    let tl = rem.min(yl);
    rem -= tl;
    let tg = rem.min(yg);
    rem -= tg;
    let sign = if stretch { 1.0 } else { -1.0 };
    // Stretch left over beyond every limit is shared equally by the word spaces (fixed-width spaces
    // keep their width), else by the letter gaps; shrink never goes past the minimums (that would
    // overlap glyphs), so an overfull line stays overfull.
    let elastic = spaces.iter().filter(|&&i| line[i].is_word_space()).count();
    let gaps = line.iter().enumerate().filter(|&(i, g)| is_box(i, g)).count();
    let (over, over_gap) = match (stretch && rem > 0.0, elastic, gaps) {
        (false, _, _) => (0.0, 0.0),
        (true, 0, 0) => (0.0, 0.0),
        (true, 0, n) => (0.0, rem / n as f64),
        (true, n, _) => (rem / n as f64, 0.0),
    };
    for (k, &i) in spaces.iter().enumerate() {
        let share = if yw > 1e-9 { tw * word[k] / yw } else { 0.0 };
        add[i] += sign * share + if line[i].is_word_space() { over } else { 0.0 };
    }
    if yl > 1e-9 && tl > 0.0 {
        for (i, l) in letter.iter().enumerate() {
            add[i] += sign * tl * l / yl;
        }
    }
    if over_gap > 0.0 {
        for (i, g) in line.iter().enumerate() {
            if is_box(i, g) {
                add[i] += over_gap;
            }
        }
    }
    if yg > 1e-9 && tg > 0.0 {
        for (i, gcap) in glyph.iter().enumerate() {
            if *gcap > 0.0 {
                let w = line[i].adv;
                scale[i] = (w + sign * tg * gcap / yg) / w;
            }
        }
    }
}

fn place(g: &Glyph, x: f64) -> PlacedGlyph {
    let visible = !(g.ch == '\t'
        || g.ch == '\n'
        || breaker::is_forced(g.ch)
        || g.ch == story::INDENT_HERE
        || g.ch == story::RIGHT_INDENT_TAB
        || g.ch == story::TABLE_ANCHOR
        || g.ch == shape::SOFT_HYPHEN
        || g.ch == shape::HIDDEN);
    PlacedGlyph {
        rendered_char: g.rendered_char,
        face: g.face,
        gid: g.gid,
        x: x + g.dx,
        y: -g.shift + g.dy,
        adv: g.adv,
        sx: g.sx,
        sy: g.sy,
        style: g.style,
        byte: g.byte,
        len: g.len,
        generated_text: g.generated_text.clone(),
        visible,
        upright: g.upright,
        tcy: g.tcy,
        rtl: false,
        dx: g.dx,
    }
}

/// Drop caps are at most this many lines and characters (larger values are capped).
pub const MAX_DROP_CAP_LINES: u32 = 25;
pub const MAX_DROP_CAP_CHARS: u32 = 150;

/// Byte where paragraph `prange`'s drop cap ends: after its first `drop_cap_chars` characters
/// (code points, as InDesign counts them, spaces included), extended to the end of the grapheme
/// cluster they end in so a letter keeps its combining marks; all of them in a shorter paragraph.
/// `prange.start` when it has none: no lines or characters, or vertical text (which sets no drop
/// caps).
fn drop_cap_end(text: &str, prange: Range<usize>, pp: &ParaProps, vertical: bool) -> usize {
    use unicode_segmentation::UnicodeSegmentation as _;
    if vertical || pp.drop_cap_lines == 0 || pp.drop_cap_chars == 0 {
        return prange.start;
    }
    let Some(para) = text.get(prange.clone()) else { return prange.start };
    let para = para.trim_end_matches('\n');
    let n = pp.drop_cap_chars.min(MAX_DROP_CAP_CHARS) as usize;
    let end = para.char_indices().nth(n).map_or(para.len(), |(i, _)| i);
    prange.start + para.grapheme_indices(true).map(|(i, _)| i).find(|i| *i >= end).unwrap_or(para.len())
}

/// A paragraph's drop cap: its first glyphs, enlarged to reach down `lines` lines.
struct DropCap {
    glyphs: Vec<Glyph>,
    /// Story byte it ends at.
    end: usize,
    lines: usize,
    /// Its advance.
    width: f64,
    /// How far its origin sits left of the indent: its first letter's left side bearing with
    /// Align Left Edge, else 0.
    lsb: f64,
    /// Its baseline below the first line's: `lines − 1` leadings of the text after it (grid steps
    /// on a baseline grid), wherever line `lines` is set.
    drop: f64,
    /// Its cap height as set (how far its top sits above its baseline).
    cap: f64,
}

/// Take the drop cap (the glyphs before byte `end`) off the front of a paragraph's glyphs and scale
/// it so its cap height reaches from the first line's cap height down to line `lines`' baseline,
/// measured in the leading and cap height of the text after it: the size of a cap set at that text's
/// size. As in InDesign, a cap set larger (or smaller) is scaled by its size over that text's,
/// keeping its baseline, so it rises above the first line (or falls short of it). A one-line drop cap keeps its own
/// size when that is smaller. On a baseline grid of increment `grid`, lines are that many grid
/// steps apart. With `align_left` its ink starts at the indent (see [`DropCap::lsb`]). Tabs inside
/// it advance to the stops of `tabs` (stops and the start-side indent; in right-to-left paragraphs
/// both measured from the column's right edge). Spaces right after it join it at its size without
/// widening it.
#[allow(clippy::too_many_arguments)]
fn split_drop_cap(
    db: &ScopedFonts<'_>,
    glyphs: &mut Vec<Glyph>,
    end: usize,
    lines: u32,
    base_leading: f64,
    base_cap: f64,
    grid: Option<f64>,
    align_left: bool,
    tabs: (&[designcraft_doc::TabStop], f64),
) -> Option<DropCap> {
    let n = glyphs.iter().take_while(|g| g.byte < end).count();
    if n == 0 {
        return None;
    }
    let lines = lines.clamp(1, MAX_DROP_CAP_LINES) as usize;
    let mut dc: Vec<Glyph> = glyphs.drain(..n).collect();
    let spaces = glyphs.iter().take_while(|g| g.is_space()).count();
    let mut after: Vec<Glyph> = glyphs.drain(..spaces).collect();
    let end = after.last().map_or(end, |g| g.byte.saturating_add(g.len).max(end));
    let body = glyphs.iter().find(|g| g.len > 0 && g.cap > 0.0);
    let (lead, cap, body_size) = body.map_or((base_leading, base_cap, None), |g| (g.leading, g.cap, Some(g.size)));
    let lead = match grid.map(|inc| (lead / inc - 1e-6).ceil().max(1.0) * inc) {
        Some(pitch) if pitch.is_finite() && pitch > 0.0 => pitch,
        _ => lead,
    };
    let drop = (lines - 1) as f64 * lead;
    // The cap's tallest glyph sets its height; its point size over the body's scales the fit.
    let (own, own_size) = dc.iter().fold((0.0, 0.0), |(c, s), g| if g.cap > c { (g.cap, g.size) } else { (c, s) });
    let ratio = match body_size {
        Some(b) if b > 0.0 && own_size > 0.0 && (own_size / b).is_finite() => own_size / b,
        _ => 1.0,
    };
    let k = if own > 0.0 { (drop + cap) / own * ratio } else { 1.0 };
    let k = if lines == 1 { k.min(1.0) } else { k };
    let k = if k.is_finite() && k > 0.0 { k.min(1000.0) } else { 1.0 };
    for g in dc.iter_mut().chain(after.iter_mut()) {
        g.adv *= k;
        g.dx *= k;
        g.dy *= k;
        g.sx *= k;
        g.sy *= k;
        g.ascent *= k;
        g.typo_ascent *= k;
        g.descent *= k;
        g.cap *= k;
        g.xh *= k;
        g.size *= k;
        g.space *= k;
    }
    let lsb = match dc.first() {
        Some(g) if align_left => {
            let ink = designcraft_geom::Shape::bounding_box(&*db.outline(g.face.get(), g.gid));
            let lsb = g.dx + ink.x0 * g.sx;
            if ink.width() > 0.0 && lsb.is_finite() { lsb } else { 0.0 }
        }
        _ => 0.0,
    };
    // Tabs advance to the paragraph's stops, measured from the column's start edge. Right to left,
    // its glyphs run from that (right) edge in this order (see `place_drop_cap`).
    let (stops, indent) = tabs;
    let mut x = indent - lsb;
    for i in 0..dc.len() {
        let tab = match dc.get(i) {
            Some(g) if g.ch == '\t' => Some(tab_advance(stops, indent, x, dc.get(i + 1..).unwrap_or_default()).0),
            _ => None,
        };
        if let (Some(w), Some(g)) = (tab, dc.get_mut(i)) {
            g.adv = w;
        }
        x += dc.get(i).map_or(0.0, |g| g.adv);
    }
    let width = dc.iter().map(|g| g.adv).sum();
    dc.append(&mut after);
    let cap = dc.iter().map(|g| g.cap).fold(0.0, f64::max);
    Some(DropCap { glyphs: dc, end, lines, width, drop, lsb, cap })
}

impl DropCap {
    /// How far the lines beside it are indented: to the end of its advance.
    fn indent(&self) -> f64 {
        self.width - self.lsb
    }
}

/// The drop cap's glyphs set from `at` (right to left: mirrored, its first character on the right).
fn place_drop_cap(dc: &DropCap, at: f64, rtl: bool) -> Vec<PlacedGlyph> {
    let mut x = at;
    let mut out = Vec::with_capacity(dc.glyphs.len());
    for g in &dc.glyphs {
        let pen = if rtl { at + at + dc.width - x - g.adv } else { x };
        let mut p = place(g, pen);
        p.y += dc.drop;
        p.rtl = rtl;
        out.push(p);
        x += g.adv;
    }
    out
}

/// The character attributes of a list label: the paragraph's, under the bullet's or number's
/// character style, in the bullet font.
fn label_chars(doc: &Document, pp: &ParaProps, base: &designcraft_doc::CharProps) -> designcraft_doc::CharProps {
    let bullets = pp.list_type == designcraft_doc::ListType::Bullets;
    let style = if bullets { &pp.bullet_char_style } else { &pp.number_char_style };
    let mut cp = if style.is_empty() || style == designcraft_doc::NO_CHAR_STYLE {
        base.clone()
    } else {
        doc.styles.resolve_char(base, &designcraft_doc::CharFormat { style: style.clone(), over: Default::default() })
    };
    if bullets && !pp.bullet_font.is_empty() {
        cp.font_family = pp.bullet_font.clone();
        if !pp.bullet_font_style.is_empty() {
            cp.font_style = pp.bullet_font_style.clone();
        }
    }
    cp
}

/// List alignment on a paragraph's first line: the label's first `n` glyphs (its text before the
/// tab) are centred on or end at the line start. Text after the label's tab stays at its stop;
/// without a tab the whole line moves with the label. Returns the line's end x.
fn align_label(placed: &mut [PlacedGlyph], n: usize, tab: bool, align: ListAlign, end_x: f64) -> f64 {
    let Some(label) = placed.get(..n).filter(|l| l.iter().all(|g| g.len == 0)) else { return end_x };
    let w: f64 = label.iter().map(|g| g.adv).sum();
    let dx = match align {
        ListAlign::Left => return end_x,
        ListAlign::Center => -w / 2.0,
        ListAlign::Right => -w,
    };
    let moved = if tab { n } else { placed.len() };
    for g in placed.iter_mut().take(moved) {
        g.x += dx;
    }
    if tab { end_x } else { end_x + dx }
}

/// Put generated `label` text before the paragraph's glyphs; returns how many glyphs it added.
fn prepend_label(
    db: &ScopedFonts<'_>,
    glyphs: &mut Vec<Glyph>,
    label: &str,
    at: usize,
    base: &designcraft_doc::CharProps,
    env: shape::TypeEnv,
    table: &mut StyleTable<'_>,
) -> usize {
    // Shape the label as a tiny standalone story so it uses the paragraph's base character style.
    let mut tmp = Story::new(StoryId(0));
    tmp.insert(0, label);
    let styles = designcraft_doc::Styles::default();
    let shaped = shape::shape_para(db, &styles, &tmp, 0, 0..label.len(), base, env, &SubstCtx::default(), table, &[], &[], &[], (0, ""));
    let mut pre: Vec<Glyph> = shaped
        .glyphs
        .into_iter()
        .map(|mut g| {
            if g.generated_text.is_none() {
                g.generated_text = Some(label.get(g.byte..g.byte + g.len).unwrap_or("").into());
            }
            g.byte = at;
            g.len = 0;
            g
        })
        .collect();
    let n = pre.len();
    pre.append(glyphs);
    *glyphs = pre;
    n
}

type LimitsKey = (usize, usize, usize, bool);

/// Mark glyphs after which a hyphen may be inserted (dictionary/pattern points within the
/// paragraph's limits; words with discretionary hyphens break only there).
/// Whether a language (a name or locale code, see [`designcraft_doc::language_tag`]) uses the
/// English hyphenation and spelling dictionaries.
pub fn is_english(language: &str) -> bool {
    // InDesign's English names first: composition asks this for every run.
    language.starts_with("English") || designcraft_doc::language_tag(language).is_some_and(|t| designcraft_doc::language_subtag(t) == "en")
}

/// Byte ranges of paragraph `prange` set in a language other than English, with the hyphenator
/// for that language (None: not hyphenated).
fn foreign_ranges(
    doc: &Document,
    story: &designcraft_doc::Story,
    prange: Range<usize>,
    base: &designcraft_doc::CharProps,
) -> Vec<(Range<usize>, Option<hyphen::Lang>)> {
    if is_english(&base.language)
        && story.runs().all(|(_, f)| f.over.language.as_deref().is_none_or(is_english) && f.style == designcraft_doc::story::NO_CHAR_STYLE)
    {
        return vec![];
    }
    story
        .runs()
        .filter(|(r, _)| r.start < prange.end && r.end > prange.start)
        .map(|(r, f)| (r, doc.styles.resolve_char(base, f).language.clone()))
        .filter(|(_, l)| !is_english(l))
        .map(|(r, l)| (r, hyphen::Lang::for_language(&l)))
        .collect()
}

fn hyphenation_points(
    text: &str,
    glyphs: &[Glyph],
    pp: &ParaProps,
    exceptions: &HashMap<String, Vec<usize>>,
    foreign: &[(Range<usize>, Option<hyphen::Lang>)],
) -> Vec<bool> {
    let mut out = vec![false; glyphs.len()];
    if !pp.hyphenate {
        return out;
    }
    // Korean is set word by word and is not hyphenated (KLREQ). An ordinary Korean document
    // leaves the paragraph language at the default English, so the paragraph's own text decides
    // rather than its language setting (#1688).
    if hyphen::mostly_hangul(text) {
        return out;
    }
    let lim = hyphen::Limits {
        min_word: pp.hyph_min_word as usize,
        after_first: pp.hyph_after_first as usize,
        before_last: pp.hyph_before_last as usize,
        capitalized: pp.hyph_capitalized,
    };
    // Words repeat a lot: remember their points per set of limits.
    static CACHE: Mutex<Vec<(LimitsKey, HashMap<(hyphen::Lang, Box<str>), Box<[usize]>>)>> = Mutex::new(Vec::new());
    let key = (lim.min_word, lim.after_first, lim.before_last, lim.capitalized);
    let mut guard = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let idx = match guard.iter().position(|e| e.0 == key) {
        Some(i) => i,
        None => {
            if guard.len() > 32 {
                guard.clear();
            }
            guard.push((key, HashMap::new()));
            guard.len() - 1
        }
    };
    let cache = &mut guard[idx].1;
    if cache.len() > 100_000 {
        cache.clear();
    }
    let mut i = 0;
    while i < glyphs.len() {
        if !glyphs[i].is_letter() || glyphs[i].len == 0 {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < glyphs.len() && (glyphs[j].is_letter() || glyphs[j].len == 0 || glyphs[j].ch == shape::SOFT_HYPHEN) && !glyphs[j].no_break {
            j += 1;
        }
        // A no-break letter can't start a hyphenatable word.
        if j == i {
            i += 1;
            continue;
        }
        // A discretionary hyphen in a word (or right before it) replaces automatic hyphenation.
        let discretionary = glyphs[i..j].iter().any(|g| g.ch == shape::SOFT_HYPHEN) || (i > 0 && glyphs[i - 1].ch == shape::SOFT_HYPHEN);
        let (a, b) = (glyphs[i].byte, glyphs[j - 1].byte + glyphs[j - 1].len);
        if !discretionary && b > a && b <= text.len() && text.is_char_boundary(a) && text.is_char_boundary(b) {
            let word = &text[a..b];
            // The whitespace-delimited token the word sits in: a Latin run with a Korean particle
            // on it ("News와") or inside an address is not a word to hyphenate (#1688).
            let token_start = text[..a].rfind(char::is_whitespace).map_or(0, |i| i + text[i..].chars().next().map_or(1, char::len_utf8));
            let token_end = text[b..].find(char::is_whitespace).map_or(text.len(), |i| b + i);
            if hyphen::keep_whole(&text[token_start..token_end]) {
                i = j.max(i + 1);
                continue;
            }
            // Do not hyphenate the paragraph's last word unless allowed.
            let is_last_word = !pp.hyph_last_word && glyphs[j..].iter().all(|g| !g.is_letter());
            let lang = foreign.iter().find(|(r, _)| r.contains(&a)).map_or(Some(hyphen::Lang::English), |(_, l)| *l);
            if !is_last_word && let Some(lang) = lang {
                let user = (!exceptions.is_empty()).then(|| exceptions.get(&word.to_lowercase())).flatten();
                let pts: &[usize] = match user {
                    // User dictionary exceptions win over the patterns and the built-in list.
                    Some(v) => v,
                    None => cache.entry((lang, word.into())).or_insert_with(|| hyphen::hyphen_points_in(word, &lim, lang).into_boxed_slice()),
                };
                for &p in pts.iter() {
                    // char index p → byte → glyph whose cluster ends at that byte.
                    let byte = a + word.char_indices().nth(p).map(|(bi, _)| bi).unwrap_or(word.len());
                    if let Some(k) = (i..j).find(|&k| glyphs[k].byte + glyphs[k].len == byte && glyphs[k].len > 0) {
                        out[k] = true;
                    }
                }
            }
        }
        i = j.max(i + 1);
    }
    out
}

/// `spanned`: a paragraph spans the frame's columns, so the lines move together.
fn vertical_justify(ft: &mut FrameText, f: &FrameSpec, spanned: bool) {
    if ft.lines.is_empty() || f.opts.vertical_justification == VerticalJustification::Top {
        return;
    }
    if spanned && f.opts.vertical_justification == VerticalJustification::Justify {
        return;
    }
    // Per column (or all lines together).
    let ncols = if spanned { 1 } else { ft.columns.len().max(1) as u32 };
    for c in 0..ncols {
        let idx: Vec<usize> = ft.lines.iter().enumerate().filter(|(_, l)| spanned || l.column == c).map(|(i, _)| i).collect();
        let (Some(&a), Some(_)) = (idx.first(), idx.last()) else { continue };
        let bottom = idx.iter().map(|&i| ft.lines[i].baseline + ft.lines[i].descent).fold(f64::NEG_INFINITY, f64::max);
        let space = f.area.y1 - bottom;
        if space <= 0.0 {
            continue;
        }
        let _ = a;
        match f.opts.vertical_justification {
            VerticalJustification::Center => idx.iter().for_each(|&i| ft.lines[i].baseline += space / 2.0),
            VerticalJustification::Bottom => idx.iter().for_each(|&i| ft.lines[i].baseline += space),
            VerticalJustification::Justify => {
                // Segments of one line (beside an object) share a row and move together.
                let rows: Vec<usize> = idx
                    .iter()
                    .scan((0usize, f64::NAN), |(row, prev), &i| {
                        let b = ft.lines[i].baseline;
                        if !prev.is_nan() && (b - *prev).abs() > 1e-9 {
                            *row += 1;
                        }
                        *prev = b;
                        Some(*row)
                    })
                    .collect();
                let n = rows.last().copied().unwrap_or(0);
                if n > 0 {
                    let per = space / n as f64;
                    for (&i, &k) in idx.iter().zip(&rows) {
                        ft.lines[i].baseline += per * k as f64;
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------- caret and hit testing (Type tool) ----------

/// Two stacked glyphs this far apart in `y` sit on different rows (warichu).
const ROW_Y: f64 = 0.25;

/// (ascent, descent) of `g` in points, both positive.
fn glyph_extents(g: &PlacedGlyph) -> (f64, f64) {
    let (asc, desc) = g.face.vertical_metrics();
    let sy = g.sy.abs();
    (asc * sy, desc * sy)
}

fn x_span(g: &PlacedGlyph) -> (f64, f64) {
    (g.x.min(g.x + g.adv), g.x.max(g.x + g.adv))
}

fn x_ranges_overlap(a: &PlacedGlyph, b: &PlacedGlyph) -> bool {
    let (a0, a1) = x_span(a);
    let (b0, b1) = x_span(b);
    a0 < b1 - 0.2 && b0 < a1 - 0.2
}

fn line_glyphs(l: &Line) -> Vec<&PlacedGlyph> {
    l.glyphs.iter().filter(|g| g.len > 0).collect()
}

/// `g` shares its x with another glyph on a different baseline (a stacked warichu row).
fn on_stacked_row(glyphs: &[&PlacedGlyph], g: &PlacedGlyph) -> bool {
    glyphs.iter().any(|o| (o.y - g.y).abs() > ROW_Y && x_ranges_overlap(o, g))
}

fn line_has_stacked_rows(l: &Line) -> bool {
    let glyphs = line_glyphs(l);
    glyphs.iter().any(|g| on_stacked_row(&glyphs, g))
}

/// The glyph `caret_x` anchors `pos` to, if the caret is not the line end.
fn caret_anchor(l: &Line, pos: usize) -> Option<&PlacedGlyph> {
    for g in l.glyphs.iter().filter(|g| g.len > 0) {
        if pos >= g.byte && pos < g.byte + g.len {
            return Some(g);
        }
        if g.byte >= pos {
            return Some(g);
        }
    }
    None
}

/// Baseline an underline or strikethrough uses. Warichu follows the small line; everything else
/// stays on the parent baseline (a baseline shift does not move the rule).
pub fn rule_baseline(style: &RunStyle, line: &Line, g: &PlacedGlyph) -> f64 {
    if style.warichu { line.baseline + g.y } else { line.baseline }
}

fn band_quad(x0: f64, x1: f64, top: f64, bot: f64) -> [Point; 4] {
    [Point::new(x0, top), Point::new(x1, top), Point::new(x1, bot), Point::new(x0, bot)]
}

/// Highlight rectangles for bytes `s..e` on `l`. `past_end` extends the line-end caret when the
/// selection continues onto the next line. Stacked rows (warichu) each get their own band.
pub fn highlight_quads(l: &Line, s: usize, e: usize, past_end: bool) -> Vec<[Point; 4]> {
    let selected: Vec<&PlacedGlyph> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte >= s && g.byte < e).collect();
    let stacked = selected.iter().any(|g| on_stacked_row(&selected, g));
    if stacked {
        let mut items: Vec<(f64, f64, f64, f64, f64)> = selected
            .iter()
            .map(|g| {
                let (asc, desc) = glyph_extents(g);
                let (x0, x1) = x_span(g);
                let y = l.baseline + g.y;
                (g.y, x0, x1, y - asc, y + desc)
            })
            .collect();
        items.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut quads = Vec::new();
        let mut cur: Option<(f64, f64, f64, f64, f64)> = None;
        for (y, x0, x1, top, bot) in items {
            match cur {
                Some(c) if (c.0 - y).abs() <= ROW_Y && x0 <= c.2 + 0.5 => {
                    cur = Some((c.0, c.1, c.2.max(x1), c.3.min(top), c.4.max(bot)));
                }
                Some(c) => {
                    quads.push(band_quad(c.1, c.2, c.3, c.4));
                    cur = Some((y, x0, x1, top, bot));
                }
                None => cur = Some((y, x0, x1, top, bot)),
            }
        }
        if let Some(c) = cur {
            quads.push(band_quad(c.1, c.2, c.3, c.4));
        }
        if past_end {
            quads.push(band_quad(l.end_x, l.end_x + 3.0, l.baseline - l.ascent, l.baseline + l.descent));
        }
        return quads;
    }
    let top = l.baseline - l.ascent;
    let bot = l.baseline + l.descent;
    if l.glyphs.iter().any(|g| g.rtl) {
        let mut spans: Vec<(f64, f64)> = selected.iter().map(|g| (g.x, g.x + g.adv)).collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (a, b) in spans {
            match merged.last_mut() {
                Some(m) if a <= m.1 + 0.5 => m.1 = m.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        return merged.into_iter().map(|(a, b)| band_quad(a, b, top, bot)).collect();
    }
    let x0 = caret_x(l, s);
    let x1 = if past_end { l.end_x.max(x0 + 3.0) } else { caret_x(l, e) };
    vec![band_quad(x0, x1, top, bot)]
}

/// Caret geometry for story byte `pos`: (frame index, x, baseline, ascent, descent).
/// A stacked warichu row reports that row's baseline and em, not the parent line's.
pub fn caret(cs: &ComposedStory, pos: usize) -> Option<(usize, f64, f64, f64, f64)> {
    let (fi, l) = caret_line(cs, pos)?;
    // In the drop cap the caret is as tall as it.
    if let Some(dc) = l.drop_cap
        && pos < dc.end
    {
        return Some((fi, caret_x(l, pos), dc.baseline, dc.baseline - dc.rect.y0, l.descent));
    }
    let x = caret_x(l, pos);
    let glyphs = line_glyphs(l);
    let metrics = caret_anchor(l, pos).filter(|g| on_stacked_row(&glyphs, g)).map(|g| {
        let (asc, desc) = glyph_extents(g);
        (l.baseline + g.y, asc, desc)
    });
    let (bl, asc, desc) = metrics.unwrap_or((l.baseline, l.ascent, l.descent));
    Some((fi, x, bl, asc, desc))
}

/// The line the caret at `pos` is drawn on (and its frame index).
fn caret_line(cs: &ComposedStory, pos: usize) -> Option<(usize, &Line)> {
    let mut best: Option<(usize, &Line)> = None;
    for (fi, ft) in cs.frames.iter().enumerate() {
        for l in &ft.lines {
            if pos >= l.range.start && pos <= l.range.end {
                // Prefer the line where pos is not at its end (except the last line of a paragraph).
                let at_end = pos == l.range.end && !l.last_in_para;
                if best.is_none() || !at_end {
                    best = Some((fi, l));
                    if !at_end {
                        break;
                    }
                }
            }
        }
        if best.is_some_and(|(_, l)| pos < l.range.end || l.last_in_para) {
            break;
        }
    }
    best
}

/// The caret's x on line `l` for story byte `pos` (bidi-aware).
pub fn caret_x(l: &Line, pos: usize) -> f64 {
    for g in &l.glyphs {
        if g.len > 0 && pos >= g.byte && pos < g.byte + g.len {
            // Inside a multi-char cluster (ligature): interpolate.
            let t = (pos - g.byte) as f64 / g.len as f64;
            return if g.rtl { g.x + g.adv * (1.0 - t) } else { g.x + g.adv * t };
        }
        if g.len > 0 && g.byte >= pos {
            return if g.rtl { g.x + g.adv } else { g.x };
        }
    }
    match l.glyphs.iter().rev().find(|g| g.len > 0) {
        Some(g) if g.rtl => g.x,
        // After the spaces that end the line.
        Some(g) if !g.visible && g.x >= l.end_x - 1e-9 => g.x + g.adv,
        _ => l.end_x,
    }
}

/// Caret positions on a line with their x, for lines with right-to-left text.
fn caret_stops(l: &Line) -> Vec<(usize, f64)> {
    let mut ps: Vec<usize> = l.glyphs.iter().filter(|g| g.len > 0).flat_map(|g| [g.byte, g.byte + g.len]).collect();
    ps.extend([l.range.start, l.range.end]);
    ps.sort_unstable();
    ps.dedup();
    ps.into_iter().filter(|p| l.range.contains(p) || *p == l.range.end).map(|p| (p, caret_x(l, p))).collect()
}

/// Bidi caret movement: the position one step to the left (or right) of `pos` on its line as
/// drawn. `None` when the line has no right-to-left text or `pos` is at that side's edge of the
/// line: then move in text order ([`line_rtl`] says which way that runs).
pub fn visual_step(cs: &ComposedStory, pos: usize, left: bool) -> Option<usize> {
    let (_, l) = caret_line(cs, pos)?;
    if !l.glyphs.iter().any(|g| g.rtl) {
        return None;
    }
    let x = caret_x(l, pos);
    let stops = caret_stops(l);
    let side = |s: &&(usize, f64)| if left { s.1 < x - 0.01 } else { s.1 > x + 0.01 };
    let pick = stops.iter().filter(side);
    if left { pick.max_by(|a, b| a.1.total_cmp(&b.1)) } else { pick.min_by(|a, b| a.1.total_cmp(&b.1)) }.map(|s| s.0)
}

/// Does the line at `pos` start (in text order) with right-to-left text?
pub fn line_rtl(cs: &ComposedStory, pos: usize) -> bool {
    caret_line(cs, pos).and_then(|(_, l)| l.glyphs.iter().find(|g| g.len > 0)).is_some_and(|g| g.rtl)
}

/// The line of frame `ft` closest to point `p` vertically (within its column), then horizontally
/// (segments of a line beside an object share its baseline).
fn hit_line(ft: &FrameText, p: Point) -> Option<&Line> {
    let x_dist = |l: &Line| (l.x0 - p.x).max(p.x - l.x1).max(0.0);
    ft.lines
        .iter()
        .filter(|l| p.x >= l.x0 - 20.0 && p.x <= l.x1 + 20.0 || ft.columns.len() <= 1)
        .min_by(|a, b| line_dist(a, p.y).total_cmp(&line_dist(b, p.y)).then(x_dist(a).total_cmp(&x_dist(b))))
        .or_else(|| ft.lines.first())
}

/// Story byte of the character drawn under point `p` (frame inner space) in frame `fi`: the
/// glyph whose advance spans `p.x` on the closest line, else the one nearest it. For selecting
/// the word (or the spaces) under a double click; [`hit`] finds a caret position instead.
pub fn hit_char(cs: &ComposedStory, fi: usize, p: Point) -> Option<usize> {
    let ft = cs.frames.get(fi)?;
    let Some(l) = hit_line(ft, p) else { return Some(ft.range.start) };
    let dist = |g: &&PlacedGlyph| {
        let (a, b) = (g.x.min(g.x + g.adv), g.x.max(g.x + g.adv));
        if p.x < a {
            a - p.x
        } else if p.x >= b {
            p.x - b
        } else {
            0.0
        }
    };
    // On a drop cap (it reaches down beside the lines after its own): its character under `p`.
    for dl in &ft.lines {
        let Some(dc) = dl.drop_cap.filter(|dc| dc.rect.contains(p)) else { continue };
        return dl.glyphs.iter().filter(|g| g.len > 0 && g.byte < dc.end).min_by(|a, b| dist(a).total_cmp(&dist(b))).map(|g| g.byte);
    }
    let under = l.glyphs.iter().filter(|g| g.len > 0).min_by(|a, b| dist(a).total_cmp(&dist(b)));
    Some(under.map_or(l.range.start, |g| g.byte))
}

/// Story byte nearest to point `p` (frame inner space) in frame `fi`.
pub fn hit(cs: &ComposedStory, fi: usize, p: Point) -> Option<usize> {
    let ft = cs.frames.get(fi)?;
    if ft.lines.is_empty() {
        return Some(ft.range.start);
    }
    // On a drop cap (it reaches down beside the lines after its own): the nearest caret position in it.
    for l in &ft.lines {
        let Some(dc) = l.drop_cap.filter(|dc| dc.rect.contains(p)) else { continue };
        return l
            .glyphs
            .iter()
            .filter(|g| g.len > 0 && g.byte < dc.end)
            .flat_map(|g| [g.byte, g.byte + g.len])
            .min_by(|a, b| (caret_x(l, *a) - p.x).abs().total_cmp(&(caret_x(l, *b) - p.x).abs()));
    }
    let l = hit_line(ft, p)?;
    if l.glyphs.iter().any(|g| g.rtl) {
        // Bidi: the caret position drawn nearest the point.
        return caret_stops(l).into_iter().min_by(|a, b| (a.1 - p.x).abs().total_cmp(&(b.1 - p.x).abs())).map(|s| s.0);
    }
    if line_has_stacked_rows(l) {
        return Some(hit_stacked(l, p));
    }
    let mut best = l.range.start;
    let mut prev_mid = f64::NEG_INFINITY;
    for g in l.glyphs.iter().filter(|g| g.len > 0) {
        let mid = g.x + g.adv / 2.0;
        if p.x < mid && p.x >= prev_mid {
            return Some(g.byte);
        }
        prev_mid = mid;
        best = g.byte + g.len;
    }
    // After the last glyph: line end (before the paragraph separator / break).
    Some(if l.last_in_para { l.range.end } else { best.min(l.range.end) })
}

fn line_dist(l: &Line, y: f64) -> f64 {
    let top = l.baseline - l.ascent;
    let bot = l.baseline + l.descent;
    if y < top {
        top - y
    } else if y > bot {
        y - bot
    } else {
        0.0
    }
}

fn glyph_contains(l: &Line, g: &PlacedGlyph, p: Point) -> bool {
    let (x0, x1) = x_span(g);
    if p.x < x0 - 0.01 || p.x > x1 + 0.01 {
        return false;
    }
    let (asc, desc) = glyph_extents(g);
    let y = l.baseline + g.y;
    p.y >= y - asc && p.y <= y + desc
}

/// Midpoint walk over `glyphs` (visual order). Past the last glyph yields the byte after it, or
/// the line end when that glyph closes the line.
fn hit_sorted(l: &Line, mut glyphs: Vec<&PlacedGlyph>, p_x: f64) -> usize {
    glyphs.sort_by(|a, b| a.x.total_cmp(&b.x));
    let last_byte = l.glyphs.iter().rev().find(|g| g.len > 0).map(|g| g.byte);
    let mut best = l.range.start;
    let mut prev_mid = f64::NEG_INFINITY;
    let mut saw_last = false;
    for g in &glyphs {
        if last_byte == Some(g.byte) {
            saw_last = true;
        }
        let mid = g.x + g.adv / 2.0;
        if p_x < mid && p_x >= prev_mid {
            return g.byte;
        }
        if mid > prev_mid {
            prev_mid = mid;
        }
        best = g.byte + g.len;
    }
    if glyphs.is_empty() {
        return if l.last_in_para { l.range.end } else { l.range.start };
    }
    if saw_last && l.last_in_para { l.range.end } else { best.min(l.range.end) }
}

fn hit_stacked(l: &Line, p: Point) -> usize {
    let glyphs = line_glyphs(l);
    let stacked = |g: &PlacedGlyph| on_stacked_row(&glyphs, g);
    let mut containing: Vec<&PlacedGlyph> = glyphs.iter().copied().filter(|g| glyph_contains(l, g, p)).collect();
    containing.sort_by(|a, b| {
        let da = (l.baseline + a.y - p.y).abs();
        let db = (l.baseline + b.y - p.y).abs();
        da.total_cmp(&db).then(b.x.total_cmp(&a.x))
    });
    if let Some(g) = containing.first() {
        let y = g.y;
        let row: Vec<&PlacedGlyph> = if stacked(g) {
            glyphs.iter().copied().filter(|o| (o.y - y).abs() <= ROW_Y).collect()
        } else {
            glyphs.iter().copied().filter(|o| !stacked(o)).collect()
        };
        return hit_sorted(l, row, p.x);
    }
    // Between the rows, or in the line box beside them: the nearest row under this x, else the
    // glyphs that are not part of the stack (the text after the note).
    let mut best: Option<(f64, f64)> = None;
    for g in glyphs.iter().copied().filter(|g| stacked(g)) {
        let (x0, x1) = glyphs.iter().copied().filter(|o| (o.y - g.y).abs() <= ROW_Y).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), o| {
            let (u, v) = x_span(o);
            (a.min(u), b.max(v))
        });
        if p.x < x0 - 1.0 || p.x > x1 + 1.0 {
            continue;
        }
        let dist = (l.baseline + g.y - p.y).abs();
        if best.is_none_or(|(d, _)| dist < d) {
            best = Some((dist, g.y));
        }
    }
    if let Some((_, y)) = best {
        let row = glyphs.iter().copied().filter(|o| (o.y - y).abs() <= ROW_Y).collect();
        return hit_sorted(l, row, p.x);
    }
    let rest = glyphs.iter().copied().filter(|g| !on_stacked_row(&glyphs, g)).collect();
    hit_sorted(l, rest, p.x)
}

/// Baseline of the stacked row above (`up`) or below the caret at `(x, caret_y)` on `l`.
/// `None` when the caret is not over a stack, or that side has no further row.
pub fn adjacent_row(l: &Line, x: f64, caret_y: f64, up: bool) -> Option<f64> {
    let glyphs = line_glyphs(l);
    let mut rows: Vec<(f64, f64, f64)> = Vec::new();
    for g in glyphs.iter().copied().filter(|g| on_stacked_row(&glyphs, g)) {
        let y = l.baseline + g.y;
        if rows.iter().any(|r| (r.0 - y).abs() <= ROW_Y) {
            continue;
        }
        let (x0, x1) =
            glyphs.iter().copied().filter(|o| (l.baseline + o.y - y).abs() <= ROW_Y).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), o| {
                let (u, v) = x_span(o);
                (a.min(u), b.max(v))
            });
        rows.push((y, x0, x1));
    }
    // The right edge belongs to the following character, so a caret parked there does not
    // count as still inside the note.
    let over = |r: &(f64, f64, f64)| x >= r.1 - 0.01 && x < r.2 - 0.01;
    let candidates = rows.iter().filter(|r| over(r)).filter(|r| if up { r.0 < caret_y - ROW_Y } else { r.0 > caret_y + ROW_Y });
    if up { candidates.map(|r| r.0).max_by(f64::total_cmp) } else { candidates.map(|r| r.0).min_by(f64::total_cmp) }
}

#[cfg(test)]
mod tests;

/// Type on a path: between the composed line (x along it, y as composed) and the frame's path
/// space, as `path_glyphs` places the glyphs.
pub struct PathTextMap {
    warp: designcraft_geom::warp::PathWarp,
    start: f64,
    /// Line y that sits on the path (the alignment), and the line's baseline.
    on: f64,
    baseline: f64,
}

impl PathTextMap {
    /// The map of `item` (type on a path) for its composed frame `ft`; None for other frames.
    pub fn new(item: &designcraft_doc::Item, ft: &FrameText) -> Option<Self> {
        let pt = item.text_frame()?.options.path.as_ref()?;
        let warp = designcraft_geom::warp::PathWarp::new(&item.path.to_bezpath(), pt.flip);
        let (on, baseline) = match ft.lines.first() {
            Some(l) => (
                match pt.align {
                    designcraft_doc::PathAlign::Baseline => l.baseline,
                    designcraft_doc::PathAlign::Ascender => l.baseline - l.ascent,
                    designcraft_doc::PathAlign::Descender => l.baseline + l.descent,
                    designcraft_doc::PathAlign::Center => l.baseline - (l.ascent - l.descent) / 2.0,
                },
                l.baseline,
            ),
            None => (0.0, 0.0),
        };
        Some(PathTextMap { warp, start: pt.start, on, baseline })
    }

    /// A point of the composed line, on the path: along it by x, across it by y.
    pub fn to_path(&self, p: Point) -> Point {
        match self.warp.at(self.start + p.x) {
            Some((q, a)) => q + designcraft_geom::Vec2::new(-a.sin(), a.cos()) * (p.y - self.on),
            None => p,
        }
    }

    /// A point of the path space, on the composed line: how far along the path its nearest point
    /// is, at the baseline (for hit testing).
    pub fn to_line(&self, p: Point) -> Point {
        Point::new(self.warp.nearest(p) - self.start, self.baseline)
    }
}

/// Each visible glyph of a type-on-a-path frame placed on the path: its outline in frame space
/// (grouped by run style).
pub fn path_glyphs(
    cs: &ComposedStory,
    ft: &FrameText,
    path: &designcraft_geom::BezPath,
    pt: &designcraft_doc::PathType,
) -> Vec<(u32, designcraft_geom::BezPath)> {
    let db = designcraft_fonts::FontDb::global();
    let warp = designcraft_geom::warp::PathWarp::new(path, pt.flip);
    let mut runs: Vec<(u32, designcraft_geom::BezPath)> = Vec::new();
    for l in &ft.lines {
        // The part of the type that sits on the path.
        let on = match pt.align {
            designcraft_doc::PathAlign::Baseline => l.baseline,
            designcraft_doc::PathAlign::Ascender => l.baseline - l.ascent,
            designcraft_doc::PathAlign::Descender => l.baseline + l.descent,
            designcraft_doc::PathAlign::Center => l.baseline - (l.ascent - l.descent) / 2.0,
        };
        for g in l.glyphs.iter().filter(|g| g.visible) {
            let outline = db.outline(&g.face, g.gid);
            if outline.elements().is_empty() {
                continue;
            }
            let style = &cs.styles[g.style as usize];
            let mid = g.x + g.adv / 2.0;
            let Some((p, a)) = warp.at(pt.start + mid) else { continue };
            let place = designcraft_geom::Affine::translate(p.to_vec2())
                * designcraft_geom::Affine::rotate(a)
                * designcraft_geom::Affine::translate((-mid, -on));
            let skew = if style.skew != 0.0 {
                designcraft_geom::Affine::new([1.0, 0.0, -style.skew.to_radians().tan(), 1.0, 0.0, 0.0])
            } else {
                designcraft_geom::Affine::IDENTITY
            };
            let shatai = style.shatai_xf(g, l.baseline).unwrap_or(designcraft_geom::Affine::IDENTITY);
            let m = place
                * shatai
                * designcraft_geom::Affine::translate((g.x, l.baseline + g.y))
                * skew
                * designcraft_geom::Affine::scale_non_uniform(g.sx, g.sy);
            let k = match runs.iter().position(|r| r.0 == g.style) {
                Some(k) => k,
                None => {
                    runs.push((g.style, designcraft_geom::BezPath::new()));
                    runs.len() - 1
                }
            };
            let bp = &mut runs[k].1;
            bp.extend((m * outline.as_ref().clone()).elements().iter().copied());
        }
    }
    runs
}

#[cfg(test)]
mod column_allocation_safety_tests {
    use super::*;

    fn frame(kind: designcraft_doc::ColumnsKind) -> FrameSpec {
        FrameSpec {
            id: ItemId(1),
            area: Rect::new(0., 0., 300., 100.),
            opts: TextFrameOptions { columns: 217, columns_kind: kind, column_width: 20., gutter: 12., ..Default::default() },
            vertical: false,
            exclusions: vec![],
            page_name: None,
            section_marker: None,
            page: None,
            grid: None,
            left_page: false,
            right_page: false,
            page_rect: None,
            outline: None,
        }
    }
    fn check(kind: designcraft_doc::ColumnsKind) {
        let mut f = frame(kind);
        // Fail safely at a small boundary before exercising hostile native options.
        assert_eq!(f.columns().len(), 216);
        f.opts.columns = u32::MAX;
        assert_eq!(f.columns().len(), 216);
        f.opts.columns = 0;
        assert_eq!(f.columns().len(), 1);
        f.opts.columns = 2;
        let expected = if kind == designcraft_doc::ColumnsKind::FixedWidth {
            vec![Rect::new(0., 0., 20., 100.), Rect::new(32., 0., 52., 100.)]
        } else {
            vec![Rect::new(0., 0., 144., 100.), Rect::new(156., 0., 300., 100.)]
        };
        assert_eq!(f.columns(), expected);
    }
    #[test]
    fn column_allocation_caps_equal_width_frame_count() {
        check(designcraft_doc::ColumnsKind::FixedNumber);
    }
    #[test]
    fn column_allocation_caps_fixed_width_frame_count() {
        check(designcraft_doc::ColumnsKind::FixedWidth);
    }
}
