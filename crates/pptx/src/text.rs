//! Text frames as text boxes.
//!
//! [`LineBreaks::Keep`]: a frame's composed lines are cut into boxes where the column, the line
//! slot (a wrap) or a gap changes; each box holds its lines as paragraphs with line breaks. Line
//! starts come from indents, widths from character spacing on the runs (PowerPoint has no word
//! spacing, so a justified space is a run of its own), baselines from percentage line spacing and
//! space before.
//!
//! [`LineBreaks::Reflow`]: one box per frame, native columns, paragraph attributes from the
//! styles.

use designcraft_color::swatch::resolve_gradient;
use designcraft_compose::{ComposedStory, FrameText, Line, PlacedGlyph, RunStyle};
use designcraft_doc::{Item, TextFrame};
use designcraft_geom::{Affine, Rect};

use crate::LineBreaks;
use crate::slide::{Exporter, XfrmBox, box_of};

/// PowerPoint's single line is 1.2 em for every font; percentage spacing scales it.
const SINGLE: f64 = 1.2;
/// Where PowerPoint puts a line's baseline in its line box (fraction from the top).
const BASELINE_IN_LINE: f64 = 0.75;
/// Largest drift (points) a run's character spacing may leave before a new run starts.
const TOLERANCE: f64 = 0.3;

/// Private-use characters DesignCraft stores for markers (page numbers, section markers, breaks).
fn is_marker(c: char) -> bool {
    ('\u{E000}'..='\u{E1FF}').contains(&c)
}

/// One piece of a line: a cluster's text and where the composer put it.
#[derive(Clone, Debug)]
struct Unit {
    text: String,
    style: u32,
    face: designcraft_fonts::FaceRef,
    size: f64,
    /// Baseline shift (points, down positive).
    y: f64,
    x: f64,
    end: f64,
    /// Width PowerPoint gives the text without extra spacing.
    natural: f64,
    tab: bool,
    rs: Option<RunStyle>,
}

/// A run of units sharing style and character spacing.
#[derive(Clone, Debug)]
struct Run {
    text: String,
    style: u32,
    face: designcraft_fonts::FaceRef,
    size: f64,
    y: f64,
    /// Character spacing, points.
    spc: f64,
    rs: Option<RunStyle>,
}

/// A laid-out line ready to write.
#[derive(Clone, Debug)]
pub(crate) struct OutLine {
    runs: Vec<Run>,
    /// Text-space x where the line's first unit starts.
    start: f64,
    /// Text-space x where it ends.
    end: f64,
    baseline: f64,
    descent: f64,
    /// Largest font size on the line (PowerPoint's line height follows it).
    size: f64,
    /// The composer's leading for the line.
    leading: f64,
    tabs: Vec<f64>,
    para: usize,
    last_in_para: bool,
    /// The line ends at a space the composer doesn't draw.
    trailing_space: bool,
}

/// Lines that share a text box.
#[derive(Clone, Debug, Default)]
pub(crate) struct Segment {
    pub lines: Vec<OutLine>,
    pub x0: f64,
    pub x1: f64,
}

pub fn frame(ex: &mut Exporter, it: &Item, tfr: &TextFrame, xf: Affine, opacity: f32) {
    let doc = ex.doc;
    let cs = ex.cache.get(doc, tfr.story, ex.page);
    let Some(ft) = cs.frame(it.id) else {
        ex.shape(it, xf, opacity);
        return;
    };
    let story_text = doc.story(tfr.story).map(|s| s.text.clone()).unwrap_or_default();
    let vertical = doc.frame_vertical(it);
    if vertical {
        ex.warn("vertical text frames are exported with PowerPoint's vertical text (line breaks may differ)");
    }
    let m = ex.to_slide * xf;
    if ex.opts.line_breaks == LineBreaks::Reflow || vertical {
        reflow(ex, it, tfr, ft, &cs, &story_text, xf, opacity, vertical);
        return;
    }
    let Some((_, dm)) = box_of(it.inner_bounds(), m) else {
        ex.warn("skewed text frames are exported unskewed");
        // Upright where the skewed frame's box is (its parents' transforms included).
        let inner = it.inner_bounds();
        let mut plain = it.clone();
        plain.xf = Affine::translate(xf.transform_rect_bbox(inner).origin().to_vec2() - inner.origin().to_vec2());
        frame(ex, &plain, tfr, plain.xf, opacity);
        return;
    };
    let k = (dm.sx * dm.sy).abs().sqrt();
    let table_lines: Vec<usize> = ft.tables.iter().map(|t| t.line).collect();
    let segs = segments(ex, &cs, ft, &story_text, &table_lines);
    let simple = segs.len() == 1 && ft.notes.is_empty() && ft.tables.is_empty() && ft.decos.is_empty();
    if simple && let Some(seg) = segs.first() {
        // One box: the frame itself carries the text, inset to where the lines sit.
        let inner = it.inner_bounds();
        let r = seg_rect(seg);
        let insets = [r.x0 - inner.x0, r.y0 - inner.y0, inner.x1 - r.x1, inner.y1 - r.y1];
        if insets.iter().all(|v| *v >= -0.01) {
            let model = model_body(ex, seg, k, opacity, insets);
            ex.shape_with_text_model(it, xf, opacity, model);
            return;
        }
    }
    ex.shape(it, xf, opacity);
    decos(ex, ft, m, opacity);
    for seg in &segs {
        text_box(ex, seg, m, k, opacity, "Text");
    }
    for n in &ft.notes {
        if let Some(nft) = n.text.frames.first() {
            let nm = m * Affine::translate(n.origin.to_vec2());
            let note = segments(ex, &n.text, nft, &n.source, &[]);
            decos(ex, nft, nm, opacity);
            for seg in &note {
                text_box(ex, seg, nm, k, opacity, "Footnote");
            }
        }
    }
    crate::table::tables(ex, ft, m, k, opacity);
    anchored(ex, tfr, ft, xf, opacity);
}

/// Anchored objects in the frame's text.
fn anchored(ex: &mut Exporter, tfr: &TextFrame, ft: &FrameText, xf: Affine, opacity: f32) {
    let Some(st) = ex.doc.story(tfr.story) else { return };
    let objects: Vec<_> = ft.objects.iter().filter_map(|o| st.objects.get(o.index).map(|obj| (obj.item.clone(), o.origin))).collect();
    for (item, origin) in objects {
        ex.item(&item, xf * Affine::translate(origin.to_vec2()), opacity);
    }
}

/// Paragraph shading and rules.
fn decos(ex: &mut Exporter, ft: &FrameText, m: Affine, opacity: f32) {
    for d in &ft.decos {
        let Some(color) = ex.model_color(&d.color, d.tint, opacity) else { continue };
        if let Some((xfrm, _)) = box_of(d.rect, m) {
            ex.rect_shape(xfrm, craft_pptx::Fill::solid(color), "Paragraph Shading");
        }
    }
}

pub(crate) fn seg_rect(seg: &Segment) -> Rect {
    let first = seg.lines.first();
    let top = first.map_or(0.0, |l| l.baseline - BASELINE_IN_LINE * line_height(seg, 0));
    let bottom = seg.lines.last().map_or(top + 1.0, |l| l.baseline + l.descent);
    Rect::new(seg.x0, top, seg.x1.max(seg.x0 + 1.0), bottom.max(top + 1.0))
}

/// The PowerPoint line height used for line `i` of a segment: the baseline distance inside its
/// paragraph, else the composer's leading for the line.
fn line_height(seg: &Segment, i: usize) -> f64 {
    let Some(l) = seg.lines.get(i) else { return 12.0 };
    para_step(seg, i).unwrap_or(if l.leading > 0.0 { l.leading } else { l.size * SINGLE })
}

/// The baseline-to-baseline distance inside line `i`'s paragraph (the median, in this segment).
fn para_step(seg: &Segment, i: usize) -> Option<f64> {
    let para = seg.lines.get(i)?.para;
    let ls: Vec<&OutLine> = seg.lines.iter().filter(|l| l.para == para).collect();
    median_step(&ls)
}

fn median_step(ls: &[&OutLine]) -> Option<f64> {
    let mut d: Vec<f64> = ls.windows(2).map(|w| w[1].baseline - w[0].baseline).filter(|v| *v > 0.0).collect();
    if d.is_empty() {
        return None;
    }
    d.sort_by(f64::total_cmp);
    d.get(d.len() / 2).copied()
}

/// [`para_step`] for every paragraph of a segment, in one pass (it is asked once per paragraph).
fn para_steps(seg: &Segment) -> std::collections::HashMap<usize, Option<f64>> {
    let mut by_para: std::collections::HashMap<usize, Vec<&OutLine>> = std::collections::HashMap::new();
    for l in &seg.lines {
        by_para.entry(l.para).or_default().push(l);
    }
    by_para.into_iter().map(|(p, ls)| (p, median_step(&ls))).collect()
}

fn text_box(ex: &mut Exporter, seg: &Segment, m: Affine, k: f64, opacity: f32, name: &str) {
    let r = seg_rect(seg);
    let Some((xfrm, _)) = box_of(r, m) else { return };
    let model = model_body(ex, seg, k, opacity, [0.0; 4]);
    write_box(ex, xfrm, model, name);
}

fn write_box(ex: &mut Exporter, xfrm: XfrmBox, model: craft_pptx::TextBody, name: &str) {
    let id = ex.id();
    ex.native_shapes.push(craft_pptx::Shape {
        id: craft_pptx::ShapeId(id),
        name: format!("{name} {id}"),
        xfrm: Some(craft_pptx::Xfrm {
            x: xfrm.rect.x0,
            y: xfrm.rect.y0,
            w: xfrm.rect.width(),
            h: xfrm.rect.height(),
            rot: xfrm.rot,
            flip_h: xfrm.flip_h,
            flip_v: xfrm.flip_v,
        }),
        geom: craft_pptx::Geom::preset("rect"),
        fill: Some(craft_pptx::Fill::None),
        line: Some(craft_pptx::Line::none()),
        text_box: true,
        text: Some(model),
        ..Default::default()
    });
}

/// Cut a frame's lines into segments and lay out their runs.
pub(crate) fn segments(ex: &mut Exporter, cs: &ComposedStory, ft: &FrameText, story: &str, skip: &[usize]) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut prev: Option<&Line> = None;
    let skip: std::collections::HashSet<usize> = skip.iter().copied().collect();
    for (i, l) in ft.lines.iter().enumerate() {
        if skip.contains(&i) {
            prev = None;
            continue;
        }
        let ol = out_line(ex, cs, l, story);
        let joins = prev.is_some_and(|p| {
            p.column == l.column
                && (p.x0 - l.x0).abs() < 0.5
                && (p.x1 - l.x1).abs() < 0.5
                && l.baseline > p.baseline
                && l.baseline - p.baseline < 2.5 * p.leading.max(l.leading).max(ol.size)
        });
        match out.last_mut() {
            Some(seg) if joins => seg.lines.push(ol),
            _ => out.push(Segment { lines: vec![ol], x0: l.x0, x1: l.x1 }),
        }
        prev = Some(l);
    }
    out
}

/// The text for glyph `g`: the story's characters, or what the glyph shows when the composer
/// inserted or changed it (page numbers, list labels, hyphens, case changes).
fn glyph_text(ex: &mut Exporter, g: &PlacedGlyph, story: &str, prev_src: &mut Option<usize>) -> Option<String> {
    let src = story.get(g.byte..g.byte.saturating_add(g.len)).filter(|t| !t.is_empty() && !t.chars().any(is_marker));
    let face = g.face.get();
    if let Some(t) = src {
        *prev_src = Some(g.byte);
        // A single character drawn as another glyph (All Caps): what the glyph shows.
        let mut cs = t.chars();
        if let (Some(c), None) = (cs.next(), cs.next())
            && let Some(shown) = ex.fonts.char_of(face, g.gid)
            && shown != c
            && ex.fonts.glyph(face, c) != Some(g.gid)
            && shown.to_lowercase().eq(c.to_lowercase())
        {
            return Some(shown.to_string());
        }
        return Some(t.to_string());
    }
    if g.len == 0 && *prev_src == Some(g.byte) {
        // Further glyphs of a cluster already written.
        return None;
    }
    *prev_src = None;
    ex.fonts.char_of(face, g.gid).map(|c| c.to_string())
}

fn out_line(ex: &mut Exporter, cs: &ComposedStory, l: &Line, story: &str) -> OutLine {
    let mut units: Vec<Unit> = Vec::new();
    let mut prev_src = None;
    let is_tab = |g: &PlacedGlyph| story.get(g.byte..g.byte.saturating_add(g.len)) == Some("\t");
    let last_shown = l.glyphs.iter().rposition(|g| g.visible || is_tab(g));
    let mut trailing_space = false;
    for (i, g) in l.glyphs.iter().enumerate() {
        let next_x = l.glyphs.get(i + 1).map_or(g.x + g.adv, |n| n.x);
        let is_tab = is_tab(g);
        if !g.visible && !is_tab {
            if last_shown.is_some_and(|k| i > k) {
                trailing_space |= story.get(g.byte..g.byte.saturating_add(g.len)).is_some_and(|t| t.chars().any(char::is_whitespace));
            } else if let Some(u) = units.last_mut() {
                // Invisible glyphs inside the line still take up their room.
                u.end = next_x.max(u.end);
            }
            continue;
        }
        let face = g.face.get();
        let size = g.sy * face.upem;
        let text = if is_tab { Some("\t".to_string()) } else { glyph_text(ex, g, story, &mut prev_src) };
        let Some(text) = text else {
            if let Some(u) = units.last_mut() {
                u.end = next_x.max(u.end);
            }
            continue;
        };
        let natural: f64 = if is_tab { 0.0 } else { text.chars().map(|c| ex.fonts.advance(face, c).unwrap_or(face.upem * 0.5) * g.sy).sum() };
        let rs = cs.styles.get(g.style as usize).cloned();
        units.push(Unit { text, style: g.style, face: g.face, size, y: g.y, x: g.x, end: next_x, natural, tab: is_tab, rs });
    }
    // A space the composer left out of the line (it ends there): in the story at the line's end.
    let end_byte = l.glyphs.iter().filter(|g| g.len > 0).map(|g| g.byte.saturating_add(g.len)).max().unwrap_or(l.range.end);
    let after = story.get(end_byte.min(story.len())..).and_then(|t| t.chars().next());
    trailing_space |= !l.last_in_para && after.is_some_and(|c| c.is_whitespace() && c != '\n' && c != '\r');
    let start = units.first().map_or(l.x0, |u| u.x);
    let end = units.last().map_or(l.x0, |u| u.end);
    let mut runs: Vec<Run> = Vec::new();
    let mut tabs = Vec::new();
    // PowerPoint's pen, and the spacing of the run it is in.
    let mut pen = start;
    for u in &units {
        if u.tab {
            tabs.push(u.end);
            runs.push(Run { text: "\t".into(), style: u.style, face: u.face, size: u.size, y: u.y, spc: 0.0, rs: u.rs.clone() });
            pen = u.end;
            continue;
        }
        let n = u.text.chars().count().max(1) as f64;
        if let Some(r) = runs.last_mut().filter(|r| !r.text.ends_with('\t'))
            && r.style == u.style
            && r.face.get().id() == u.face.get().id()
            && (r.size - u.size).abs() < 0.005
            && (r.y - u.y).abs() < 0.01
            && (pen + u.natural + r.spc * n - u.end).abs() <= TOLERANCE
        {
            r.text.push_str(&u.text);
            pen += u.natural + r.spc * n;
            continue;
        }
        // A new run whose spacing lands this unit where the composer put it (0.01 pt steps).
        let spc = ((u.end - pen - u.natural) / n * 100.0).round() / 100.0;
        let spc = if spc.is_finite() { spc.clamp(-100.0, 100.0) } else { 0.0 };
        runs.push(Run { text: u.text.clone(), style: u.style, face: u.face, size: u.size, y: u.y, spc, rs: u.rs.clone() });
        pen += u.natural + spc * n;
    }
    let size = units.iter().map(|u| u.size).fold(0.0, f64::max);
    OutLine {
        runs,
        start,
        end,
        baseline: l.baseline,
        descent: l.descent,
        size: if size > 0.0 { size } else { (l.ascent + l.descent).max(1.0) / SINGLE },
        leading: l.leading,
        tabs,
        para: l.para,
        last_in_para: l.last_in_para,
        trailing_space,
    }
}

/// Paragraph alignment read off the lines: centred and right-aligned lines leave room on both or
/// the left side; everything else starts at its left indent.
fn align_of(seg: &Segment, lines: &[OutLine]) -> &'static str {
    let gaps: Vec<(f64, f64)> = lines.iter().map(|l| (l.start - seg.x0, seg.x1 - l.end)).collect();
    let roomy = |g: &(f64, f64)| g.0 > 1.0 || g.1 > 1.0;
    if !gaps.iter().any(roomy) {
        return "l";
    }
    if gaps.iter().all(|(a, b)| (a - b).abs() < 0.75) && gaps.iter().any(|(a, _)| *a > 1.0) {
        return "ctr";
    }
    // Flush right with room on the left: more than an indented first line of justified text.
    let indented = gaps.iter().filter(|(a, _)| *a > 1.0).count();
    if gaps.iter().all(|(_, b)| *b < 0.75) && (indented > 1 || (gaps.len() == 1 && indented == 1)) {
        return "r";
    }
    "l"
}

/// The composed lines represented in the shared text model.
/// The model equivalent of the composed text represented by `body`.
pub(crate) fn model_body(ex: &mut Exporter, seg: &Segment, k: f64, opacity: f32, insets: [f64; 4]) -> craft_pptx::TextBody {
    fn color(ex: &Exporter, swatch: &str, tint: f32, opacity: f32) -> Option<craft_pptx::ColorRef> {
        let color = if let Some(gradient) = resolve_gradient(&ex.doc.swatches, swatch) {
            let (_, color, alpha) = gradient.expanded_stops().into_iter().next()?;
            let [r, g, b, a] = color.to_rgba8(alpha * opacity);
            craft_pptx::Rgba::rgba(r, g, b, a)
        } else {
            let [r, g, b, a] = ex.doc.resolve_color(swatch, tint)?.to_rgba8(opacity);
            craft_pptx::Rgba::rgba(r, g, b, a)
        };
        Some(craft_pptx::ColorRef::rgb(color))
    }

    fn run_props(ex: &mut Exporter, run: Option<&Run>, size: f64, k: f64, opacity: f32) -> craft_pptx::text::RunProps {
        let shifted = run.is_some_and(|r| r.y.abs() > 0.01 && r.size > 0.0);
        let actual_size = if shifted { size * 1.5 } else { size };
        let mut props = craft_pptx::text::RunProps { size: Some(actual_size * k), lang: Some("en-US".into()), ..Default::default() };
        let Some(run) = run else { return props };
        let font = ex.fonts.use_face(run.face);
        props.font = Some(font.family.clone());
        props.font_ea = Some(font.family.clone());
        props.font_cs = Some(font.family.clone());
        props.bold = Some(font.bold);
        props.italic = Some(font.italic || run.rs.as_ref().is_some_and(|style| style.skew.abs() > 0.01));
        props.spacing = Some(run.spc * k);
        props.kern = Some(4000.0);
        if shifted {
            props.baseline = Some(-run.y / actual_size);
        }
        if let Some(style) = run.rs.as_ref() {
            if style.underline {
                props.underline = Some("sng".into());
            }
            if style.strikethrough {
                props.strike = Some(craft_pptx::text::Strike::Single);
            }
            props.fill = Some(color(ex, &style.fill, style.fill_tint, opacity).map_or(craft_pptx::Fill::None, craft_pptx::Fill::solid));
            if style.stroke != designcraft_color::swatch::NONE
                && style.stroke_weight > 0.0
                && let Some(color) = color(ex, &style.stroke, style.stroke_tint, opacity)
            {
                props.outline = Some(craft_pptx::Line::solid(color, style.stroke_weight * k));
            }
        }
        props
    }

    let [left, top, right, bottom] = insets;
    let mut model = craft_pptx::TextBody {
        body: craft_pptx::text::BodyProps {
            inset_l: Some(left * k),
            inset_t: Some(top * k),
            inset_r: Some(right * k),
            inset_b: Some(bottom * k),
            anchor: Some(craft_pptx::text::Anchor::Top),
            wrap: Some(false),
            autofit: Some(craft_pptx::text::AutoFit::None),
            rtl_col: Some(false),
            ..Default::default()
        },
        ..Default::default()
    };
    let steps = para_steps(seg);
    let mut previous: Option<(f64, f64)> = None;
    for lines in seg.lines.chunk_by(|a, b| a.para == b.para && !a.last_in_para) {
        let (Some(first), Some(last)) = (lines.first(), lines.last()) else { continue };
        let mut height = steps.get(&first.para).copied().flatten().unwrap_or(if first.leading > 0.0 { first.leading } else { first.size * SINGLE });
        if let Some((previous_baseline, previous_height)) = previous
            && lines.len() == 1
        {
            let natural = (1.0 - BASELINE_IN_LINE) * previous_height + BASELINE_IN_LINE * height;
            let gap = first.baseline - previous_baseline;
            if gap < natural {
                height = ((gap - (1.0 - BASELINE_IN_LINE) * previous_height) / BASELINE_IN_LINE).max(first.size * 0.2);
            }
        }
        let align = match align_of(seg, lines) {
            "ctr" => craft_pptx::text::Align::Center,
            "r" => craft_pptx::text::Align::Right,
            _ => craft_pptx::text::Align::Left,
        };
        let mut paragraph = craft_pptx::text::Paragraph {
            props: craft_pptx::text::ParaProps {
                align: Some(align),
                line_spacing: Some(craft_pptx::text::Spacing::Pct(height / (first.size * SINGLE))),
                ..Default::default()
            },
            ..Default::default()
        };
        if align == craft_pptx::text::Align::Left {
            let rest = lines.get(1).map_or(first.start, |line| line.start) - seg.x0;
            let indent = first.start - seg.x0 - rest;
            if rest.abs() > 0.005 {
                paragraph.props.margin_left = Some(rest * k);
            }
            if indent.abs() > 0.005 {
                paragraph.props.indent = Some(indent * k);
            }
        }
        if let Some((previous_baseline, previous_height)) = previous {
            let natural = (1.0 - BASELINE_IN_LINE) * previous_height + BASELINE_IN_LINE * height;
            let before = first.baseline - previous_baseline - natural;
            if before > 0.005 {
                paragraph.props.space_before = Some(craft_pptx::text::Spacing::Pts(before * k));
            }
        }
        let mut tabs: Vec<f64> = lines.iter().flat_map(|line| line.tabs.iter().copied()).collect();
        tabs.sort_by(f64::total_cmp);
        tabs.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        if !tabs.is_empty() {
            paragraph.props.tabs =
                Some(tabs.into_iter().map(|position| craft_pptx::text::TabStop { pos: (position - seg.x0) * k, align: "l".into() }).collect());
        }
        for (line_index, line) in lines.iter().enumerate() {
            if line_index > 0 {
                paragraph.runs.push(craft_pptx::text::Run {
                    text: String::new(),
                    props: run_props(ex, line.runs.first(), line.size, k, opacity),
                    kind: craft_pptx::text::RunKind::Break,
                });
            }
            let n_runs = line.runs.len();
            for (run_index, run) in line.runs.iter().enumerate() {
                let tail = if run_index + 1 == n_runs
                    && line.trailing_space
                    && align == craft_pptx::text::Align::Left
                    && !run.text.ends_with(char::is_whitespace)
                {
                    " "
                } else {
                    ""
                };
                paragraph.runs.push(craft_pptx::text::Run::with(format!("{}{tail}", run.text), run_props(ex, Some(run), run.size, k, opacity)));
            }
        }
        paragraph.end_props = run_props(ex, last.runs.last(), last.size, k, opacity);
        model.paragraphs.push(paragraph);
        previous = Some((last.baseline, height));
    }
    model
}

/// Build shared paragraph and run model values for a composed text segment.
#[allow(clippy::too_many_arguments)]
fn reflow(ex: &mut Exporter, it: &Item, tfr: &TextFrame, ft: &FrameText, cs: &ComposedStory, story: &str, xf: Affine, opacity: f32, vertical: bool) {
    if ex.doc.story(tfr.story).is_none() {
        ex.shape(it, xf, opacity);
        return;
    }
    let m = ex.to_slide * xf;
    let k = box_of(it.inner_bounds(), m).map_or(1.0, |(_, d)| (d.sx * d.sy).abs().sqrt());
    let model = reflow_model(ex, it, tfr, ft, cs, story, k, opacity, vertical);
    ex.shape_with_text_model(it, xf, opacity, model);
}

/// Reflow text in the shared text model. PowerPoint is free to break each paragraph again, but
/// the words, run formatting, paragraph alignment, columns and frame options stay editable.
#[allow(clippy::too_many_arguments)]
fn reflow_model(
    ex: &mut Exporter,
    it: &Item,
    tfr: &TextFrame,
    ft: &FrameText,
    cs: &ComposedStory,
    story: &str,
    k: f64,
    opacity: f32,
    vertical: bool,
) -> craft_pptx::TextBody {
    let mut lines: Vec<OutLine> = Vec::new();
    let mut by_para: std::collections::BTreeMap<usize, Vec<&Line>> = std::collections::BTreeMap::new();
    for line in &ft.lines {
        by_para.entry(line.para).or_default().push(line);
    }
    for (para, source) in by_para {
        let mut merged: Option<OutLine> = None;
        for line in source {
            let mut out = out_line(ex, cs, line, story);
            out.para = para;
            out.last_in_para = true;
            if line.hyphenated
                && let Some(run) = out.runs.last_mut()
                && run.text.ends_with(['-', '\u{2010}'])
            {
                run.text.pop();
            }
            if out.trailing_space
                && let Some(run) = out.runs.last_mut()
                && !run.text.ends_with(char::is_whitespace)
            {
                run.text.push(' ');
            }
            match &mut merged {
                Some(previous) => {
                    previous.end = out.end;
                    previous.size = previous.size.max(out.size);
                    previous.leading = out.leading;
                    previous.runs.extend(out.runs);
                    previous.tabs.extend(out.tabs);
                    previous.trailing_space = out.trailing_space;
                }
                None => merged = Some(out),
            }
        }
        if let Some(mut line) = merged {
            for run in &mut line.runs {
                run.spc = 0.0;
            }
            lines.push(line);
        }
    }
    let inner = it.inner_bounds();
    let mut segment = Segment { lines, x0: inner.x0 + tfr.options.inset[1], x1: inner.x1 - tfr.options.inset[3] };
    let insets = [tfr.options.inset[1], tfr.options.inset[0], tfr.options.inset[3], tfr.options.inset[2]];
    let mut body = model_body(ex, &segment, k, opacity, insets);
    segment.lines.clear();
    body.body.wrap = Some(true);
    body.body.columns = Some(u32::try_from(ft.columns.len().max(1)).unwrap_or(u32::MAX));
    body.body.col_spacing = Some(tfr.options.gutter * k);
    body.body.vert = vertical.then_some(craft_pptx::text::TextDir::EaVertical);
    body.body.anchor = Some(match tfr.options.vertical_justification {
        designcraft_doc::VerticalJustification::Top => craft_pptx::text::Anchor::Top,
        designcraft_doc::VerticalJustification::Center => craft_pptx::text::Anchor::Middle,
        designcraft_doc::VerticalJustification::Bottom => craft_pptx::text::Anchor::Bottom,
        designcraft_doc::VerticalJustification::Justify => craft_pptx::text::Anchor::Justified,
    });
    for paragraph in &mut body.paragraphs {
        for run in &mut paragraph.runs {
            run.props.spacing = Some(0.0);
        }
    }
    body
}
