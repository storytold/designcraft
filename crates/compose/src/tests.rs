use designcraft_doc::build::NewDocument;
use designcraft_doc::{Align, Document, ParaAttrs, ParaFormat, SpreadRef};
use designcraft_geom::Rect;

use super::*;

const LOREM: &str = "Typography is the art and technique of arranging type to make written language legible, readable and appealing when displayed. \
The arrangement of type involves selecting typefaces, point sizes, line lengths, line spacing, and letter spacing, and adjusting the space between pairs of letters.";

fn doc_with(text: &str, rect: Rect, para: ParaAttrs) -> (Document, StoryId, ItemId) {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), rect, lid, text, ParaFormat { para, ..Default::default() }).unwrap();
    (d, sid, fid)
}

fn all_lines(cs: &ComposedStory) -> Vec<&Line> {
    cs.frames.iter().flat_map(|f| f.lines.iter()).collect()
}

#[test]
fn composes_simple_paragraph() {
    let (d, sid, _) = doc_with(LOREM, Rect::new(36.0, 36.0, 300.0, 700.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let lines = all_lines(&cs);
    assert!(lines.len() >= 5, "{}", lines.len());
    // Baselines increase by the auto leading (12 × 120% = 14.4).
    for w in lines.windows(2) {
        assert!((w[1].baseline - w[0].baseline - 14.4).abs() < 1e-6);
    }
    // Lines fit their measure.
    for l in &lines {
        assert!(l.end_x <= l.x1 + 0.5, "line overflows: {} > {}", l.end_x, l.x1);
    }
}

#[test]
fn line_ranges_partition_the_story() {
    let text = format!("{LOREM}\n\nSecond paragraph here.\n{LOREM}");
    let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 200.0, 2000.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let mut pos = 0;
    for l in &lines {
        // Each line starts where the previous ended (or after a paragraph separator).
        assert!(l.range.start == pos || l.range.start == pos + 1, "gap at {pos}: {:?}", l.range);
        pos = l.range.end;
    }
    assert_eq!(pos, text.len());
    assert_eq!(lines.iter().filter(|l| l.first_in_para).count(), 4);
}

#[test]
fn justified_lines_fill_the_measure() {
    let (d, sid, _) = doc_with(LOREM, Rect::new(0.0, 0.0, 220.0, 2000.0), ParaAttrs { align: Some(Align::LeftJustified), ..Default::default() });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    for l in &lines[..lines.len() - 1] {
        assert!((l.end_x - l.x1).abs() < 0.6, "justified line ends at {} not {}", l.end_x, l.x1);
    }
    let last = lines.last().unwrap();
    assert!(last.end_x < last.x1 - 1.0, "last line is ragged");
}

#[test]
fn small_frame_oversets() {
    let (d, sid, _) = doc_with(LOREM, Rect::new(0.0, 0.0, 200.0, 40.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.is_overset());
    let shown = cs.frames[0].range.clone();
    assert_eq!(cs.overset_at.unwrap(), shown.end.max(cs.overset_at.unwrap()).min(cs.overset_at.unwrap()));
    assert!(cs.frames[0].lines.len() <= 3);
}

#[test]
fn text_flows_through_threaded_frames() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let long = [LOREM; 4].join("\n");
    let (a, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 200.0, 120.0), lid, &long, ParaFormat::default()).unwrap();
    let (b, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(250.0, 0.0, 450.0, 2000.0), lid, "", ParaFormat::default()).unwrap();
    d.thread(a, b).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert!(!cs.frames[0].lines.is_empty() && !cs.frames[1].lines.is_empty());
    assert_eq!(cs.frames[0].range.end, cs.frames[1].range.start.min(cs.frames[0].range.end).max(cs.frames[0].range.end));
    // The second frame's lines start at its top.
    assert!(cs.frames[1].lines[0].baseline < 20.0);
}

#[test]
fn columns_fill_left_to_right() {
    let (mut d, sid, fid) = doc_with(&[LOREM; 3].join(" "), Rect::new(0.0, 0.0, 400.0, 150.0), ParaAttrs::default());
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.iter().any(|l| l.column == 1));
    let c1 = lines.iter().find(|l| l.column == 1).unwrap();
    assert!(c1.x0 > 200.0);
}

#[test]
fn centered_text_is_centered() {
    let (d, sid, _) = doc_with("Hello", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs { align: Some(Align::Center), ..Default::default() });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let left = l.glyphs[0].x - l.x0;
    let right = l.x1 - l.end_x;
    assert!((left - right).abs() < 0.5, "{left} vs {right}");
}

#[test]
fn caret_and_hit_roundtrip() {
    let (d, sid, _) = doc_with(LOREM, Rect::new(0.0, 0.0, 200.0, 1000.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    for pos in [0, 5, 40, 100, LOREM.len()] {
        let (fi, x, b, _, _) = caret(&cs, pos).unwrap();
        let back = hit(&cs, fi, Point::new(x + 0.1, b - 2.0)).unwrap();
        assert!((back as i64 - pos as i64).abs() <= 1, "{pos} -> {back}");
    }
}

#[test]
fn empty_story_has_one_line() {
    let (d, sid, _) = doc_with("", Rect::new(0.0, 0.0, 200.0, 100.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(cs.line_count(), 1);
    assert!(caret(&cs, 0).is_some());
}

#[test]
fn wrap_pushes_text_aside() {
    let (mut d, sid, _) = doc_with(&[LOREM; 2].join(" "), Rect::new(0.0, 0.0, 300.0, 800.0), ParaAttrs::default());
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut it =
        designcraft_doc::Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 120.0, 60.0)));
    it.wrap.mode = WrapMode::BoundingBox;
    it.wrap.offsets = [0.0, 0.0, 6.0, 6.0];
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let first = &cs.frames[0].lines[0];
    assert!(first.x0 >= 126.0 - 1e-6, "{}", first.x0);
    let below = cs.frames[0].lines.iter().find(|l| l.baseline - l.ascent > 66.0).unwrap();
    assert!(below.x0 < 1.0);
}

#[test]
fn paragraph_composer_is_no_worse_than_greedy() {
    // Sum of squared slack over lines (excluding last) should not exceed the greedy result.
    let measure = 180.0;
    let mk = |c: designcraft_doc::Composer| {
        let (d, sid, _) = doc_with(
            &[LOREM; 2].join(" "),
            Rect::new(0.0, 0.0, measure, 4000.0),
            ParaAttrs { composer: Some(c), hyphenate: Some(false), ..Default::default() },
        );
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let lines = all_lines(&cs);
        let n = lines.len();
        lines[..n - 1].iter().map(|l| (l.x1 - l.end_x).powi(2)).sum::<f64>()
    };
    let kp = mk(designcraft_doc::Composer::Paragraph);
    let gr = mk(designcraft_doc::Composer::SingleLine);
    assert!(kp <= gr * 1.05 + 1.0, "kp {kp} greedy {gr}");
}

#[test]
fn tabs_align_to_stops() {
    let pa = ParaAttrs {
        tabs: Some(vec![designcraft_doc::TabStop { position: 100.0, align: TabAlign::Right, leader: String::new(), align_on: String::new() }]),
        ..Default::default()
    };
    let (d, sid, _) = doc_with("Name\t42", Rect::new(0.0, 0.0, 300.0, 100.0), pa);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    assert!((l.end_x - 100.0).abs() < 0.5, "{}", l.end_x);
}

const SWATCH_TEXT: &str = "Color holds it all together. A restrained palette of two or three swatches, applied consistently to headlines, rules and backgrounds, gives a publication its voice. Spot inks, tints and gradients are all just named swatches, so a single change ripples through every page.";

/// Worst word-space ratio (excluding last lines) of `text` in a narrow justified column.
fn narrow_worst(para: ParaAttrs) -> f64 {
    let para = ParaAttrs { align: Some(Align::LeftJustified), first_line_indent: Some(12.0), hyph_min_word: Some(6), ..para };
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d
        .add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 170.67, 2000.0), lid, SWATCH_TEXT, ParaFormat { para, ..Default::default() })
        .unwrap();
    d.story_mut(sid).unwrap().format_chars(0..SWATCH_TEXT.len(), |f| f.over.size = Some(9.75));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let mut worst = 0.0f64;
    for l in all_lines(&cs) {
        eprintln!("{:5.2} {}", l.spacing, &SWATCH_TEXT[l.range.clone()]);
        assert!(l.end_x <= l.x1 + 0.5, "overfull line");
        if !l.last_in_para {
            assert!((l.end_x - l.x1).abs() < 0.6, "justified line ends at {} not {}", l.end_x, l.x1);
            worst = worst.max(l.spacing);
        }
    }
    worst
}

#[test]
fn justified_narrow_column_has_no_extreme_lines() {
    // Word spacing only (InDesign's defaults: letter spacing 0%, glyph scaling 100%). The first
    // line can't do better than ~2.7: "…together. A re-" is the longest first line that fits.
    let words_only = narrow_worst(ParaAttrs::default());
    assert!(words_only < 4.5, "loose line ratio {words_only}");
    // A typical magazine body setup: letter spacing −5…10% and glyph scaling 97…103% absorb the rest.
    let full = narrow_worst(ParaAttrs {
        letter_space_min: Some(-0.05),
        letter_space_max: Some(0.10),
        glyph_scale_min: Some(0.97),
        glyph_scale_max: Some(1.03),
        ..Default::default()
    });
    assert!(full < 2.0, "loose line ratio {full}");
}

#[test]
fn glyph_scaling_and_letter_spacing_are_applied() {
    let para = ParaAttrs {
        align: Some(Align::LeftJustified),
        hyphenate: Some(false),
        letter_space_min: Some(-0.05),
        letter_space_max: Some(0.2),
        glyph_scale_min: Some(0.95),
        glyph_scale_max: Some(1.05),
        ..Default::default()
    };
    let text = CORPUS.join(" ");
    let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 150.0, 20000.0), para.clone());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let mut scaled = false;
    let mut sum = 0.0;
    for l in &lines[..lines.len() - 1] {
        assert!((l.end_x - l.x1).abs() < 0.6, "justified line ends at {} not {}", l.end_x, l.x1);
        sum += l.spacing;
        let k = 12.0 / l.glyphs[0].face.upem;
        scaled |= l.glyphs.iter().any(|g| g.visible && (g.sx / k - 1.0).abs() > 1e-3);
    }
    assert!(scaled, "some line uses glyph scaling");
    // Word spaces stay closer to their desired width than with word spacing alone.
    let plain = ParaAttrs { align: Some(Align::LeftJustified), hyphenate: Some(false), ..Default::default() };
    let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 150.0, 20000.0), plain);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let pl = all_lines(&cs);
    let plain_avg = pl[..pl.len() - 1].iter().map(|l| l.spacing).sum::<f64>() / (pl.len() - 1) as f64;
    let avg = sum / (lines.len() - 1) as f64;
    assert!(avg < plain_avg - 0.1, "with tiers {avg:.3}, words only {plain_avg:.3}");
    // Desired values apply to every line, including ragged text.
    let wide = |ls: f64| {
        let (d, sid, _) =
            doc_with("Hello world", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs { letter_space_desired: Some(ls), ..Default::default() });
        compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].end_x
    };
    assert!(wide(0.5) > wide(0.0) + 10.0 * 0.5 * 2.0);
}

#[test]
fn hyphen_limit_is_a_hard_constraint() {
    let text = CORPUS.join(" ");
    for limit in [1u32, 2] {
        let para = ParaAttrs { align: Some(Align::LeftJustified), hyph_limit: Some(limit), hyph_weight: Some(0.0), ..Default::default() };
        let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 130.0, 100_000.0), para);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let mut run = 0;
        let mut any = false;
        for l in all_lines(&cs) {
            run = if l.hyphenated { run + 1 } else { 0 };
            any |= l.hyphenated;
            assert!(run <= limit, "{run} consecutive hyphens with limit {limit}");
        }
        assert!(any);
    }
}

#[test]
fn hyphenation_zone_limits_ragged_hyphens() {
    let text = CORPUS.join(" ");
    let count = |zone: f64, composer: designcraft_doc::Composer| {
        let para =
            ParaAttrs { align: Some(Align::Left), hyph_zone: Some(zone), composer: Some(composer), hyph_weight: Some(0.0), ..Default::default() };
        let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 110.0, 100_000.0), para);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs).iter().filter(|l| l.hyphenated).count()
    };
    for c in [designcraft_doc::Composer::Paragraph, designcraft_doc::Composer::SingleLine] {
        let (free, zoned, huge) = (count(0.0, c), count(36.0, c), count(1000.0, c));
        // A huge zone leaves only words that start a line (no space to break at) hyphenable.
        assert!(free > 0 && zoned <= free && huge * 4 <= free, "{c:?}: {free} {zoned} {huge}");
    }
}

#[test]
fn discretionary_hyphen_replaces_automatic_points() {
    let text = "aaaa bbbb cccc extraordi\u{AD}narily dddd eeee";
    let para = ParaAttrs { align: Some(Align::Left), ..Default::default() };
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 120.0, 1000.0), para);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let soft = text.find('\u{AD}').unwrap();
    for l in all_lines(&cs) {
        if l.hyphenated {
            let last = l.glyphs.iter().rfind(|g| g.len > 0).unwrap();
            assert!(last.byte + last.len >= soft && last.byte <= soft + 2, "{:?}", &text[l.range.clone()]);
        }
    }
    // Hyphenation points of the word itself: none besides the discretionary one.
    let lines = all_lines(&cs);
    assert!(lines.iter().filter(|l| l.hyphenated).count() <= 1);
}

/// Story of `n` one-line filler paragraphs followed by `extra` paragraphs, in a 2-column frame.
fn keep_doc(fillers: usize, extra: &[&str], height: f64) -> (Document, StoryId, Vec<usize>) {
    let mut parts: Vec<String> = (0..fillers).map(|k| format!("Filler {k}")).collect();
    parts.extend(extra.iter().map(|s| s.to_string()));
    let text = parts.join("\n");
    let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 400.0, height), ParaAttrs::default());
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    (d, sid, (0..parts.len()).collect())
}

fn column_of_para(cs: &ComposedStory, pi: usize) -> Vec<u32> {
    all_lines(cs).iter().filter(|l| l.para == pi).map(|l| l.column).collect()
}

/// Lines that fit in one column of the keep test frame.
fn lines_per_column(height: f64) -> usize {
    let (d, sid, _) = keep_doc(60, &[], height);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    all_lines(&cs).iter().filter(|l| l.column == 0).count()
}

#[test]
fn keep_with_next_moves_a_heading() {
    let h = 200.0;
    let n = lines_per_column(h);
    let body = "Body text that follows the heading.";
    let (mut d, sid, _) = keep_doc(n - 1, &["Heading", body], h);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(column_of_para(&cs, n - 1), vec![0], "without keeps the heading ends column 0");
    d.story_mut(sid).unwrap().paras[n - 1].para.keep_with_next = Some(1);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(column_of_para(&cs, n - 1), vec![1]);
    assert_eq!(column_of_para(&cs, n), vec![1]);
    assert_eq!(column_of_para(&cs, n - 2), vec![0]);
}

#[test]
fn keep_lines_together_moves_the_paragraph() {
    let h = 200.0;
    let n = lines_per_column(h);
    let long = [LOREM, LOREM].join(" ");
    let (mut d, sid, _) = keep_doc(n - 3, &[&long], h);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(column_of_para(&cs, n - 3).contains(&0));
    d.story_mut(sid).unwrap().paras[n - 3].para.keep_lines_together = Some(true);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(column_of_para(&cs, n - 3).iter().all(|&c| c == 1));
}

#[test]
fn keep_first_and_last_lines() {
    let h = 200.0;
    let n = lines_per_column(h);
    // Orphan: one line at the bottom of column 0 → the paragraph moves.
    let (mut d, sid, _) = keep_doc(n - 1, &[LOREM], h);
    {
        let p = &mut d.story_mut(sid).unwrap().paras[n - 1].para;
        p.keep_lines_together = Some(true);
        p.keep_all_lines = Some(false);
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let cols = column_of_para(&cs, n - 1);
    assert!(cols.iter().all(|&c| c == 1), "{cols:?}");
    // Widow: a paragraph whose last line would sit alone in column 1 gives it a companion.
    let para_lines = {
        let (d, sid, _) = keep_doc(0, &[LOREM], 2000.0);
        compose_story(&d, sid, &ComposeOptions::default()).line_count()
    };
    assert!(para_lines >= 4);
    let start = n - (para_lines - 1);
    let (mut d, sid, _) = keep_doc(start, &[LOREM], h);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(column_of_para(&cs, start).iter().filter(|&&c| c == 1).count(), 1, "one widowed line without keeps");
    {
        let p = &mut d.story_mut(sid).unwrap().paras[start].para;
        p.keep_lines_together = Some(true);
        p.keep_all_lines = Some(false);
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let cols = column_of_para(&cs, start);
    assert_eq!(cols.iter().filter(|&&c| c == 1).count(), 2, "{cols:?}");
    assert!(cols.iter().filter(|&&c| c == 0).count() >= 2);
}

#[test]
fn balance_ragged_lines_evens_a_headline() {
    let text = "Balanced headlines read better than a stub";
    let widths = |balance: bool| {
        let para = ParaAttrs { balance_ragged: Some(balance), hyphenate: Some(false), ..Default::default() };
        let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 500.0), para);
        d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.size = Some(18.0));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs).iter().map(|l| l.end_x - l.x0).collect::<Vec<_>>()
    };
    let plain = widths(false);
    let bal = widths(true);
    assert_eq!(plain.len(), bal.len());
    assert!(plain.len() >= 2);
    let spread = |w: &[f64]| w.iter().cloned().fold(f64::MIN, f64::max) - w.iter().cloned().fold(f64::MAX, f64::min);
    assert!(spread(&bal) < spread(&plain), "{bal:?} vs {plain:?}");
}

#[test]
fn optical_margin_hangs_punctuation() {
    let text = "Hanging punctuation, quietly. “Quotes” sit outside the measure, as do commas, periods and hyphens.";
    let run = |optical: bool| {
        let para = ParaAttrs { align: Some(Align::LeftJustified), optical_margin: Some(optical), ..Default::default() };
        let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 120.0, 1000.0), para);
        compose_story(&d, sid, &ComposeOptions::default())
    };
    let plain = run(false);
    let hung = run(true);
    for l in all_lines(&plain) {
        assert!(l.end_x <= l.x1 + 0.5);
    }
    // Some justified line ends with punctuation that now protrudes past the right edge.
    let lines = all_lines(&hung);
    assert!(lines[..lines.len() - 1].iter().any(|l| l.end_x > l.x1 + 0.5), "no line hangs");
    assert!(lines.iter().all(|l| l.end_x <= l.x1 + 12.0));
}

// ---------- golden paragraph corpus ----------

/// Original prose (written for this corpus) with a realistic mix of short and long words.
const CORPUS: &[&str] = &[
    "Every printed page is a negotiation between the designer and the reader. The designer proposes an order: a column of type, a picture that interrupts it, a caption that explains the picture. The reader accepts that order only when nothing on the page calls attention to itself for the wrong reasons.",
    "Justified text is especially unforgiving. When the measure is narrow, the composer must choose between loose lines with rivers of white running down the column, tight lines in which words collide, and hyphenated lines that interrupt the rhythm of reading. Good composition balances all three considerations across the whole paragraph instead of one line at a time.",
    "Hyphenation dictionaries record where conventional syllable divisions fall in thousands of ordinary words: international, responsibility, characteristically, misunderstanding, photographer, extraordinarily, administration, comprehensive, establishment, unquestionably and their many relatives.",
    "Magazines often set body copy in two or three columns on a page, with generous gutters and a baseline grid that keeps neighbouring lines aligned. Headlines span the columns, pull quotes break into them, and captions sit beside photographs in a smaller, contrasting typeface.",
    "The typesetter of the nineteenth century worked with metal sorts, composing sticks and wedges of spacing material. Many of the conventions we take for granted today, such as the preference for consistent word spacing over consistent line endings, were established by those craftsmen long before computers automated their decisions.",
    "Readers rarely notice good typography, but they immediately feel the discomfort of bad typography: uneven spacing, awkward breaks, widowed lines stranded at the top of a column, and orphaned headings left at the bottom of a page without the paragraph that follows them.",
];

#[derive(Debug, Default)]
struct CorpusStats {
    lines: usize,
    overfull: usize,
    hyphens: usize,
    sum_spacing: f64,
    worst: f64,
    loose: usize,
}

fn corpus_stats(align: Align, measure: f64, size: f64) -> CorpusStats {
    let text = CORPUS.join("\n");
    let para = ParaAttrs { align: Some(align), ..Default::default() };
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, measure, 100_000.0), lid, &text, ParaFormat { para, ..Default::default() }).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.size = Some(size));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let mut st = CorpusStats::default();
    for l in all_lines(&cs) {
        if l.end_x > l.x1 + 0.5 {
            st.overfull += 1;
        }
        if l.hyphenated {
            st.hyphens += 1;
        }
        if !l.last_in_para {
            st.lines += 1;
            st.sum_spacing += l.spacing;
            st.worst = st.worst.max(l.spacing);
            if l.spacing > 1.33 + 1e-6 {
                st.loose += 1;
            }
        }
    }
    st
}

#[test]
fn golden_corpus_spacing() {
    let mut total = CorpusStats::default();
    for &(measure, size) in &[(120.0, 9.0), (170.67, 9.75), (240.0, 10.0), (340.0, 11.0)] {
        for align in [Align::LeftJustified, Align::Left] {
            let st = corpus_stats(align, measure, size);
            let avg = st.sum_spacing / st.lines.max(1) as f64;
            eprintln!(
                "{align:?} measure {measure:6.2} size {size:5.2}: lines {:3} hyphens {:2} avg {avg:.3} worst {:.3} loose {:2} overfull {}",
                st.lines, st.hyphens, st.worst, st.loose, st.overfull
            );
            assert_eq!(st.overfull, 0, "overfull lines at {measure}");
            if align == Align::LeftJustified && measure >= 170.0 {
                assert!(st.worst < 3.5, "worst justified line {} at {measure}", st.worst);
                assert!(avg < 1.65, "average justified word-space ratio {avg} at {measure}");
            }
            if align == Align::LeftJustified {
                total.lines += st.lines;
                total.sum_spacing += st.sum_spacing;
                total.worst = total.worst.max(st.worst);
                total.loose += st.loose;
            }
        }
    }
    let avg = total.sum_spacing / total.lines.max(1) as f64;
    eprintln!("justified total: lines {} avg {avg:.3} worst {:.3} loose {}", total.lines, total.worst, total.loose);
    assert!(avg < 1.9, "average justified word-space ratio {avg}");
}

#[test]
#[ignore = "perf: run with --release -- --ignored --nocapture"]
fn perf_compose_300k_story() {
    let mut text = String::new();
    let mut k = 0;
    while text.len() < 300_000 {
        text.push_str(CORPUS[k % CORPUS.len()]);
        text.push('\n');
        k += 1;
    }
    let para = ParaAttrs { align: Some(Align::LeftJustified), ..Default::default() };
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 240.0, 3_000_000.0), lid, &text, ParaFormat { para, ..Default::default() }).unwrap();
    // Cold run (loads hyphenation data, fills the word caches), then the best of a few warm runs.
    let t = std::time::Instant::now();
    let _ = compose_story(&d, sid, &ComposeOptions::default());
    let cold = t.elapsed().as_secs_f64() * 1000.0;
    let mut ms = f64::INFINITY;
    let mut cs = ComposedStory::default();
    for _ in 0..5 {
        let t = std::time::Instant::now();
        cs = compose_story(&d, sid, &ComposeOptions::default());
        ms = ms.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    eprintln!("composed {} chars, {} lines: cold {cold:.1} ms, warm {ms:.1} ms", text.len(), cs.line_count());
    assert!(!cs.is_overset());
    if !cfg!(debug_assertions) {
        assert!(ms < 300.0, "composition took {ms:.1} ms");
    }
}

#[test]
fn column_break_moves_following_text() {
    let text = format!("First column text.{}Second column text.", designcraft_doc::story::COLUMN_BREAK);
    let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 400.0, 300.0), ParaAttrs::default());
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.len(), 2, "{}", lines.len());
    assert_eq!(lines[0].column, 0);
    assert_eq!(lines[1].column, 1);
    assert!(lines[1].baseline < 20.0, "second column starts at the top");
}

#[test]
fn tabs_without_stops_use_default_half_inch() {
    let (d, sid, _) = doc_with("A\tB", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let b = l.glyphs.iter().find(|g| g.byte == 2).expect("B");
    assert!((b.x - 36.0).abs() < 0.5, "B at {}", b.x);
}

// ---------- tables ----------

fn table_doc(frame: Rect, t: designcraft_doc::Table) -> (Document, StoryId, ItemId) {
    let (mut d, sid, fid) = doc_with("Intro", frame, ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    let end = st.len();
    st.insert_table(end, t);
    let end = st.len();
    st.insert(end, "After");
    d.check().unwrap();
    (d, sid, fid)
}

fn filled(rows: usize, cols: usize, header: usize, width: f64) -> designcraft_doc::Table {
    let mut t = designcraft_doc::Table::new(77, rows, cols, header, 0, width);
    for r in 0..t.nrows() {
        for c in 0..t.ncols() {
            t.cell_mut(r, c).unwrap().text.insert(0, &format!("R{r}C{c}"));
        }
    }
    t
}

#[test]
fn table_rows_fit_their_content() {
    let mut t = filled(3, 3, 1, 300.0);
    t.cell_mut(2, 1).unwrap().text.insert(0, &format!("{LOREM} "));
    t.rows[3].height = 50.0;
    t.rows[3].mode = designcraft_doc::RowHeightMode::Exactly;
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 400.0, 1000.0), t);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let ft = &cs.frames[0];
    assert_eq!(ft.tables.len(), 1);
    let tf = &ft.tables[0];
    assert_eq!(tf.cells.len(), 12);
    let h = |r: usize| tf.cell(r, 0).unwrap().rect.height();
    // One line of 12 pt text + 4 + 4 insets.
    assert!(h(0) > 12.0 && h(0) < 25.0, "{}", h(0));
    // The long cell makes its row taller; all cells in a row share the height.
    assert!(h(2) > 5.0 * h(0), "{} vs {}", h(2), h(0));
    assert!((tf.cell(2, 2).unwrap().rect.height() - h(2)).abs() < 1e-9);
    assert!((h(3) - 50.0).abs() < 1e-9);
    // Cell text sits inside the cell insets.
    let c = tf.cell(1, 1).unwrap();
    let l = &c.text.frames[0].lines[0];
    assert!(c.origin.x + l.x0 >= c.rect.x0 + 3.99);
    assert!(c.origin.y + l.baseline < c.rect.y1);
    // Text after the table is below it.
    let len = d.story(sid).unwrap().len();
    let after = ft.lines.iter().find(|l| l.range.start == len - 5).unwrap();
    assert!(after.baseline > tf.rect.y1);
    // Strokes: outer border + inner edges.
    assert!(tf.strokes.len() >= 4 * 3 + 4);
}

#[test]
fn table_columns_scale_to_the_text_column() {
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 200.0, 1000.0), filled(2, 4, 0, 600.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let tf = &cs.frames[0].tables[0];
    assert!((tf.rect.width() - 200.0).abs() < 1e-6, "{}", tf.rect.width());
    assert!((tf.cell(0, 3).unwrap().rect.x1 - 200.0).abs() < 1e-6);
    // A narrow table keeps its widths.
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 400.0, 1000.0), filled(2, 2, 0, 100.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!((cs.frames[0].tables[0].rect.width() - 100.0).abs() < 1e-6);
}

#[test]
fn table_rows_break_across_frames_with_header_repeat() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (f1, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 300.0, 150.0), lid, "", ParaFormat::default()).unwrap();
    let (f2, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 200.0, 300.0, 350.0), lid, "", ParaFormat::default()).unwrap();
    d.thread(f1, f2).unwrap();
    d.story_mut(sid).unwrap().insert_table(0, filled(9, 2, 1, 300.0));
    d.check().unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset(), "overset at {:?}", cs.overset_at);
    let (a, b) = (&cs.frames[0].tables, &cs.frames[1].tables);
    assert_eq!((a.len(), b.len()), (1, 1));
    // Every body row appears exactly once; the header is repeated in the second frame.
    let body: Vec<usize> = a[0].cells.iter().chain(b[0].cells.iter()).filter(|c| c.row > 0 && c.col == 0).map(|c| c.row).collect();
    assert_eq!(body, (1..10).collect::<Vec<_>>());
    assert!(!a[0].cell(0, 0).unwrap().repeated);
    assert!(b[0].cell(0, 0).unwrap().repeated);
    assert!((b[0].rect.y0 - 200.0).abs() < 1e-6, "{}", b[0].rect.y0);
    assert!(a[0].rect.y1 <= 150.0 + 1e-6);
    assert!(a[0].first && !a[0].last && b[0].last);
    // Without header repeat the second fragment starts with a body row.
    let st = d.story_mut(sid).unwrap();
    st.table_mut(77).unwrap().options.repeat_header = false;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.frames[1].tables[0].cell(0, 0).is_none());
    // Too little room: overset.
    let st = d.story_mut(sid).unwrap();
    let t = st.table_mut(77).unwrap();
    t.insert_rows(5, 30);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.is_overset());
}

#[test]
fn table_cells_hit_test_and_place_carets() {
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 300.0, 1000.0), filled(2, 3, 0, 300.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let c = cs.frames[0].tables[0].cell(1, 2).unwrap().clone();
    let p = Point::new(c.rect.x1 - 2.0, c.rect.center().y);
    let (t, r, col, b) = hit_cell(&cs, 0, p).unwrap();
    assert_eq!((t, r, col), (77, 1, 2));
    assert_eq!(b, "R1C2".len());
    let (fi, x, bl, _, _) = cell_caret(&cs, 77, 1, 2, 0).unwrap();
    assert_eq!(fi, 0);
    assert!(c.rect.contains(Point::new(x + 0.1, bl - 1.0)));
    // Outside every cell: no cell.
    assert!(hit_cell(&cs, 0, Point::new(1.0, 1.0)).is_none());
    // The anchor has a caret position (after the table).
    let a = d.story(sid).unwrap().table_anchor(77).unwrap();
    assert!(caret(&cs, a).is_some());
}

#[test]
fn table_fills_and_merged_cells() {
    let mut t = filled(4, 3, 1, 300.0);
    t.options.alt_rows = Some(designcraft_doc::AltFills { first: 1, next: 1, ..Default::default() });
    t.cell_mut(0, 0).unwrap().fill = "[Black]".into();
    t.merge(designcraft_doc::CellRange::new(1, 1, 2, 2)).unwrap();
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 300.0, 1000.0), t);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let tf = &cs.frames[0].tables[0];
    assert!(tf.cell(0, 0).unwrap().fill.is_some());
    assert!(tf.cell(0, 1).unwrap().fill.is_none(), "header rows don't alternate");
    assert!(tf.cell(1, 0).unwrap().fill.is_some());
    assert!(tf.cell(2, 0).unwrap().fill.is_none());
    let m = tf.cell(1, 1).unwrap();
    let both = tf.cell(1, 0).unwrap().rect.height() + tf.cell(2, 0).unwrap().rect.height();
    assert!((m.rect.height() - both).abs() < 1e-6);
    assert!(tf.cell(2, 2).is_none());
    assert_eq!(tf.cells.len(), 15 - 3);
}

#[test]
fn tab_leaders_fill_the_gap() {
    let mut doc = designcraft_doc::Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = doc.default_layer();
    let pf = designcraft_doc::ParaFormat {
        para: designcraft_doc::ParaAttrs {
            tabs: Some(vec![designcraft_doc::TabStop { position: 200.0, align: TabAlign::Right, leader: ".".into(), align_on: String::new() }]),
            ..Default::default()
        },
        ..Default::default()
    };
    let (_, sid) =
        doc.add_text_frame(designcraft_doc::SpreadRef::Doc(0), designcraft_geom::Rect::new(0.0, 0.0, 300.0, 100.0), lid, "Intro\t12", pf).unwrap();
    let cs = crate::compose_story(&doc, sid, &Default::default());
    let line = &cs.frames[0].lines[0];
    let dots: Vec<&crate::PlacedGlyph> = line.glyphs.iter().filter(|g| g.len == 0 && g.visible).collect();
    assert!(dots.len() > 20, "{} leader glyphs", dots.len());
    let last_dot = dots.iter().map(|g| g.x).fold(0.0, f64::max);
    let num_x = line.glyphs.iter().find(|g| g.byte == 6).unwrap().x;
    assert!(last_dot < num_x, "leaders stop before the page number");
}

#[test]
fn footnotes_sit_at_the_column_bottom_and_push_text() {
    // A short frame: without footnotes the text fills it; with two footnotes the body text
    // makes room and the footnotes stack against the bottom with a rule above.
    let text = format!("{LOREM} {LOREM} {LOREM}");
    let (mut d, sid, fid) = doc_with(&text, Rect::new(36.0, 36.0, 300.0, 236.0), ParaAttrs::default());
    let plain = compose_story(&d, sid, &ComposeOptions::default());
    let plain_lines = plain.frames[0].lines.len();
    {
        let st = d.story_mut(sid).unwrap();
        st.insert_note(10, "First note, long enough to take two lines in this narrow frame for sure.", ParaFormat::default());
        st.insert_note(40, "Second note.", ParaFormat::default());
        st.check().unwrap();
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = cs.frame(fid).unwrap();
    assert_eq!(ft.notes.len(), 2);
    assert!(ft.lines.len() < plain_lines, "body text makes room: {} vs {plain_lines}", ft.lines.len());
    let (a, b) = (&ft.notes[0], &ft.notes[1]);
    assert_eq!(a.label, "1");
    assert_eq!(b.label, "2");
    assert!(a.rect.y1 <= b.rect.y0 + 1e-6, "stacked in order");
    let area = ft.columns[0];
    assert!((b.rect.y1 - area.y1).abs() < 1e-6, "last footnote ends at the column bottom");
    let last = ft.lines.last().unwrap();
    assert!(last.baseline + last.descent <= a.rect.y0 - d.footnote_options.space_before + 0.01, "text clears the footnotes");
    assert!(a.text.frames[0].lines.len() >= 2);
    // The rule sits above the first footnote.
    assert!(ft.decos.iter().any(|dc| (dc.rect.width() - 72.0).abs() < 1e-6 && dc.rect.y1 <= a.rect.y0 + 1e-6));
    // The reference shows its number as superscript glyphs in the text.
    let refs: Vec<&PlacedGlyph> = ft.lines.iter().flat_map(|l| l.glyphs.iter()).filter(|g| g.byte == 10 && g.len > 0).collect();
    assert_eq!(refs.len(), 1);
    // Numbering continues from options and restarts per page when asked.
    d.footnote_options.start_at = 5;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(cs.frames[0].notes[1].label, "6");
    // Hit testing and carets reach footnote text.
    let n = &cs.frames[0].notes[1];
    let (id, _) = hit_note(&cs, 0, Point::new(n.rect.x0 + 40.0, n.rect.y0 + 5.0)).unwrap();
    assert_eq!(id, n.id);
    assert!(note_caret(&cs, n.id, 0).is_some());
}

#[test]
fn footnote_line_at_column_top_still_sets() {
    // A note taller than the frame can't push its reference line forever.
    let (mut d, sid, _) = doc_with("Short text.", Rect::new(0.0, 0.0, 200.0, 40.0), ParaAttrs::default());
    let long = LOREM.repeat(3);
    d.story_mut(sid).unwrap().insert_note(5, &long, ParaFormat::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.frames[0].lines.is_empty());
    assert_eq!(cs.frames[0].notes.len(), 1);
}

#[test]
fn hj_severity_shades() {
    use crate::hj_severity;
    assert_eq!(hj_severity(1.0, 0.8, 1.33), 0);
    assert_eq!(hj_severity(1.4, 0.8, 1.33), 1);
    assert_eq!(hj_severity(1.6, 0.8, 1.33), 2);
    assert_eq!(hj_severity(2.5, 0.8, 1.33), 3);
    assert_eq!(hj_severity(0.6, 0.8, 1.33), 3);
}

#[test]
fn right_to_left_runs_are_ordered_visually() {
    // Hebrew between Latin words: the Hebrew letters read right to left.
    let text = "abc \u{5D0}\u{5D1}\u{5D2} def";
    let (d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 500.0, 200.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = all_lines(&cs)[0];
    let x_of = |byte: usize| l.glyphs.iter().find(|g| g.byte == byte).map(|g| g.x).unwrap();
    let (alef, bet, gimel) = (4, 6, 8);
    assert!(x_of(alef) > x_of(bet) && x_of(bet) > x_of(gimel), "Hebrew reversed");
    assert!(x_of(0) < x_of(gimel) && x_of(alef) < x_of(11), "Latin around it stays in place");
    // A right-to-left paragraph puts its first word on the right.
    let rtl = ParaAttrs { direction: Some(designcraft_doc::TextDirection::RightToLeft), ..Default::default() };
    let (d, sid, _) = doc_with("\u{5D0}\u{5D1} abc", Rect::new(36.0, 36.0, 500.0, 200.0), rtl);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = all_lines(&cs)[0];
    let x_of = |byte: usize| l.glyphs.iter().find(|g| g.byte == byte).map(|g| g.x).unwrap();
    assert!(x_of(0) > x_of(5), "the Hebrew word (first in the text) is right of the Latin one");
    // Shaping keeps clusters in text order.
    let face = designcraft_fonts::FontDb::global().face(designcraft_fonts::DEFAULT_FAMILY, "Regular");
    let g = designcraft_fonts::shape(&face, "ab \u{5D0}\u{5D1}", &[], |c| c);
    assert!(g.windows(2).all(|w| w[0].cluster <= w[1].cluster), "{:?}", g.iter().map(|g| g.cluster).collect::<Vec<_>>());
}

#[test]
fn bidi_matches_the_reference_order() {
    use unicode_bidi::{BidiInfo, Level};
    let text = "English then \u{645}\u{631}\u{62D}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645} 123 and \u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD} end.";
    for rtl in [false, true] {
        let dir = if rtl { designcraft_doc::TextDirection::RightToLeft } else { designcraft_doc::TextDirection::LeftToRight };
        let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 2000.0, 200.0), ParaAttrs { direction: Some(dir), ..Default::default() });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let l = all_lines(&cs)[0];
        // Characters by glyph x (one glyph per character in this text).
        let mut gs: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0).collect();
        gs.sort_by(|a, b| a.x.total_cmp(&b.x));
        let ours: String = gs.iter().map(|g| text[g.byte..].chars().next().unwrap()).collect();
        let info = BidiInfo::new(text, Some(if rtl { Level::rtl() } else { Level::ltr() }));
        let reference = info.reorder_line(&info.paragraphs[0], 0..text.len()).to_string();
        assert_eq!(ours, reference, "rtl={rtl}");
    }
}

#[test]
fn carets_in_right_to_left_text_follow_the_drawing() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    // "abc" then an Arabic word, in a left-to-right paragraph.
    let text = "abc سلام";
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 400.0, 200.0), lid, text, ParaFormat::default()).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let ar = text.find('س').unwrap();
    assert!(l.glyphs.iter().filter(|g| g.len > 0 && g.byte >= ar).all(|g| g.rtl));
    assert!(l.glyphs.iter().filter(|g| g.byte < 3).all(|g| !g.rtl));
    let x = |p: usize| caret(&cs, p).unwrap().1;
    // The Arabic word starts at its right edge and runs left.
    let after = ar + 'س'.len_utf8();
    assert!(x(after) < x(ar), "{} {}", x(after), x(ar));
    assert!(x(text.len()) < x(ar));
    // Arrow keys move as drawn: the word's start is at its right edge, so Left steps into it.
    assert_eq!(visual_step(&cs, ar, true), Some(after));
    assert_eq!(visual_step(&cs, after, false), Some(ar));
    // Latin text has no right-to-left glyphs.
    let (_, s2) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 300.0, 400.0, 400.0), lid, "abc", ParaFormat::default()).unwrap();
    assert_eq!(visual_step(&compose_story(&d, s2, &ComposeOptions::default()), 1, true), None);
    // A click at the word's right edge puts the caret at its start.
    let hx = x(ar) - 0.1;
    assert_eq!(hit(&cs, 0, Point::new(hx, l.baseline)), Some(ar));
}

#[test]
fn kashidas_stretch_justified_arabic_before_spaces() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let text = "بسم الله الرحمن الرحيم الحمد لله رب العالمين الرحمن الرحيم مالك يوم الدين اياك نعبد واياك نستعين";
    let pf = ParaFormat {
        para: ParaAttrs { align: Some(Align::RightJustified), direction: Some(designcraft_doc::TextDirection::RightToLeft), ..Default::default() },
        ..Default::default()
    };
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 260.0, 400.0), lid, text, pf).unwrap();
    let first = |d: &Document| compose_story(d, sid, &ComposeOptions::default()).frames[0].lines[0].clone();
    let with = first(&d);
    assert!(compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines.len() > 1);
    // Spaces keep (about) their natural width; the joins took the extra length.
    let space_w = |l: &Line| l.glyphs.iter().filter(|g| g.len > 0 && text[g.byte..].starts_with(' ')).map(|g| g.adv).fold(0.0, f64::max);
    d.story_mut(sid).unwrap().paras[0].para.kashidas = Some(false);
    let without = first(&d);
    assert!(space_w(&with) < space_w(&without) - 0.5, "{} {}", space_w(&with), space_w(&without));
    // Both fill the measure: the line's (visually last) glyph reaches the left edge either way.
    let left = |l: &Line| l.glyphs.iter().filter(|g| g.len > 0).map(|g| g.x).fold(f64::MAX, f64::min);
    assert!((left(&with) - left(&without)).abs() < 1.0, "{} {}", left(&with), left(&without));
    // Tatweels are drawn in the gaps when the font has one.
    if with.glyphs.iter().any(|g| g.len == 0 && g.gid != 0) {
        assert!(with.glyphs.len() > without.glyphs.len());
    }
}

#[test]
fn digits_option_draws_figures_in_another_script() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 400.0, 200.0), lid, "No. 2024", ParaFormat::default()).unwrap();
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    d.story_mut(sid).unwrap().format_chars(0..8, |f| f.over.digits = Some(designcraft_doc::Digits::Hindi));
    let hindi = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    // One glyph per digit, each still mapped to its own source character.
    let digits: Vec<_> = hindi.iter().filter(|g| g.byte >= 4 && g.len > 0).collect();
    assert_eq!(digits.iter().map(|g| (g.byte, g.len)).collect::<Vec<_>>(), [(4, 1), (5, 1), (6, 1), (7, 1)]);
    // The letters are untouched; the digits are other glyphs.
    assert_eq!(plain[0].gid, hindi[0].gid);
    let gid = |gs: &[PlacedGlyph], b: usize| gs.iter().find(|g| g.byte == b).map(|g| g.gid);
    assert_ne!(gid(&plain, 4), gid(&hindi, 4));
}

#[test]
fn ruby_and_kenten_sit_over_their_text() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 400.0, 200.0), lid, "漢字です", ParaFormat::default()).unwrap();
    let base_n = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.len();
    let kanji = 0.."漢字".len();
    d.story_mut(sid).unwrap().format_chars(kanji.clone(), |f| f.over.ruby = Some("かんじ".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let ruby: Vec<_> = l.glyphs[base_n..].iter().collect();
    assert_eq!(ruby.len(), 3, "one glyph per kana");
    let size = cs.styles[l.glyphs[0].style as usize].size;
    let (x0, x1) = (l.glyphs[0].x, l.glyphs[1].x + l.glyphs[1].adv);
    // Above the base, half size, within the base's width (shorter: spread 1-2-1).
    assert!(ruby.iter().all(|g| g.y < -size * 0.8 && g.len == 0 && (g.sx - l.glyphs[0].sx * 0.5).abs() < 1e-9));
    assert!(ruby[0].x > x0 && ruby[2].x + ruby[2].adv < x1 + 1e-6, "{x0} {x1} {:?}", ruby.iter().map(|g| g.x).collect::<Vec<_>>());
    // Caret positions ignore them.
    assert_eq!(caret(&cs, "漢字".len()).unwrap().1, l.glyphs[2].x);
    // Kenten: a dot over each character.
    d.story_mut(sid).unwrap().format_chars(kanji.end..kanji.end + "で".len(), |f| f.over.kenten = Some(true));
    let l = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].clone();
    assert_eq!(l.glyphs.len(), base_n + 4);
    let dot = l.glyphs.last().unwrap();
    let de = &l.glyphs[2];
    assert!((dot.x + dot.adv / 2.0 - (de.x + de.adv / 2.0)).abs() < 1.0 && dot.y < 0.0);
}

#[test]
fn tate_chu_yoko_sets_digits_across_one_em() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 400.0), lid, "令和12年", ParaFormat::default()).unwrap();
    let at = "令和".len();
    d.story_mut(sid).unwrap().format_chars(at..at + 2, |f| f.over.tate_chu_yoko = Some(true));
    let glyphs = |d: &Document| compose_story(d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    // Horizontal text ignores it.
    assert!(glyphs(&d).iter().all(|g| g.tcy.is_none()));
    if let Some(tf) = d.item_mut(fid).and_then(|i| i.text_frame_mut()) {
        tf.options.vertical = true;
    }
    let gs = glyphs(&d);
    let digits: Vec<_> = gs.iter().filter(|g| g.tcy.is_some()).collect();
    assert_eq!(digits.len(), 2);
    let em = digits[0].tcy.unwrap()[2];
    // The pair takes one em along the line, so the next ideograph follows an em after it.
    assert!((digits.iter().map(|g| g.adv).sum::<f64>() - em).abs() < 1e-6);
    let nen = gs.iter().find(|g| g.byte == at + 2).unwrap();
    assert!((nen.x - (digits[0].x + em)).abs() < 1e-6, "{} {}", nen.x, digits[0].x);
    // Both turn about the same centre and sit side by side across it.
    let [a0, x0, _] = digits[0].tcy.unwrap();
    let [a1, x1, _] = digits[1].tcy.unwrap();
    assert!(((digits[0].x + a0) - (digits[1].x + a1)).abs() < 1e-6);
    assert!(x0 < 0.0 && x1 > x0 && x1 < em, "{x0} {x1}");
}

#[test]
fn vertical_frames_compose_in_the_turned_box() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (fid, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 400.0), lid, "縦書きの文章です。Latin", ParaFormat::default()).unwrap();
    if let Some(tf) = d.item_mut(fid).and_then(|i| i.text_frame_mut()) {
        tf.options.vertical = true;
        tf.options.inset = [0.0; 4];
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = &cs.frames[0];
    assert!(ft.vertical);
    // The line runs along the frame's height (300 pt), so it all fits on one line.
    assert_eq!(ft.lines.len(), 1);
    let l = &ft.lines[0];
    assert!(l.x1 - l.x0 > 290.0, "{} {}", l.x0, l.x1);
    assert!(l.glyphs.iter().filter(|g| g.len > 0).take(5).all(|g| g.upright));
    assert!(!l.glyphs.iter().rev().find(|g| g.len > 0).unwrap().upright, "Latin turns");
    // Text space → frame: the first line sits at the right edge and runs down.
    let it = d.item(fid).unwrap();
    let p0 = it.text_xf() * designcraft_geom::Point::new(l.glyphs[0].x, l.baseline);
    let p1 = it.text_xf() * designcraft_geom::Point::new(l.glyphs[3].x, l.baseline);
    assert!(p0.x > 150.0 && p1.y > p0.y && (p1.x - p0.x).abs() < 1e-6, "{p0:?} {p1:?}");
}

#[test]
fn cjk_text_breaks_between_characters_with_kinsoku() {
    use crate::breaker::cjk_break_between;
    assert!(cjk_break_between('日', '本'));
    assert!(!cjk_break_between('す', '。'), "no line starts with a full stop");
    assert!(!cjk_break_between('「', '日'), "no line ends with an opening bracket");
    assert!(!cjk_break_between('a', 'b'));
    let text = "日本語の文章は単語の間に空白を入れずに書くので、文字と文字の間で改行します。";
    let (d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 156.0, 400.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.len() > 1, "wraps");
    for l in &lines {
        assert!(l.end_x <= l.x1 + 0.5, "line fits: {} > {}", l.end_x, l.x1);
        let first = text[l.range.clone()].chars().next().unwrap_or(' ');
        assert!(!"、。".contains(first), "kinsoku: {:?}", &text[l.range.clone()]);
    }
}

#[test]
fn only_english_text_gets_english_hyphenation() {
    let text = "internationalization internationalization internationalization internationalization";
    let narrow = Rect::new(36.0, 36.0, 120.0, 700.0);
    let hyphenated = |d: &Document, sid| all_lines(&compose_story(d, sid, &ComposeOptions::default())).iter().filter(|l| l.hyphenated).count();
    let (d, sid, _) = doc_with(text, narrow, ParaAttrs::default());
    assert!(hyphenated(&d, sid) > 0, "English hyphenates");
    let mut d2 = d.clone();
    let st = d2.story_mut(sid).unwrap();
    let n = st.len();
    st.format_chars(0..n, |f| f.over.language = Some("French".into()));
    assert_eq!(hyphenated(&d2, sid), 0, "French isn't hyphenated with English rules");
}

#[test]
fn cjk_aki_adds_space_without_scaling_outlines() {
    let (mut d, sid, _) = doc_with("AB", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    let old = compose_story(&d, sid, &ComposeOptions::default());
    d.story_mut(sid).unwrap().format_chars(0..2, |f| {
        f.over.leading_aki = Some(Some(0.25));
        f.over.trailing_aki = Some(Some(0.5));
    });
    let new = compose_story(&d, sid, &ComposeOptions::default());
    let a = &old.frames[0].lines[0];
    let b = &new.frames[0].lines[0];
    assert!((b.end_x - a.end_x - 18.0).abs() < 1e-6);
    assert_eq!(a.glyphs[0].sx, b.glyphs[0].sx);
    assert!((b.glyphs[0].x - a.glyphs[0].x - 3.0).abs() < 1e-6);
}

#[test]
fn cjk_jidori_sets_group_width_and_survives_narrow_frames() {
    for composer in [designcraft_doc::Composer::SingleLine, designcraft_doc::Composer::Paragraph] {
        let (mut d, sid, _) = doc_with(
            "AB",
            Rect::new(0.0, 0.0, 30.0, 150.0),
            ParaAttrs { composer: Some(composer), glyph_scale_desired: Some(1.2), letter_space_desired: Some(0.2), ..Default::default() },
        );
        d.story_mut(sid).unwrap().format_chars(0..2, |f| f.over.jidori = Some(4));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let lines = all_lines(&cs);
        assert_eq!(lines.len(), 1, "an indivisible jidori group must not be emergency-split");
        assert!((lines[0].end_x - lines[0].x0 - 48.0).abs() < 1e-6);
        assert_eq!(lines[0].range, 0..2);
    }
}

#[test]
fn cjk_tsume_removes_sidebearings_without_collapsing_ink() {
    let (mut d, sid, _) = doc_with("HH", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    let old = compose_story(&d, sid, &ComposeOptions::default());
    d.story_mut(sid).unwrap().format_chars(0..2, |f| f.over.tsume = Some(1.0));
    let new = compose_story(&d, sid, &ComposeOptions::default());
    let a = &old.frames[0].lines[0];
    let b = &new.frames[0].lines[0];
    let g = &b.glyphs[0];
    let outline = designcraft_fonts::FontDb::global().outline(&g.face, g.gid);
    let ink_width = designcraft_geom::Shape::bounding_box(&*outline).width() * g.sx;
    assert!(ink_width > 0.0);
    assert!((b.end_x - b.x0 - 2.0 * ink_width).abs() < 1e-6);
    assert!(b.end_x < a.end_x);
    assert_eq!(a.glyphs[0].sx, g.sx);
}

#[test]
fn cjk_composite_font_selects_entries_before_shaping() {
    use designcraft_doc::cjk::{CompositeFont, CompositeFontEntry};
    let (mut d, sid, _) = doc_with("A1B", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    std::sync::Arc::make_mut(&mut d.styles).composite_fonts.push(CompositeFont {
        name: "Mixed".into(),
        entries: vec![
            CompositeFontEntry { family: "Source Serif 4".into(), ..Default::default() },
            CompositeFontEntry { characters: "0123456789".into(), family: "Source Sans 3".into(), relative_size: 0.5, ..Default::default() },
        ],
    });
    d.story_mut(sid).unwrap().format_chars(0..3, |f| f.over.font_family = Some("CompositeFont/Mixed".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let line = &cs.frames[0].lines[0];
    let a = line.glyphs.iter().find(|g| g.byte == 0).unwrap();
    let n = line.glyphs.iter().find(|g| g.byte == 1).unwrap();
    assert_ne!(a.face.id(), n.face.id());
    assert_eq!(cs.styles[a.style as usize].size, 12.0);
    assert_eq!(cs.styles[n.style as usize].size, 6.0);
    assert!(!cs.styles.iter().any(|s| s.missing_font));
}

#[test]
fn cjk_bunri_does_not_split_double_dash() {
    for composer in [designcraft_doc::Composer::SingleLine, designcraft_doc::Composer::Paragraph] {
        let (d, sid, _) =
            doc_with("--", Rect::new(0.0, 0.0, 5.0, 100.0), ParaAttrs { composer: Some(composer), bunri_kinshi: Some(true), ..Default::default() });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        assert_eq!(all_lines(&cs).len(), 1);
    }
}

#[test]
fn cjk_custom_kinsoku_disables_selected_breaks() {
    let (mut d, sid, _) = doc_with(
        "甲乙丙丁",
        Rect::new(0.0, 0.0, 25.0, 200.0),
        ParaAttrs {
            composer: Some(designcraft_doc::Composer::SingleLine),
            kinsoku: Some(Some(designcraft_doc::cjk::Kinsoku { name: "Test".into(), no_start: "乙丙丁".into(), ..Default::default() })),
            ..Default::default()
        },
    );
    d.story_mut(sid).unwrap().format_chars(0.."甲乙丙丁".len(), |f| f.over.size = Some(12.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(all_lines(&cs).len(), 1);
}

#[test]
fn cjk_center_leading_measures_em_centers_across_different_sizes() {
    let (mut d, sid, _) = doc_with("A\nB", Rect::new(0.0, 0.0, 300.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..3, |f| {
        f.over.leading = Some(designcraft_doc::Leading::Points(30.0));
        f.over.leading_model = Some(designcraft_doc::cjk::LeadingModel::Center);
    });
    d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.size = Some(24.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.len(), 2);
    let center = |l: &Line| {
        let g = &l.glyphs[0];
        let (a, b) = g.face.vertical_metrics();
        l.baseline + (b - a) / (2.0 * (a + b)) * g.face.units_per_em() * g.sy
    };
    assert!((center(lines[1]) - center(lines[0]) - 30.0).abs() < 1e-6);
    assert!((lines[1].baseline - lines[0].baseline - 30.0).abs() > 0.1);
}

#[test]
fn cjk_em_center_alignment_moves_small_characters() {
    let (mut d, sid, _) = doc_with("AB", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.size = Some(24.0));
    d.story_mut(sid).unwrap().format_chars(1..2, |f| f.over.character_alignment = Some(designcraft_doc::cjk::CharacterAlignment::EmCenter));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let line = &cs.frames[0].lines[0];
    let center = |g: &PlacedGlyph| {
        let (a, b) = g.face.vertical_metrics();
        g.y + (b - a) / (2.0 * (a + b)) * g.face.units_per_em() * g.sy
    };
    assert!((center(&line.glyphs[0]) - center(&line.glyphs[1])).abs() < 1e-6);
}

#[test]
fn cjk_hanging_punctuation_uses_frame_edge_without_changing_glyph_size() {
    let k = designcraft_doc::cjk::Kinsoku { name: "Test".into(), no_start: ",".into(), hanging: ",".into(), ..Default::default() };
    let (mut d, sid, fid) = doc_with(
        "AB,",
        Rect::new(0.0, 0.0, 300.0, 100.0),
        ParaAttrs {
            composer: Some(designcraft_doc::Composer::SingleLine),
            kinsoku: Some(Some(k)),
            kinsoku_hang: Some(designcraft_doc::cjk::KinsokuHang::Regular),
            ..Default::default()
        },
    );
    let initial = compose_story(&d, sid, &ComposeOptions::default());
    let line = &initial.frames[0].lines[0];
    let text_w = line.glyphs.iter().take(2).map(|g| g.adv).sum::<f64>();
    // A fresh frame whose measure fits the letters, but not the comma.
    let frame = d.item_mut(fid).unwrap();
    frame.path = designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, text_w + 1.0, 100.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(all_lines(&cs).len(), 1);
    assert_eq!(cs.frames[0].lines[0].glyphs.last().unwrap().sx, line.glyphs.last().unwrap().sx);
}

#[test]
fn cjk_fullwidth_space_obeys_the_paragraph_spacing_switch() {
    let text = "A　B";
    let (mut d, sid, _) = doc_with(
        text,
        Rect::new(0.0, 0.0, 300.0, 100.0),
        ParaAttrs { word_space_desired: Some(2.0), treat_ideographic_space_as_space: Some(false), ..Default::default() },
    );
    let fixed = compose_story(&d, sid, &ComposeOptions::default());
    d.story_mut(sid).unwrap().paras[0].para.treat_ideographic_space_as_space = Some(true);
    let elastic = compose_story(&d, sid, &ComposeOptions::default());
    let space = fixed.frames[0].lines[0].glyphs.iter().find(|g| g.byte == 1).unwrap().adv;
    assert!((elastic.frames[0].lines[0].end_x - fixed.frames[0].lines[0].end_x - space).abs() < 1e-6);
}

#[test]
fn arabic_bidi_keeps_paragraph_context_across_forced_lines() {
    let text = "مرحبا\u{2028}123 - 456";
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 600.0, 200.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.len(), 2);
    let mut gs: Vec<_> = lines[1].glyphs.iter().filter(|g| g.len > 0).collect();
    gs.sort_by(|a, b| a.x.total_cmp(&b.x));
    let actual: String = gs.iter().map(|g| text[g.byte..].chars().next().unwrap()).collect();
    let info = unicode_bidi::BidiInfo::new(text, Some(unicode_bidi::Level::ltr()));
    let start = text.find('1').unwrap();
    let expected = info.reorder_line(&info.paragraphs[0], start..text.len()).to_string();
    assert_eq!(actual, expected);
    assert_ne!(actual, "123 - 456", "the previous Arabic strong character affects number ordering");
}

#[test]
fn arabic_character_direction_override_survives_line_layout() {
    let text = "abc 123 def";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 600.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(4..7, |f| f.over.character_direction = Some(designcraft_doc::arabic::CharacterDirection::RightToLeft));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let line = all_lines(&cs)[0];
    let x = |byte| line.glyphs.iter().find(|g| g.byte == byte).unwrap().x;
    assert!(x(4) > x(5) && x(5) > x(6));
    assert!(x(0) < x(6) && x(4) < x(8));
}

#[test]
fn arabic_fallback_marks_stay_with_bases_and_custom_offsets_move_only_marks() {
    let db = designcraft_fonts::FontDb::global();
    let Some(face) = db.fallback_for('ب', db.face(designcraft_fonts::DEFAULT_FAMILY, "Regular").id()) else { return };
    if !face.covers('ُ') {
        return;
    }
    let text = "بُبَ";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 600.0, 200.0), ParaAttrs::default());
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    assert!(plain.iter().all(|g| g.face.id() == face.id()), "fallback must not split Arabic marks into the Latin face");
    assert!(plain.iter().all(|g| g.gid != 0));
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.diacritic_x_offset = Some(200.0);
        f.over.diacritic_y_offset = Some(-100.0);
    });
    let shifted = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    let mut marks = 0;
    for (a, b) in plain.iter().zip(&shifted) {
        assert_eq!(a.gid, b.gid);
        assert_eq!(a.adv, b.adv);
        if face.glyph_is_mark(a.gid) {
            marks += 1;
            assert!((b.x - a.x - 2.4).abs() < 1e-6);
            assert!((b.y - a.y - 1.2).abs() < 1e-6);
        } else {
            assert!((a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6);
        }
    }
    assert!(marks > 0);
}

#[test]
fn arabic_joining_context_crosses_character_style_boundaries() {
    let text = "ببب";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 600.0, 200.0), ParaAttrs::default());
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    if plain.iter().any(|g| g.gid == 0) {
        return;
    }
    d.story_mut(sid).unwrap().format_chars(2..4, |f| f.over.fill = Some("Paper".into()));
    let split = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    assert_eq!(plain.iter().map(|g| (g.byte, g.gid)).collect::<Vec<_>>(), split.iter().map(|g| (g.byte, g.gid)).collect::<Vec<_>>());
}

#[test]
fn arabic_character_kashida_switch_prevents_insertion() {
    let text = "بسم الله الرحمن الرحيم الحمد لله رب العالمين";
    let (mut d, sid, _) = doc_with(
        text,
        Rect::new(0.0, 0.0, 160.0, 300.0),
        ParaAttrs { direction: Some(designcraft_doc::TextDirection::RightToLeft), align: Some(Align::RightJustified), ..Default::default() },
    );
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.allow_kashidas = Some(false));
    let off = compose_story(&d, sid, &ComposeOptions::default());
    assert!(all_lines(&off).iter().all(|l| l.glyphs.iter().all(|g| g.len > 0 || g.face.glyph_for('\u{0640}') != g.gid)));
}

#[test]
fn arabic_indic_digit_conversion_retains_original_utf8_ranges() {
    let text = "١٢٣";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 200.0, 100.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.digits = Some(designcraft_doc::Digits::Arabic));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let gs = &cs.frames[0].lines[0].glyphs;
    assert_eq!(gs.iter().map(|g| (g.byte, g.len)).collect::<Vec<_>>(), [(0, 2), (2, 2), (4, 2)]);
    for (g, c) in gs.iter().zip("123".chars()) {
        assert_eq!(g.gid, g.face.glyph_for(c));
    }
}

#[test]
fn arabic_contextual_digits_follow_strong_text_across_style_changes() {
    let text = "ب 12 A 34";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    let arabic_digits = text.find('1').unwrap();
    d.story_mut(sid).unwrap().format_chars(arabic_digits..arabic_digits + 2, |f| f.over.fill = Some("Paper".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let gs = &cs.frames[0].lines[0].glyphs;
    let g = gs.iter().find(|g| g.byte == arabic_digits).unwrap();
    assert_eq!(g.gid, g.face.glyph_for('١'));
    let g = gs.iter().find(|g| g.byte == text.find('3').unwrap()).unwrap();
    assert_eq!(g.gid, g.face.glyph_for('3'));
}

#[test]
fn arabic_story_direction_flows_right_column_first() {
    let text = format!("first{}second", designcraft_doc::story::COLUMN_BREAK);
    let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 300.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().direction = designcraft_doc::TextDirection::RightToLeft;
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].x0 > lines[1].x0);
    assert!(cs.frames[0].columns[0].x0 > cs.frames[0].columns[1].x0);
}

#[test]
fn arabic_rtl_table_keeps_logical_indices_and_merged_cell_hit_testing() {
    let mut t = designcraft_doc::Table::new(987, 2, 3, 0, 0, 240.0);
    t.options.direction = designcraft_doc::TextDirection::RightToLeft;
    t.cell_mut(0, 0).unwrap().text = designcraft_doc::Story::with_text(StoryId(0), "Right", ParaFormat::default());
    t.cell_mut(0, 2).unwrap().text = designcraft_doc::Story::with_text(StoryId(0), "Left", ParaFormat::default());
    t.merge(designcraft_doc::CellRange::new(1, 0, 1, 1)).unwrap();
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 400.0, 400.0), t);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let (_, _, right) = table::find_cell(&cs, 987, 0, 0).unwrap();
    let (_, _, left) = table::find_cell(&cs, 987, 0, 2).unwrap();
    assert!(right.rect.x0 > left.rect.x0);
    let (_, _, merged) = table::find_cell(&cs, 987, 1, 0).unwrap();
    assert!((merged.rect.width() - 160.0).abs() < 1e-6);
    assert_eq!(table::hit_cell(&cs, 0, right.rect.center()).map(|(_, r, c, _)| (r, c)), Some((0, 0)));
}

#[test]
fn arabic_kashida_moves_marks_with_their_cluster_and_respects_nonjoiners() {
    let text = "بُسْمِ بُسْمِ";
    let (mut d, sid, _) = doc_with(
        text,
        Rect::new(0.0, 0.0, 180.0, 100.0),
        ParaAttrs { direction: Some(designcraft_doc::TextDirection::RightToLeft), align: Some(Align::FullyJustified), ..Default::default() },
    );
    let with = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    if with.iter().any(|g| g.gid == 0) {
        return;
    }
    assert!(with.iter().any(|g| g.len == 0 && g.gid == g.face.glyph_for('\u{0640}')));
    d.story_mut(sid).unwrap().paras[0].para.kashidas = Some(false);
    let without = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    for (a, b) in with.iter().zip(&without) {
        assert_eq!((a.gid, a.byte), (b.gid, b.byte));
        let first_a = with.iter().find(|g| g.byte == a.byte).unwrap();
        let first_b = without.iter().find(|g| g.byte == b.byte).unwrap();
        assert!(((a.x - first_a.x) - (b.x - first_b.x)).abs() < 1e-6, "marks detached from their cluster");
    }
    let text = "ب\u{200C}ب ب\u{200C}ب";
    let (d, sid, _) = doc_with(
        text,
        Rect::new(0.0, 0.0, 180.0, 100.0),
        ParaAttrs { direction: Some(designcraft_doc::TextDirection::RightToLeft), align: Some(Align::FullyJustified), ..Default::default() },
    );
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.frames[0].lines[0].glyphs.iter().all(|g| g.gid != g.face.glyph_for('\u{0640}') || g.len > 0));
}

#[test]
fn arabic_explicit_isolated_forms_disable_contextual_joining() {
    let text = "بب";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 180.0, 100.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.positional_form = Some("Isolated".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let gs = &cs.frames[0].lines[0].glyphs;
    for g in gs {
        assert_eq!(g.gid, g.face.glyph_for('ب'));
    }
}

#[test]
fn arabic_character_overrides_mirror_brackets_using_final_levels() {
    use designcraft_doc::arabic::CharacterDirection as D;
    for (text, direction, mirrored) in [("(123)", D::RightToLeft, true), ("(ب)", D::LeftToRight, false)] {
        let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
        d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.character_direction = Some(direction));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let gs = &cs.frames[0].lines[0].glyphs;
        for (byte, ch) in [(0, '('), (text.len() - 1, ')')] {
            let g = gs.iter().find(|g| g.byte == byte).unwrap();
            let expected = if mirrored { unicode_bidi_mirroring::get_mirrored(ch).unwrap() } else { ch };
            assert_eq!(g.gid, g.face.glyph_for(expected), "{text:?} {direction:?} byte {byte}");
        }
    }
}
