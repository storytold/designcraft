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
pub mod hyphen;
mod notes;
mod overlay;
mod ruby;
pub mod shape;
pub mod table;
pub mod vars;
mod warichu;
pub mod xref;

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use designcraft_doc::{
    Align, Composer, Document, FirstBaseline, GridAlign, ItemId, ParaProps, SpanColumns, StartParagraph, Story, StoryId, TabAlign, TextFrameOptions,
    VerticalJustification, WrapMode, story,
};
use designcraft_fonts::{FontDb, ScopedFonts};
use designcraft_geom::{Point, Rect};

use crate::breaker::{Break, Spacing};
pub use crate::cache::Cache;
use crate::notes::Notes;
pub use crate::notes::{find_note, hit_note, note_caret};
use crate::shape::{Glyph, StyleTable, SubstCtx};
pub use crate::table::{PlacedCell, StrokeSeg, TableFrag, cell_caret, find_cell, hit_cell};

/// Character appearance shared by many glyphs (indexed from [`PlacedGlyph::style`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RunStyle {
    pub fill: String,
    pub fill_tint: f32,
    pub stroke: String,
    pub stroke_tint: f32,
    pub stroke_weight: f64,
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
    /// Ruby over the run, and kenten (emphasis dots).
    pub ruby: Option<String>,
    pub kenten: bool,
    pub kenten_character: String,
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
    /// Control characters (tabs, breaks, markers' carriers) are not drawn.
    pub visible: bool,
    /// Stays upright in vertical frames (CJK ideographs, kana, hangul, full-width forms).
    pub upright: bool,
    /// Tate-chu-yoko placement (see [`shape::Glyph::tcy`]).
    pub tcy: Option<[f64; 3]>,
    /// Set right to left (an odd bidi level): the caret before it is at its right edge.
    pub rtl: bool,
}

impl PlacedGlyph {
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
}

/// A paragraph rule or shading rectangle in frame inner space.
#[derive(Clone, Debug, PartialEq)]
pub struct Deco {
    pub rect: Rect,
    pub color: String,
    pub tint: f32,
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
    pub rect: Rect,
    pub mode: WrapMode,
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
    pub page_name: Option<String>,
    /// Absolute document page the frame is on (None on parent pages).
    pub page: Option<usize>,
    /// Baseline grid in inner space: (first grid line y, increment).
    pub grid: Option<(f64, f64)>,
    /// The frame is on a left page (for towards/away-from-spine alignment).
    pub left_page: bool,
    /// The page and its margins in the frame's inner space (custom anchored objects).
    pub page_rect: Option<(Rect, Rect)>,
}

impl FrameSpec {
    pub fn columns(&self) -> Vec<Rect> {
        let a = self.area;
        let n = self.opts.columns.max(1);
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

/// Composition inputs that are not in the story itself.
#[derive(Clone, Debug, Default)]
pub struct ComposeOptions {
    /// Page name for page-number markers when the story is composed for a specific page (parent items).
    pub page_name: Option<String>,
    /// Absolute page for text variables when composed for a specific page (parent items).
    pub page: Option<usize>,
    /// Running-header index (needed when the story contains running-header variables).
    pub running: Option<std::sync::Arc<vars::RunningIndex>>,
    /// Generated text before the first paragraph (a footnote's number and separator).
    pub label: Option<String>,
    /// Anchor pages for cross-references (needed when the story contains cross-references).
    pub xrefs: Option<Arc<xref::XrefIndex>>,
}

/// Build the frame specs of a story from the document (geometry, wrap, page names, grid).
pub fn frame_specs(doc: &Document, sid: StoryId) -> Vec<FrameSpec> {
    let Some(st) = doc.story(sid) else { return vec![] };
    let mut out = Vec::with_capacity(st.frames.len());
    for &fid in &st.frames {
        let Some(loc) = doc.find(fid) else { continue };
        let Some(item) = doc.item_at(&loc) else { continue };
        let Some(tf) = item.text_frame() else { continue };
        let xf = doc.parent_xf(&loc) * item.xf;
        let inv = xf.inverse();
        let mut exclusions = Vec::new();
        let spread = doc.spread(loc.spread);
        if !tf.options.ignore_wrap
            && let Some(sp) = spread
        {
            for other in &sp.items {
                if other.id == item.id || other.wrap.mode == WrapMode::None || other.hidden {
                    continue;
                }
                if doc.layer(other.layer).is_some_and(|l| !l.visible) {
                    continue;
                }
                let o = other.wrap.offsets;
                let b = other.bounds();
                let r = Rect::new(b.x0 - o[1], b.y0 - o[0], b.x1 + o[3], b.y1 + o[2]);
                let inner = inv.transform_rect_bbox(r);
                exclusions.push(Exclusion { rect: inner, mode: other.wrap.mode });
            }
        }
        let (page_name, page, left_page) = match (loc.spread, spread) {
            (designcraft_doc::SpreadRef::Doc(si), Some(sp)) => {
                let c = item.bounds().center();
                let pi = sp.page_at_x(c.x).unwrap_or(0);
                let abs = doc.first_page_of_spread(si) + pi;
                (Some(doc.page_name(abs)), Some(abs), sp.pages.get(pi).is_some_and(|p| p.side == designcraft_doc::PageSide::Left))
            }
            (designcraft_doc::SpreadRef::Parent(pi), Some(sp)) => {
                let prefix = sp.parent.as_ref().map(|p| p.prefix.clone()).unwrap_or_else(|| "A".into());
                let _ = pi;
                (Some(prefix), None, false)
            }
            _ => (None, None, false),
        };
        // The page under the frame and its margins, in inner space (bounding boxes).
        let page_rect = match (loc.spread, spread) {
            (_, Some(sp)) => sp
                .page_at_x(item.bounds().center().x)
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
                    path: None,
                    ..tf.options.clone()
                };
                (Rect::new(0.0, 0.0, (len - pt.start).max(1.0), size * 1.6), opts)
            }
            None => (item.text_area(), tf.options.clone()),
        };
        // Vertical type: composed in the turned box; wraps and page rects turned with it.
        let vertical = doc.frame_vertical(item);
        let (area, exclusions, grid, page_rect) = if vertical {
            let v = designcraft_doc::vertical_text_xf(area).inverse();
            let ex = exclusions.into_iter().map(|e| Exclusion { rect: v.transform_rect_bbox(e.rect), ..e }).collect();
            let pr = page_rect.map(|(a, b)| (v.transform_rect_bbox(a), v.transform_rect_bbox(b)));
            (Rect::new(0.0, 0.0, area.height(), area.width()), ex, None, pr)
        } else {
            (area, exclusions, grid, page_rect)
        };
        out.push(FrameSpec {
            id: fid,
            area,
            opts,
            vertical,
            exclusions: if tf.options.path.is_some() { vec![] } else { exclusions },
            page_name,
            page,
            grid,
            left_page,
            page_rect,
        });
    }
    out
}

/// Compose a story of `doc` through its frames.
pub fn compose_story(doc: &Document, sid: StoryId, opts: &ComposeOptions) -> ComposedStory {
    let specs = frame_specs(doc, sid);
    match doc.story(sid) {
        Some(st) => compose(doc, st, &specs, opts),
        None => ComposedStory { story: sid, ..Default::default() },
    }
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
        pending: 0.0,
        pi: 0,
        band: Band::default(),
        limits: Rc::default(),
    };
    let para_ranges = story.para_ranges();
    let np = para_ranges.len();
    let hyph_exceptions = doc.hyphenation_exception_map();
    let mut list_counter: u32 = 0;
    // Named lists number independently of layout: one pass up front.
    let named_numbers: Vec<Option<u32>> = {
        let mut counters: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        story
            .paras
            .iter()
            .map(|p| {
                let (pp, _) = doc.styles.resolve_para(p);
                if pp.list_type != designcraft_doc::ListType::Numbers || pp.list_name.is_empty() {
                    return None;
                }
                let c = counters.entry(pp.list_name.clone()).or_insert_with(|| doc.list_start(story.id, &pp.list_name));
                *c = pp.start_at.map_or(*c + 1, |s| s.max(1));
                Some(*c)
            })
            .collect()
    };
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
    let balance_budget = 20 * story.paras.iter().filter(|p| matches!(doc.styles.resolve_para(p).0.span_columns, SpanColumns::Span(_))).count();
    // Paragraphs set across columns (vertical justification moves their frames' lines together).
    let mut span_paras = vec![false; np];
    'paras: while pi < np {
        let prange = para_ranges[pi].clone();
        cur.pi = pi;
        cur.limits = Rc::clone(&limits);
        let snap = Snapshot::take(&out, &cur, list_counter);
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
        let pf = &story.paras[pi];
        let (pp, base_chars) = doc.styles.resolve_para(pf);
        // A break character that ends the previous paragraph sends this one on, like a start option.
        let after_break = pi
            .checked_sub(1)
            .and_then(|p| para_ranges.get(p))
            .and_then(|r| story.text.get(r.clone()))
            .and_then(|t| t.chars().next_back())
            .and_then(break_start);
        if let Some(t) = story.para_table(pi) {
            if cur.last_baseline.is_some() {
                cur.start(after_break.unwrap_or(pp.start_paragraph), &cols, frames, doc);
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
            list_counter = 0;
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
            section_marker: None,
            vars: var_values,
            hidden_conditions: doc.conditions.iter().filter(|c| !c.visible).map(|c| c.name.clone()).collect(),
            vertical: cur_frame.is_some_and(|f| f.vertical),
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
        );
        if !pp.nested_line_styles.is_empty() && cur.fi < frames.len() {
            // Nested line styles: find where the first lines end in this column, restyle them, and
            // look again (the style changes the widths) until the lines settle.
            let cols_here = &cols[cur.fi];
            let col = span_rect(cols_here, cols_here[cur.col.min(cols_here.len() - 1)], pp.span_columns);
            let spacing = spacing_for(&pp, base_chars.size);
            let mut lines: Vec<(std::ops::Range<usize>, String)> = Vec::new();
            for _ in 0..3 {
                let width = |j: usize| (col.width() - pp.left_indent - pp.right_indent - if j == 0 { pp.first_line_indent } else { 0.0 }).max(1.0);
                // The same spacing, hyphenation and breaker as the layout below.
                let mut gl = sp.glyphs.clone();
                apply_desired_spacing(&mut gl, &pp);
                let hy = hyphenation_points(&story.text, &gl, &pp, &hyph_exceptions, &foreign_ranges(doc, story, prange.clone(), &base_chars));
                let breaks = if pp.composer == Composer::SingleLine || gl.iter().any(|g| g.ch == '\t') || gl.len() > 4000 {
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
                );
            }
        }
        // List labels take the default super/subscript settings.
        let label_env = shape::TypeEnv { adv: Default::default(), ..env };
        match pp.list_type {
            designcraft_doc::ListType::Numbers if !pp.list_name.is_empty() => {
                // A named list: carries on past other paragraphs (and from earlier stories).
                let n = named_numbers.get(pi).copied().flatten().unwrap_or(1);
                let label = format!("{}.{}", pp.number_style.format(n), pp.list_separator);
                prepend_label(db, &mut sp.glyphs, &label, prange.start, &base_chars, label_env, &mut table);
            }
            designcraft_doc::ListType::Numbers => {
                list_counter = pp.start_at.map_or(list_counter + 1, |s| s.max(1));
                let label = format!("{}.{}", pp.number_style.format(list_counter), pp.list_separator);
                prepend_label(db, &mut sp.glyphs, &label, prange.start, &base_chars, label_env, &mut table);
            }
            designcraft_doc::ListType::Bullets => {
                let label = format!("{}{}", pp.bullet_char, pp.list_separator);
                prepend_label(db, &mut sp.glyphs, &label, prange.start, &base_chars, label_env, &mut table);
            }
            designcraft_doc::ListType::None => list_counter = 0,
        }
        if pi == 0
            && let Some(label) = &opts.label
        {
            prepend_label(db, &mut sp.glyphs, label, prange.start, &base_chars, label_env, &mut table);
        }
        let mut glyphs = sp.glyphs;
        let bidi_text = bidi::paragraph_text(&mut glyphs, &story.text);
        let bidi_info = unicode_bidi::BidiInfo::new(
            &bidi_text,
            Some(if pp.direction == designcraft_doc::TextDirection::RightToLeft { unicode_bidi::Level::rtl() } else { unicode_bidi::Level::ltr() }),
        );
        bidi::resolve_mirroring(&mut glyphs, &bidi_info);
        apply_desired_spacing(&mut glyphs, &pp);
        let hyph_after = hyphenation_points(&story.text, &glyphs, &pp, &hyph_exceptions, &foreign_ranges(doc, story, prange.clone(), &base_chars));
        let base_size = base_chars.size;
        let base_leading = match base_chars.leading {
            designcraft_doc::Leading::Auto => base_size * pp.auto_leading,
            designcraft_doc::Leading::Points(v) => v,
        };
        let spacing = spacing_for(&pp, base_size);
        // A paragraph set across the columns moves on to the next frame where others move on to
        // the next column.
        let spanning = matches!(pp.span_columns, SpanColumns::Span(n) if n != 1);
        span_paras[pi] = spanning;
        let next_column = |cur: &mut Cursor| if spanning { cur.next_frame() } else { cur.next_column(&cols) };
        // A paragraph start or a break character; Next Column while a paragraph spans columns is
        // the next frame, as above.
        let begin_at = |cur: &mut Cursor, start: StartParagraph| match start {
            StartParagraph::NextColumn => next_column(cur),
            other => cur.start(other, &cols, frames, doc),
        };
        // Paragraph start options.
        if let Some(start) = after_break
            && cur.last_baseline.is_some()
        {
            begin_at(&mut cur, start);
        }
        if force_col[pi] {
            next_column(&mut cur);
        }
        if cur.last_baseline.is_some() {
            begin_at(&mut cur, pp.start_paragraph);
        }
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
            } else if cur.col != 0 {
                cur.col = 0;
                cur.resume();
            }
        }
        if cur.last_baseline.is_some() {
            cur.pending += pp.space_before;
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
        loop {
            if cur.fi >= frames.len() {
                if let Some(j) = trial_failed(&mut trial, &mut limits, balance_runs < balance_budget) {
                    balance_runs += 1;
                    rewind(j, &snaps, &note_snaps, &mut notes, &mut out, &mut cur, &mut list_counter, &mut force_col, &mut line_cap);
                    pi = j;
                    continue 'paras;
                }
                out.overset_at = Some(glyphs.get(g0).map(|g| g.byte).unwrap_or(prange.start));
                // The part of the paragraph that fits keeps its shading and border.
                para_box_decos(&mut out, pi, &pp);
                break 'paras;
            }
            let f = &frames[cur.fi];
            let base = cols[cur.fi][cur.col.min(cols[cur.fi].len() - 1)];
            let col = cur.clip(base);
            // A spanning paragraph isn't held to the balanced bottom of the columns above it.
            let col = if spanning { span_rect(&cols[cur.fi], Rect::new(col.x0, col.y0, col.x1, base.y1), pp.span_columns) } else { col };
            // Estimate slots for the breaker with the paragraph's base leading.
            let est_first = cur.next_baseline(f, col, base_leading, base_chars.size * 0.75, &pp);
            let slots = estimate_slots(f, col, est_first, base_leading, base_chars.size, &glyphs[g0..], &pp, line_no);
            let width = |j: usize| -> f64 {
                let (x0, x1) = slots.get(j).copied().unwrap_or((col.x0, col.x1));
                let ind = pp.left_indent + pp.right_indent + if line_no + j == 0 { pp.first_line_indent } else { 0.0 };
                (x1 - x0 - ind).max(1.0)
            };
            let rest = &glyphs[g0..];
            let rest_h = &hyph_after[g0..];
            let breaks: Vec<Break> = if pp.composer == Composer::SingleLine || has_tabs || rest.len() > 4000 {
                // Where each line starts relative to the tab origin, as `layout_line` places it.
                let tab = |j: usize, x: f64, i: usize| {
                    let x0 = slots.get(j).map_or(col.x0, |s| s.0);
                    let ind = pp.left_indent + if line_no + j == 0 { pp.first_line_indent } else { 0.0 };
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
                let (asc, desc, lead) = line_metrics(line_glyphs, &glyphs, s, base_leading, base_chars.size, db, &base_chars);
                let reference = cjk_line_reference(line_glyphs);
                let mut baseline = cur.next_baseline(f, col, lead, asc, &pp);
                if cur.last_baseline.is_some() {
                    baseline += cur.last_reference - reference;
                }
                // Baseline grid.
                if let Some((g_start, inc)) = f.grid
                    && (pp.grid_align == GridAlign::AllLines || (pp.grid_align == GridAlign::FirstLineOnly && line_no == 0))
                    && inc > 0.0
                {
                    let n = ((baseline - g_start) / inc - 1e-6).ceil();
                    baseline = g_start + n * inc;
                }
                // Wrap: push the line down until a slot exists.
                let (mut x0, mut x1) = (col.x0, col.x1);
                if !f.exclusions.is_empty() {
                    let mut tries = 0;
                    loop {
                        match free_slot(f, col, baseline - asc, baseline + desc, base_size) {
                            Some((a, b)) => {
                                x0 = a;
                                x1 = b;
                                break;
                            }
                            None => {
                                baseline += 1.0;
                                tries += 1;
                                if baseline > col.y1 || tries > 4000 {
                                    break;
                                }
                            }
                        }
                    }
                }
                let capped = line_cap[pi] == Some(line_no) && line_no > col_first_line;
                // Footnotes referenced on this line need room at the bottom of the column too
                // (a line at the top of a column is set anyway).
                let line_notes = if notes.active() { notes.refs_in(line_glyphs) } else { Vec::new() };
                let col_w = cols[cur.fi][cur.col.min(cols[cur.fi].len() - 1)].width();
                let reserve = notes.reserve(doc, cur.fi, cur.col, &line_notes, col_w, f, opts);
                let reserve = if cur.last_baseline.is_none() { notes.reserve(doc, cur.fi, cur.col, &[], col_w, f, opts) } else { reserve };
                let fits = baseline + desc <= col.y1 - reserve + 0.01 && !capped;
                if !fits {
                    if !capped {
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
                    moved = true;
                    break;
                }
                let ind_l = pp.left_indent + if line_no == 0 { pp.first_line_indent } else { 0.0 };
                let lx0 = x0 + ind_l;
                let lx1 = x1 - pp.right_indent;
                let last = k + 1 == breaks.len();
                // The break character that ends this line, if any (it is the line's last glyph).
                let brk = if b.forced && e > s {
                    e.checked_sub(1).and_then(|i| glyphs.get(i)).map(|g| g.ch).filter(|&c| breaker::is_forced(c))
                } else {
                    None
                };
                // A column, frame or page break ends the line as a paragraph's last line (last-line
                // alignment); a justified line ended by a forced line break stays justified.
                let ends_para = last || matches!(brk, Some(story::COLUMN_BREAK | story::FRAME_BREAK | story::PAGE_BREAK));
                let forced_mid = brk == Some(story::FORCED_LINE_BREAK) && !last;
                let (mut placed, end_x, ratio) =
                    layout_line(&glyphs, s, e, b.hyphen, lx0, lx1, col.x0, &pp, &spacing, ends_para, forced_mid, f.left_page, &bidi_info);
                // Warichu runs before ruby so a reading is placed over the stacked note.
                let end_x = end_x + warichu::place(&styles_tab, &mut placed);
                ruby::annotate(db, &styles_tab, &mut placed, doc.settings.glyph_fallback);
                let range_end = if last { prange.end } else { glyphs.get(g0 + b.next).map(|g| g.byte).unwrap_or(prange.end) };
                let range_start = glyphs.get(s).map(|g| g.byte).unwrap_or(prange.start).min(range_end);
                let range_start = if line_no == 0 { prange.start } else { range_start };
                let ft = &mut out.frames[cur.fi];
                if line_no == 0 {
                    first_line_rect = Some((cur.fi, baseline, asc, x0));
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
                });
                for k in line_notes {
                    notes.place(doc, k, cur.fi, cur.col, col_w, f, opts);
                }
                if !sub_objects.is_empty() {
                    let ft = &mut out.frames[cur.fi];
                    let li = ft.lines.len() - 1;
                    let text_ascent = line_glyphs.iter().filter(|g| g.ch != designcraft_doc::OBJECT_MARK).map(|g| g.ascent).fold(0.0, f64::max);
                    let text_ascent = if text_ascent > 0.0 { text_ascent } else { base_chars.size * 0.75 };
                    for g in ft.lines[li].glyphs.iter().filter(|g| g.len > 0) {
                        if let Some(o) = sub_objects.get(&g.byte) {
                            ft.objects.push(PlacedObject { index: o.index, origin: Point::ZERO, size: (o.w, o.h), line: li, x: g.x, text_ascent });
                        }
                    }
                }
                cur.last_baseline = Some(baseline);
                cur.last_reference = reference;
                cur.last_descent = desc;
                cur.pending = 0.0;
                line_no += 1;
                // A column / frame / page break inside the paragraph: the rest of it continues in the
                // new column/frame/page, re-broken there. (A break that ends the paragraph moves the
                // next paragraph instead.)
                if b.forced
                    && k + 1 < breaks.len()
                    && let Some(start) = brk.and_then(break_start)
                {
                    begin_at(&mut cur, start);
                    col_first_line = line_no;
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
                pending: pp.space_after,
            };
            cur.band = Band { para: pi + 1, line0: ft.lines.len(), above: Some(above) };
        }
        // Rules and shading for the paragraph.
        if let Some((fi, bl, asc, _)) = first_line_rect {
            let ft = &mut out.frames[fi];
            if pp.rule_above.on {
                let r = &pp.rule_above;
                let y = bl - asc - r.offset;
                let col = ft
                    .lines
                    .iter()
                    .find(|l| l.para == pi && l.first_in_para)
                    .map(|l| if r.column_width { (l.x0, l.x1) } else { (l.x0, l.end_x) })
                    .unwrap_or((0.0, 0.0));
                ft.decos.push(Deco {
                    rect: Rect::new(col.0 + r.left_indent, y - r.weight, col.1 - r.right_indent, y),
                    color: r.color.clone(),
                    tint: r.tint,
                });
            }
        }
        para_box_decos(&mut out, pi, &pp);
        if pp.rule_below.on
            && let Some(ft) = out.frames.get_mut(cur.fi.min(frames.len().saturating_sub(1)))
            && let Some(l) = ft.lines.iter().rev().find(|l| l.para == pi)
        {
            let r = &pp.rule_below;
            let y = l.baseline + r.offset;
            let (x0, x1) = if r.column_width { (l.x0, l.x1) } else { (l.x0, l.end_x) };
            ft.decos.push(Deco { rect: Rect::new(x0 + r.left_indent, y, x1 - r.right_indent, y + r.weight), color: r.color.clone(), tint: r.tint });
        }
        cur.pending += pp.space_after;
        pi += 1;
    }
    notes.finish(doc, story, frames, &cols, &mut out);
    // Ranges, content heights and vertical justification.
    for (fi, ft) in out.frames.iter_mut().enumerate() {
        let f = &frames[fi];
        if let (Some(a), Some(z)) = (ft.lines.first(), ft.lines.last()) {
            ft.range = a.range.start..z.range.end;
            ft.content_height = z.baseline + z.descent - f.area.y0;
        } else {
            let p = ft_prev_end(&out.overset_at, story.text.len());
            ft.range = p..p;
        }
        let before: Vec<f64> = ft.tables.iter().map(|t| ft.lines.get(t.line).map_or(0.0, |l| l.baseline)).collect();
        let spanned = ft.columns.len() > 1 && ft.lines.iter().any(|l| span_paras.get(l.para) == Some(&true));
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
                        let (rfx, rfy) = frac(*ref_point);
                        let (ofx, ofy) = frac(*object_point);
                        let rx = area(*x_relative).map_or(o.x, |a| a.x0 + a.width() * rfx);
                        let ry = area(*y_relative).map_or(l.baseline, |a| a.y0 + a.height() * rfy);
                        let mut y = ry + y_offset - h * ofy;
                        if *keep_within_column {
                            y = y.clamp(col.y0, (col.y1 - h).max(col.y0));
                        }
                        Point::new(rx + x_offset - w * ofx, y)
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
    // Column rules, once vertical justification has put the lines in place.
    for (ft, f) in out.frames.iter_mut().zip(frames) {
        if f.opts.column_rule {
            let rects = column_rule_rects(&ft.columns, &ft.lines, &f.opts);
            let tint = if f.opts.column_rule_tint.is_finite() { f.opts.column_rule_tint.clamp(0.0, 1.0) } else { 1.0 };
            ft.decos.extend(rects.into_iter().map(|rect| Deco { rect, color: f.opts.column_rule_color.clone(), tint }));
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

/// Paragraph shading and border, per column the paragraph sits in (a split paragraph gets one box
/// per part; the border's top edge on the first part, its bottom edge on the last).
fn para_box_decos(out: &mut ComposedStory, pi: usize, pp: &ParaProps) {
    if !pp.shading_on && !pp.border_on {
        return;
    }
    let mut parts: Vec<(usize, Rect)> = Vec::new();
    for (fi, ft) in out.frames.iter().enumerate() {
        let mut cols_seen: Vec<u32> = ft.lines.iter().filter(|l| l.para == pi).map(|l| l.column).collect();
        cols_seen.dedup();
        for c in cols_seen {
            let lines: Vec<&Line> = ft.lines.iter().filter(|l| l.para == pi && l.column == c).collect();
            if let (Some(a), Some(z)) = (lines.first(), lines.last()) {
                parts.push((fi, Rect::new(a.x0, a.baseline - a.ascent, a.x1, z.baseline + z.descent)));
            }
        }
    }
    let n = parts.len();
    for (k, (fi, r)) in parts.into_iter().enumerate() {
        let ft = &mut out.frames[fi];
        if pp.shading_on {
            let [t, l, b, rr] = pp.shading_offsets;
            ft.decos
                .insert(0, Deco { rect: Rect::new(r.x0 - l, r.y0 - t, r.x1 + rr, r.y1 + b), color: pp.shading_color.clone(), tint: pp.shading_tint });
        }
        if pp.border_on {
            let [t, l, b, rr] = pp.border_offsets;
            let o = Rect::new(r.x0 - l, r.y0 - t, r.x1 + rr, r.y1 + b);
            let [wt, wl, wb, wr] = pp.border_weights.map(|w| w.max(0.0));
            let mut edge = |rect: Rect| ft.decos.push(Deco { rect, color: pp.border_color.clone(), tint: pp.border_tint });
            if k == 0 && wt > 0.0 {
                edge(Rect::new(o.x0 - wl, o.y0 - wt, o.x1 + wr, o.y0));
            }
            if k + 1 == n && wb > 0.0 {
                edge(Rect::new(o.x0 - wl, o.y1, o.x1 + wr, o.y1 + wb));
            }
            if wl > 0.0 {
                edge(Rect::new(o.x0 - wl, o.y0, o.x0, o.y1));
            }
            if wr > 0.0 {
                edge(Rect::new(o.x1, o.y0, o.x1 + wr, o.y1));
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
        hyph_zone: if pp.align.is_justified() { 0.0 } else { pp.hyph_zone },
        optical: pp.optical_margin,
        korean_char_breaks: pp.korean_char_breaks,
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
        if g.ch == ' ' || (g.ch == '\u{3000}' && g.ideographic_space_elastic) {
            g.adv += g.space * (ws - 1.0);
        } else if !g.is_space() && g.adv > 0.0 {
            g.adv = g.adv * gs + ls * g.space;
            g.sx *= gs;
            g.dx *= gs;
        }
    }
}

/// Lengths of the composed output and the cursor before a paragraph (keep resolution re-lays
/// paragraphs from here). Only the cursor's frame and later ones can change afterwards.
struct Snapshot {
    cur: Cursor,
    list_counter: u32,
    lines: usize,
    decos: usize,
    tables: usize,
}

impl Snapshot {
    fn take(out: &ComposedStory, cur: &Cursor, list_counter: u32) -> Snapshot {
        let (lines, decos, tables) = out.frames.get(cur.fi).map_or((0, 0, 0), |f| (f.lines.len(), f.decos.len(), f.tables.len()));
        Snapshot { cur: cur.clone(), list_counter, lines, decos, tables }
    }
    fn restore(&self, out: &mut ComposedStory, cur: &mut Cursor, list_counter: &mut u32) {
        let fi = self.cur.fi;
        for (i, f) in out.frames.iter_mut().enumerate().skip(fi) {
            let (l, d, t) = if i == fi { (self.lines, self.decos, self.tables) } else { (0, 0, 0) };
            f.lines.truncate(l);
            f.decos.truncate(d);
            f.tables.truncate(t);
            f.objects.retain(|o| o.line < l);
        }
        *cur = self.cur.clone();
        *list_counter = self.list_counter;
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
    list_counter: &mut u32,
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
        let starts_here = if j == ctx.pi { ctx.started_here } else { info[j].start == ctx.here };
        (!(force_col[j] || top && starts_here)).then_some(KeepAction::Force(j))
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
    /// Space before/after waiting to be added to the next line.
    pending: f64,
    /// The paragraph being laid.
    pi: usize,
    /// The band of columns the cursor is in.
    band: Band,
    /// Column bottoms that balance bands, by band key (kept by the compose loop).
    limits: Rc<Vec<(BandKey, f64)>>,
}

/// A band's frame and first paragraph.
type BandKey = (usize, usize);

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
}

#[derive(Clone, Copy, Debug)]
struct Above {
    /// Bottom of the spanning paragraph's last line (the band's top).
    top: f64,
    baseline: f64,
    descent: f64,
    reference: f64,
    /// The spanning paragraph's space after.
    pending: f64,
}

/// Balancing the columns above a spanning paragraph: a search for the lowest column bottom at
/// which the band's text still fits its frame, re-laying the band at each trial bottom.
#[derive(Debug)]
struct Trial {
    key: BandKey,
    /// The spanning paragraph that closes the band.
    span: usize,
    /// Highest bottom known not to fit / lowest known to fit.
    lo: f64,
    hi: f64,
    tries: u32,
    /// The band is laid at `hi` for good.
    done: bool,
}

impl Trial {
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
fn trial_failed(trial: &mut Option<Trial>, limits: &mut Rc<Vec<(BandKey, f64)>>, budget_left: bool) -> Option<usize> {
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
    Some(key.1)
}

fn set_limit(limits: &mut Rc<Vec<(BandKey, f64)>>, key: BandKey, bottom: Option<f64>) {
    let v = Rc::make_mut(limits);
    v.retain(|(k, _)| *k != key);
    if let Some(b) = bottom {
        v.push((key, b));
    }
}

fn limit_of(limits: &[(BandKey, f64)], key: BandKey) -> Option<f64> {
    limits.iter().find(|(k, _)| *k == key).map(|&(_, b)| b)
}

impl Cursor {
    fn next_column(&mut self, cols: &[Vec<Rect>]) {
        self.col += 1;
        if self.fi < cols.len() && self.col >= cols[self.fi].len() {
            self.next_frame();
        } else {
            self.resume();
        }
    }
    fn next_frame(&mut self) {
        self.fi += 1;
        self.col = 0;
        self.band = Band { para: self.pi, ..Band::default() };
        self.resume();
    }
    /// Start the current column at the top of the band.
    fn resume(&mut self) {
        match self.band.above {
            Some(a) => {
                self.last_baseline = Some(a.baseline);
                self.last_descent = a.descent;
                self.last_reference = a.reference;
                self.pending = a.pending;
            }
            None => {
                self.last_baseline = None;
                self.pending = 0.0;
            }
        }
    }
    fn band_key(&self) -> BandKey {
        (self.fi, self.band.para)
    }
    /// Column `col` of the current frame limited to the band: below the spanning paragraph above
    /// it and, while balancing, above the trial bottom.
    fn clip(&self, col: Rect) -> Rect {
        let y0 = self.band.above.map_or(col.y0, |a| a.top.max(col.y0).min(col.y1.max(col.y0)));
        let y1 = limit_of(&self.limits, self.band_key()).map_or(col.y1, |b| b.min(col.y1).max(y0));
        Rect::new(col.x0, y0, col.x1, y1)
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
    /// Baseline for the next line with leading `lead` and ascent `asc`.
    fn next_baseline(&self, f: &FrameSpec, col: Rect, lead: f64, asc: f64, _pp: &ParaProps) -> f64 {
        match self.last_baseline {
            Some(b) => b + lead + self.pending,
            None => {
                let off = match f.opts.first_baseline {
                    FirstBaseline::Ascent => asc,
                    FirstBaseline::CapHeight => asc * 0.72,
                    FirstBaseline::XHeight => asc * 0.5,
                    FirstBaseline::Leading => lead,
                    FirstBaseline::Fixed => 0.0,
                };
                col.y0 + off.max(f.opts.first_baseline_min)
            }
        }
    }
}

/// Em box relative to the baseline. Font ascender/descender proportions locate it;
/// the box's total extent is the em, rather than the font's optional line gap.
fn cjk_em_box(g: &Glyph) -> (f64, f64) {
    let height = g.face.units_per_em() * g.sy.abs();
    let top = -height * g.ascent / (g.ascent + g.descent).max(1e-9);
    (top, top + height)
}
fn cjk_alignment_shift(g: &Glyph, reference: &Glyph) -> f64 {
    use designcraft_doc::cjk::CharacterAlignment as A;
    let (t, b) = cjk_em_box(g);
    let (rt, rb) = cjk_em_box(reference);
    match g.character_alignment {
        A::Baseline => 0.0,
        A::EmTop => t - rt,
        A::EmCenter => (t + b - rt - rb) / 2.0,
        A::EmBottom => b - rb,
        A::IcfTop => reference.ascent - g.ascent,
        A::IcfBottom => g.descent - reference.descent,
    }
}
fn cjk_line_reference(line: &[Glyph]) -> f64 {
    use designcraft_doc::cjk::LeadingModel as L;
    let Some(g) = line.iter().max_by(|a, b| a.size.total_cmp(&b.size)) else { return 0.0 };
    let (top, bottom) = cjk_em_box(g);
    match g.leading_model {
        L::Roman => 0.0,
        L::AkiBelow => bottom,
        L::AkiAbove => top,
        L::Center | L::CenterDown => (top + bottom) / 2.0,
    }
}

fn line_metrics(
    line: &[Glyph],
    all: &[Glyph],
    s: usize,
    base_leading: f64,
    base_size: f64,
    db: &ScopedFonts<'_>,
    base: &designcraft_doc::CharProps,
) -> (f64, f64, f64) {
    let src: &[Glyph] = if line.is_empty() { all.get(s..(s + 1).min(all.len())).unwrap_or(&[]) } else { line };
    if src.is_empty() {
        let face = db.face(&base.font_family, &base.font_style);
        let k = base_size / face.upem;
        return (face.ascent * k, face.descent * k, base_leading);
    }
    let mut asc: f64 = 0.0;
    let mut desc: f64 = 0.0;
    let mut lead: f64 = 0.0;
    let reference = src.iter().max_by(|a, b| a.size.total_cmp(&b.size));
    for g in src {
        let shift = g.shift + reference.map_or(0.0, |r| cjk_alignment_shift(g, r));
        asc = asc.max(g.ascent + shift.max(0.0));
        desc = desc.max(g.descent - shift.min(0.0));
        lead = lead.max(g.leading);
    }
    (asc, desc, lead)
}

/// Widest free horizontal interval of `col` in the band, or None if blocked.
fn free_slot(f: &FrameSpec, col: Rect, y0: f64, y1: f64, size: f64) -> Option<(f64, f64)> {
    let mut free = vec![(col.x0, col.x1)];
    for ex in &f.exclusions {
        let r = ex.rect;
        let overlaps_band = r.y0 < y1 && r.y1 > y0;
        match ex.mode {
            WrapMode::JumpToNextColumn if y1 > r.y0 && r.x0 < col.x1 && r.x1 > col.x0 => return None,
            WrapMode::JumpObject if overlaps_band && r.x0 < col.x1 && r.x1 > col.x0 => return None,
            WrapMode::BoundingBox | WrapMode::Contour if overlaps_band => {
                let mut next = Vec::new();
                for (a, b) in free {
                    if r.x1 <= a || r.x0 >= b {
                        next.push((a, b));
                        continue;
                    }
                    if r.x0 > a {
                        next.push((a, r.x0));
                    }
                    if r.x1 < b {
                        next.push((r.x1, b));
                    }
                }
                free = next;
            }
            _ => {}
        }
    }
    free.into_iter().filter(|(a, b)| b - a >= size * 1.5).max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))
}

#[allow(clippy::too_many_arguments)]
fn estimate_slots(f: &FrameSpec, col: Rect, first: f64, lead: f64, size: f64, glyphs: &[Glyph], pp: &ParaProps, line_no: usize) -> Vec<(f64, f64)> {
    let _ = (pp, line_no);
    if f.exclusions.is_empty() {
        return vec![];
    }
    // Rough upper bound on lines: total advance / column width × 2.
    let total: f64 = glyphs.iter().map(|g| g.adv).sum();
    let n = ((total / (col.width().max(10.0) * 0.5)).ceil() as usize + 2).min(2000);
    let mut v = Vec::with_capacity(n);
    let mut b = first;
    for _ in 0..n {
        let mut tries = 0;
        let slot = loop {
            if let Some(s) = free_slot(f, col, b - size * 0.8, b + size * 0.25, size) {
                break s;
            }
            b += 1.0;
            tries += 1;
            if b > col.y1 || tries > 4000 {
                break (col.x0, col.x1);
            }
        };
        v.push(slot);
        b += lead;
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
    let spaces: Vec<usize> = line.iter().enumerate().skip(seg).filter(|(_, g)| g.is_space() && !g.no_break).map(|(i, _)| i).collect();
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
    let squeeze_last = align.is_justified() && !justify_this && seg_justifies && extra < 0.0 && !spaces.is_empty();
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
    if (justify_this || squeeze_last) && !spaces.is_empty() {
        let rebased: Vec<usize> = spaces.iter().map(|&i| i - seg).collect();
        let (seg_line, seg_add, seg_scale) =
            (line.get(seg..).unwrap_or(&[]), add.get_mut(seg..).unwrap_or(&mut []), scale.get_mut(seg..).unwrap_or(&mut []));
        distribute(seg_line, &rebased, extra, sp, seg_add, seg_scale);
    } else if justify_this && spaces.is_empty() && line.len() > seg + 1 && !last {
        // Single word: Single Word Justification.
        match pp.single_word_justify {
            Align::FullyJustified => {
                let gaps: Vec<_> = line
                    .iter()
                    .enumerate()
                    .take(line.len() - 1)
                    .skip(seg)
                    .filter(|(_, g)| !g.locked_advance || g.break_after != Some(false))
                    .map(|(i, _)| i)
                    .collect();
                if !gaps.is_empty() {
                    let per = extra / gaps.len() as f64;
                    for i in gaps {
                        add[i] = per;
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
        if g.ch == ' ' && g.space > 0.0 {
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
        out.push(PlacedGlyph { gid, x: at - 0.2, y: -g.shift, adv: 0.0, sx: g.sx * k, len: 0, upright: false, tcy: None, ..place(g, at) });
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
                visible: true,
                upright: false,
                tcy: None,
                rtl: false,
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
            if g.ch != ' ' && !(g.ch == '\u{3000}' && g.ideographic_space_elastic) {
                0.0
            } else if stretch {
                g.space * (sp.word_max - sp.word_desired).max(0.0)
            } else {
                g.space * (sp.word_desired - sp.word_min).max(0.0)
            }
        })
        .collect();
    // Letter gaps: between visible glyphs (not after the line's last glyph).
    let last_box = line.iter().rposition(|g| !g.is_space() && g.adv > 0.0).unwrap_or(0);
    let is_box = |i: usize, g: &Glyph| i < last_box && !g.is_space() && g.adv > 0.0 && (!g.locked_advance || g.break_after != Some(false));
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
            if g.is_space() || g.adv <= 0.0 || g.locked_advance {
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
    for (k, &i) in spaces.iter().enumerate() {
        let share = if yw > 1e-9 { tw * word[k] / yw } else { 0.0 };
        // Left over beyond every limit: shared equally by the word spaces.
        add[i] += sign * (share + rem / spaces.len() as f64);
    }
    if yl > 1e-9 && tl > 0.0 {
        for (i, l) in letter.iter().enumerate() {
            add[i] += sign * tl * l / yl;
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
        visible,
        upright: g.upright,
        tcy: g.tcy,
        rtl: false,
    }
}

fn prepend_label(
    db: &ScopedFonts<'_>,
    glyphs: &mut Vec<Glyph>,
    label: &str,
    at: usize,
    base: &designcraft_doc::CharProps,
    env: shape::TypeEnv,
    table: &mut StyleTable<'_>,
) {
    // Shape the label as a tiny standalone story so it uses the paragraph's base character style.
    let mut tmp = Story::new(StoryId(0));
    tmp.insert(0, label);
    let styles = designcraft_doc::Styles::default();
    let shaped = shape::shape_para(db, &styles, &tmp, 0, 0..label.len(), base, env, &SubstCtx::default(), table, &[], &[], &[]);
    let mut pre: Vec<Glyph> = shaped
        .glyphs
        .into_iter()
        .map(|mut g| {
            g.byte = at;
            g.len = 0;
            g
        })
        .collect();
    pre.append(glyphs);
    *glyphs = pre;
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
    let lim = hyphen::Limits {
        min_word: pp.hyph_min_word as usize,
        after_first: pp.hyph_after_first as usize,
        before_last: pp.hyph_before_last as usize,
        capitalized: pp.hyph_capitalized,
    };
    // Words repeat a lot: remember their points per set of limits.
    static CACHE: Mutex<Vec<(LimitsKey, HashMap<(hyphen::Lang, Box<str>), Box<[usize]>>)>> = Mutex::new(Vec::new());
    let key = (lim.min_word, lim.after_first, lim.before_last, lim.capitalized);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
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
            VerticalJustification::Justify if idx.len() > 1 => {
                let per = space / (idx.len() - 1) as f64;
                for (k, &i) in idx.iter().enumerate() {
                    ft.lines[i].baseline += per * k as f64;
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

/// Story byte nearest to point `p` (frame inner space) in frame `fi`.
pub fn hit(cs: &ComposedStory, fi: usize, p: Point) -> Option<usize> {
    let ft = cs.frames.get(fi)?;
    if ft.lines.is_empty() {
        return Some(ft.range.start);
    }
    // Closest line vertically (within its column).
    let l = ft
        .lines
        .iter()
        .filter(|l| p.x >= l.x0 - 20.0 && p.x <= l.x1 + 20.0 || ft.columns.len() <= 1)
        .min_by(|a, b| line_dist(a, p.y).total_cmp(&line_dist(b, p.y)))
        .or_else(|| ft.lines.first())?;
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
    if up { candidates.map(|r| r.0).max_by(|a, b| a.total_cmp(b)) } else { candidates.map(|r| r.0).min_by(|a, b| a.total_cmp(b)) }
}

#[cfg(test)]
mod tests;

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
            let m =
                place * designcraft_geom::Affine::translate((g.x, l.baseline + g.y)) * skew * designcraft_geom::Affine::scale_non_uniform(g.sx, g.sy);
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
