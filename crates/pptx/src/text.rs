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

use std::fmt::Write as _;

use designcraft_compose::{ComposedStory, FrameText, Line, PlacedGlyph, RunStyle};
use designcraft_doc::{Align, Item, TextFrame};
use designcraft_geom::{Affine, Rect};

use crate::LineBreaks;
use crate::slide::{Exporter, XfrmBox, box_of};
use crate::xml::{emu, emu_len, esc, pct};

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
    let cs = ex.cache.get(doc, tfr.story, ex.page_name.as_deref());
    let Some(ft) = cs.frame(it.id) else {
        ex.shape(it, xf, opacity, None);
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
            let body = body(ex, seg, r, k, opacity, insets);
            ex.shape(it, xf, opacity, Some(&body));
            return;
        }
    }
    ex.shape(it, xf, opacity, None);
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
        let Some(c) = ex.color(&d.color, d.tint, opacity) else { continue };
        if let Some((xfrm, _)) = box_of(d.rect, m) {
            ex.rect_shape(xfrm, &format!("<a:solidFill>{c}</a:solidFill>"), "Paragraph Shading");
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
    let mut d: Vec<f64> = ls.windows(2).map(|w| w[1].baseline - w[0].baseline).filter(|v| *v > 0.0).collect();
    if d.is_empty() {
        return None;
    }
    d.sort_by(f64::total_cmp);
    d.get(d.len() / 2).copied()
}

fn text_box(ex: &mut Exporter, seg: &Segment, m: Affine, k: f64, opacity: f32, name: &str) {
    let r = seg_rect(seg);
    let Some((xfrm, _)) = box_of(r, m) else { return };
    let body = body(ex, seg, r, k, opacity, [0.0; 4]);
    write_box(ex, xfrm, &body, name);
}

fn write_box(ex: &mut Exporter, xfrm: XfrmBox, body: &str, name: &str) {
    let id = ex.id();
    let _ = write!(
        ex.out,
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name} {id}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>{body}</p:sp>",
        xfrm.xml()
    );
}

/// Cut a frame's lines into segments and lay out their runs.
pub(crate) fn segments(ex: &mut Exporter, cs: &ComposedStory, ft: &FrameText, story: &str, skip: &[usize]) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut prev: Option<&Line> = None;
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

/// `<p:txBody>` for a segment whose box is `r` (text space); `insets` place the text inside a
/// larger shape (left, top, right, bottom).
fn body(ex: &mut Exporter, seg: &Segment, r: Rect, k: f64, opacity: f32, insets: [f64; 4]) -> String {
    let [li, ti, ri, bi] = insets.map(|v| emu_len(v * k));
    format!(
        "<p:txBody><a:bodyPr wrap=\"none\" lIns=\"{li}\" tIns=\"{ti}\" rIns=\"{ri}\" bIns=\"{bi}\" rtlCol=\"0\" anchor=\"t\"><a:noAutofit/></a:bodyPr><a:lstStyle/>{}</p:txBody>",
        paragraphs(ex, seg, r, k, opacity)
    )
}

/// The `<a:p>` elements of a segment whose box is `r` (text space).
pub(crate) fn paragraphs(ex: &mut Exporter, seg: &Segment, r: Rect, k: f64, opacity: f32) -> String {
    let mut s = String::new();
    let mut prev_end: Option<(f64, f64)> = None;
    // The segment's lines, a paragraph at a time; `at` is the index of each one's first line.
    let mut at = 0;
    for lines in seg.lines.chunk_by(|a, b| a.para == b.para && !a.last_in_para) {
        let i = at;
        at += lines.len();
        let (Some(first), Some(last)) = (lines.first(), lines.last()) else { continue };
        let mut h = line_height(seg, i);
        let r_in = BASELINE_IN_LINE;
        if let Some((prev_base, prev_h)) = prev_end
            && lines.len() == 1
        {
            // A one-line paragraph closer to the line before than its own line box allows (tight
            // display type): a smaller box puts it there.
            let natural = (1.0 - r_in) * prev_h + r_in * h;
            let gap = first.baseline - prev_base;
            if gap < natural {
                h = ((gap - (1.0 - r_in) * prev_h) / r_in).max(first.size * 0.2);
            }
        }
        let algn = align_of(seg, lines);
        let mut ppr = format!("<a:pPr algn=\"{algn}\"");
        if algn == "l" {
            let rest = lines.get(1).map_or(first.start, |l| l.start) - r.x0;
            let ind = first.start - r.x0 - rest;
            if rest.abs() > 0.005 {
                let _ = write!(ppr, " marL=\"{}\"", emu(rest * k));
            }
            if ind.abs() > 0.005 {
                let _ = write!(ppr, " indent=\"{}\"", emu(ind * k));
            }
        }
        ppr.push('>');
        let _ = write!(ppr, "<a:lnSpc><a:spcPct val=\"{}\"/></a:lnSpc>", pct(h / (first.size * SINGLE)));
        if let Some((prev_base, prev_h)) = prev_end {
            // PowerPoint puts the next line one line box down: the rest of the previous line's box
            // and the top part of this one. Space Before makes up the difference.
            let natural = (1.0 - r_in) * prev_h + r_in * h;
            let before = first.baseline - prev_base - natural;
            if before > 0.005 {
                let _ = write!(ppr, "<a:spcBef><a:spcPts val=\"{}\"/></a:spcBef>", (before * k * 100.0).round() as i64);
            }
        }
        let tabs: Vec<f64> = lines.iter().flat_map(|l| l.tabs.iter().copied()).collect();
        if !tabs.is_empty() {
            ppr.push_str("<a:tabLst>");
            let mut sorted = tabs.clone();
            sorted.sort_by(f64::total_cmp);
            sorted.dedup_by(|a, b| (*a - *b).abs() < 0.01);
            for t in sorted {
                let _ = write!(ppr, "<a:tab pos=\"{}\" algn=\"l\"/>", emu((t - r.x0) * k));
            }
            ppr.push_str("</a:tabLst>");
        }
        ppr.push_str("</a:pPr>");
        s.push_str("<a:p>");
        s.push_str(&ppr);
        for (n, l) in lines.iter().enumerate() {
            if n > 0 {
                let _ = write!(s, "<a:br>{}</a:br>", rpr(ex, l.runs.first(), l.size, k, opacity, "a:rPr"));
            }
            let n_runs = l.runs.len();
            for (ri, run) in l.runs.iter().enumerate() {
                // The undrawn space at a left-aligned line's end goes back in (invisible there),
                // so removing the break doesn't join words.
                let tail = if ri + 1 == n_runs && l.trailing_space && algn == "l" && !run.text.ends_with(char::is_whitespace) { " " } else { "" };
                let _ = write!(s, "<a:r>{}<a:t>{}{tail}</a:t></a:r>", rpr(ex, Some(run), run.size, k, opacity, "a:rPr"), esc(&run.text));
            }
        }
        s.push_str(&rpr(ex, last.runs.last(), last.size, k, opacity, "a:endParaRPr"));
        s.push_str("</a:p>");
        prev_end = Some((last.baseline, h));
    }
    s
}

/// Run properties (`a:rPr`, or `a:endParaRPr` for the paragraph mark).
fn rpr(ex: &mut Exporter, run: Option<&Run>, size: f64, k: f64, opacity: f32, tag: &str) -> String {
    // PowerPoint draws raised and lowered text at 2/3 of its size: state it larger.
    let shifted = run.is_some_and(|r| r.y.abs() > 0.01 && r.size > 0.0);
    let size = if shifted { size * 1.5 } else { size };
    let sz = ((size * k * 100.0).round() as i64).clamp(100, 400_000);
    let Some(run) = run else { return format!("<{tag} lang=\"en-US\" sz=\"{sz}\" dirty=\"0\"/>") };
    let cs = run.rs.as_ref();
    let font = ex.fonts.use_face(run.face);
    let mut a = format!("<{tag} lang=\"en-US\" sz=\"{sz}\"");
    let italic = font.italic || cs.is_some_and(|c| c.skew.abs() > 0.01);
    if font.bold {
        a.push_str(" b=\"1\"");
    }
    if italic {
        a.push_str(" i=\"1\"");
    }
    if let Some(st) = cs {
        if st.underline {
            a.push_str(" u=\"sng\"");
        }
        if st.strikethrough {
            a.push_str(" strike=\"sngStrike\"");
        }
    }
    // Kerning off: the composer's kerning is in the character spacing already.
    a.push_str(" kern=\"400000\"");
    let spc = (run.spc * k * 100.0).round() as i64;
    if spc != 0 && tag == "a:rPr" {
        let _ = write!(a, " spc=\"{spc}\"");
    }
    if shifted {
        let _ = write!(a, " baseline=\"{}\"", pct(-run.y / size));
    }
    a.push_str(" dirty=\"0\">");
    if let Some(st) = cs {
        if st.stroke != designcraft_color::swatch::NONE
            && st.stroke_weight > 0.0
            && let Some(c) = ex.color(&st.stroke, st.stroke_tint, opacity)
        {
            let _ = write!(a, "<a:ln w=\"{}\"><a:solidFill>{c}</a:solidFill></a:ln>", emu_len(st.stroke_weight * k));
        }
        match ex.color(&st.fill, st.fill_tint, opacity) {
            Some(c) => {
                let _ = write!(a, "<a:solidFill>{c}</a:solidFill>");
            }
            None => a.push_str("<a:noFill/>"),
        }
    }
    let fam = esc(&font.family);
    let _ = write!(a, "<a:latin typeface=\"{fam}\"/><a:ea typeface=\"{fam}\"/><a:cs typeface=\"{fam}\"/></{tag}>");
    a
}

/// Reflow: the frame as one shape whose text PowerPoint sets, in the frame's columns.
#[allow(clippy::too_many_arguments)]
fn reflow(ex: &mut Exporter, it: &Item, tfr: &TextFrame, ft: &FrameText, cs: &ComposedStory, story: &str, xf: Affine, opacity: f32, vertical: bool) {
    let doc = ex.doc;
    let Some(st) = doc.story(tfr.story) else {
        ex.shape(it, xf, opacity, None);
        return;
    };
    let o = &tfr.options;
    let m = ex.to_slide * xf;
    let k = box_of(it.inner_bounds(), m).map_or(1.0, |(_, d)| (d.sx * d.sy).abs().sqrt());
    let [t, l, b, r] = o.inset.map(|v| emu_len(v * k));
    let cols = ft.columns.len().max(1);
    let mut s = format!("<p:txBody><a:bodyPr wrap=\"square\" lIns=\"{l}\" tIns=\"{t}\" rIns=\"{r}\" bIns=\"{b}\" rtlCol=\"0\"");
    if cols > 1 {
        let _ = write!(s, " numCol=\"{cols}\" spcCol=\"{}\"", emu_len(o.gutter * k));
    }
    if vertical {
        s.push_str(" vert=\"eaVert\"");
    }
    let anchor = match o.vertical_justification {
        designcraft_doc::VerticalJustification::Center => "ctr",
        designcraft_doc::VerticalJustification::Bottom => "b",
        designcraft_doc::VerticalJustification::Justify => "just",
        designcraft_doc::VerticalJustification::Top => "t",
    };
    let _ = write!(s, " anchor=\"{anchor}\"><a:noAutofit/></a:bodyPr><a:lstStyle/>");
    // The paragraphs this frame shows, with the runs of their composed glyphs.
    let ranges = st.para_ranges();
    let mut first = true;
    for (pi, range) in ranges.iter().enumerate() {
        if range.end < ft.range.start || range.start >= ft.range.end.max(ft.range.start + 1) {
            continue;
        }
        let lines: Vec<&Line> = ft.lines.iter().filter(|l| l.para == pi).collect();
        let Some(pf) = st.paras.get(pi) else { continue };
        let (pp, chars) = doc.styles.resolve_para(pf);
        let algn = match pp.align {
            Align::Center => "ctr",
            Align::Right => "r",
            Align::LeftJustified | Align::CenterJustified | Align::RightJustified => "just",
            Align::FullyJustified => "dist",
            _ => "l",
        };
        let mut ppr = format!("<a:pPr algn=\"{algn}\"");
        if pp.left_indent.abs() > 0.005 {
            let _ = write!(ppr, " marL=\"{}\"", emu(pp.left_indent * k));
        }
        if pp.right_indent.abs() > 0.005 {
            let _ = write!(ppr, " marR=\"{}\"", emu(pp.right_indent * k));
        }
        if pp.first_line_indent.abs() > 0.005 {
            let _ = write!(ppr, " indent=\"{}\"", emu(pp.first_line_indent * k));
        }
        ppr.push('>');
        let size = chars.size.max(1.0);
        let lead = match lines.windows(2).map(|w| w[1].baseline - w[0].baseline).find(|d| *d > 0.0) {
            Some(d) => d,
            None => lines.first().map_or(size * pp.auto_leading, |l| l.leading.max(1.0)),
        };
        let _ = write!(ppr, "<a:lnSpc><a:spcPct val=\"{}\"/></a:lnSpc>", pct(lead / (size * SINGLE)));
        if !first && pp.space_before > 0.0 {
            let _ = write!(ppr, "<a:spcBef><a:spcPts val=\"{}\"/></a:spcBef>", (pp.space_before * k * 100.0).round() as i64);
        }
        if pp.space_after > 0.0 {
            let _ = write!(ppr, "<a:spcAft><a:spcPts val=\"{}\"/></a:spcAft>", (pp.space_after * k * 100.0).round() as i64);
        }
        ppr.push_str("</a:pPr>");
        s.push_str("<a:p>");
        s.push_str(&ppr);
        let mut last_size = size;
        for l in &lines {
            let ol = out_line(ex, cs, l, story);
            let mut runs: Vec<Run> = Vec::new();
            for mut r in ol.runs {
                // PowerPoint sets the spacing; only tracking-sized adjustments would survive, and
                // they vary line to line, so none is kept.
                r.spc = 0.0;
                match runs.last_mut() {
                    Some(p)
                        if p.style == r.style
                            && p.face.get().id() == r.face.get().id()
                            && (p.size - r.size).abs() < 0.005
                            && (p.y - r.y).abs() < 0.01 =>
                    {
                        p.text.push_str(&r.text)
                    }
                    _ => runs.push(r),
                }
            }
            // Composer hyphens at line ends go: PowerPoint breaks the lines itself.
            if l.hyphenated
                && let Some(r) = runs.last_mut()
                && r.text.ends_with(['-', '\u{2010}'])
            {
                r.text.pop();
            }
            if ol.trailing_space
                && let Some(r) = runs.last_mut()
                && !r.text.ends_with(char::is_whitespace)
            {
                r.text.push(' ');
            }
            for run in &runs {
                let _ = write!(s, "<a:r>{}<a:t>{}</a:t></a:r>", rpr(ex, Some(run), run.size, k, opacity, "a:rPr"), esc(&run.text));
                last_size = run.size;
            }
        }
        s.push_str(&rpr(ex, None, last_size, k, opacity, "a:endParaRPr"));
        s.push_str("</a:p>");
        first = false;
    }
    s.push_str("</p:txBody>");
    ex.shape(it, xf, opacity, Some(&s));
}
