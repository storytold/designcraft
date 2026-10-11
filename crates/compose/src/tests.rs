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

#[test]
fn hyphenation_skips_a_no_break_word_at_the_start() {
    // Used to index before the first glyph and panic.
    let text = "Extraordinarily long words wrap in a narrow column";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 60.0, 1000.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..15, |f| f.over.no_break = Some(true));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    // The no-break word stays whole on the first line.
    let first = all_lines(&cs)[0];
    assert!(!first.hyphenated && first.range.end >= 15, "{:?}", &text[first.range.clone()]);
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

/// InDesign: a break character ends its paragraph; the next paragraph starts on the next page (odd
/// or even for those breaks), with no empty line where the break was.
#[test]
fn break_characters_end_their_paragraph_and_keep_page_parity() {
    use designcraft_doc::story::{EVEN_PAGE_BREAK, ODD_PAGE_BREAK, PAGE_BREAK};
    let mut d = Document::new(&NewDocument { pages: 4, facing_pages: false, primary_text_frame: true, ..Default::default() });
    let sid = d.settings.primary_story.unwrap();
    let text = format!("one{ODD_PAGE_BREAK}\ntwo{EVEN_PAGE_BREAK}\nthree{PAGE_BREAK}\nfour");
    d.story_mut(sid).unwrap().insert(0, &text);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    // Frame i is on page i + 1: "one" on 1, "two" on the next odd page (3), "three" on the next
    // even page (4), and "four" has no page left.
    let lines: Vec<(usize, usize)> = cs.frames.iter().enumerate().flat_map(|(fi, f)| f.lines.iter().map(move |l| (fi, l.para))).collect();
    assert_eq!(lines, vec![(0, 0), (2, 1), (3, 2)]);
    assert_eq!(cs.overset_at, Some(text.find("four").unwrap()));
}

#[test]
fn line_before_a_break_character_is_a_last_line() {
    // As in InDesign: column, frame and page breaks end the paragraph's last line (set with the
    // last-line alignment), while a forced line break inside a justified paragraph is justified.
    use designcraft_doc::story::{COLUMN_BREAK, FORCED_LINE_BREAK, FRAME_BREAK, PAGE_BREAK};
    for (brk, align, justified) in [
        (COLUMN_BREAK, Align::LeftJustified, false),
        (FRAME_BREAK, Align::LeftJustified, false),
        (PAGE_BREAK, Align::LeftJustified, false),
        (PAGE_BREAK, Align::FullyJustified, true),
        (FORCED_LINE_BREAK, Align::LeftJustified, true),
    ] {
        // A space after the break is skipped and does not keep the break from moving the text.
        let text = format!("one two three{brk} four five six");
        let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 400.0, 300.0), ParaAttrs { align: Some(align), ..Default::default() });
        d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let l = &cs.frames[0].lines[0];
        let full = (l.end_x - l.x1).abs() < 0.6;
        assert_eq!(full, justified, "{brk:?} {align:?}: line ends at {} of {}", l.end_x, l.x1);
        if brk == COLUMN_BREAK {
            assert_eq!(all_lines(&cs)[1].column, 1, "the text after a column break starts the next column");
        }
    }
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
fn footnote_first_line_wraps_at_the_column_edge_after_its_number() {
    // InDesign sets the footnote number and separator as part of the first line: that line
    // wraps at the same right edge as the others, whatever the separator. A tab separator
    // reaches the footnote style's tab stop.
    let tab_at = |position: f64| {
        let stop = designcraft_doc::TabStop { position, align: TabAlign::Left, leader: String::new(), align_on: String::new() };
        ParaAttrs { tabs: Some(vec![stop]), ..Default::default() }
    };
    let right_edge = |l: &Line, source: &str| {
        let space = |g: &&PlacedGlyph| g.len > 0 && source.get(g.byte..).and_then(|s| s.chars().next()).is_some_and(char::is_whitespace);
        l.glyphs.iter().filter(|g| g.visible && !space(g)).map(|g| g.x + g.adv).fold(0.0, f64::max)
    };
    let cases =
        [("\t", tab_at(100.0)), ("\t", ParaAttrs::default()), (" ", ParaAttrs::default()), (".\u{2003}\u{2003}\u{2003}", ParaAttrs::default())];
    for (sep, para) in cases {
        let (mut d, sid, _) = doc_with("Short text.", Rect::new(36.0, 36.0, 300.0, 400.0), ParaAttrs::default());
        d.footnote_options.separator = sep.into();
        d.footnote_options.start_at = 1234;
        d.story_mut(sid).unwrap().insert_note(5, &LOREM.repeat(2), ParaFormat { para, ..Default::default() });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let n = &cs.frames[0].notes[0];
        let width = n.rect.width();
        let lines = &n.text.frames[0].lines;
        assert!(lines.len() >= 3, "{sep:?}: {} lines", lines.len());
        for (i, l) in lines.iter().enumerate() {
            let right = right_edge(l, &n.source);
            assert!(right <= width + 0.01, "{sep:?}: line {i} ends at {right}, past the column's {width}");
        }
        assert!(lines[0].glyphs.first().is_some_and(|g| g.len == 0 && g.x < 1.0), "{sep:?}: the number leads line 0");
    }
    // The same holds for a tab in body text.
    let text = format!("Term\t{LOREM}");
    let (d, sid, _) = doc_with(&text, Rect::new(36.0, 36.0, 300.0, 400.0), tab_at(100.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let edge = cs.frames[0].columns[0].x1;
    assert!(right_edge(l, &text) <= edge + 0.01, "body line 0 ends at {}, past {edge}", right_edge(l, &text));
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
    check_kashida_justification(FontDb::global(), false);
}

#[test]
fn kashidas_without_tatweel_fall_back_to_spaces() {
    // A private database excludes host fonts without mutating the global database
    // used by parallel tests. Bundled Latin/Japanese fonts have no tatweel.
    let db = FontDb::with_font_dirs(Vec::new());
    db.set_system_fallback(false);
    check_kashida_justification(&db, true);
}

fn check_kashida_justification(db: &FontDb, require_fallback: bool) {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let text = "بسم الله الرحمن الرحيم الحمد لله رب العالمين الرحمن الرحيم مالك يوم الدين اياك نعبد واياك نستعين";
    let pf = ParaFormat {
        para: ParaAttrs { align: Some(Align::RightJustified), direction: Some(designcraft_doc::TextDirection::RightToLeft), ..Default::default() },
        ..Default::default()
    };
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 260.0, 400.0), lid, text, pf).unwrap();
    // The Arabic is drawn from a fallback font when the database has one.
    d.settings.glyph_fallback = true;
    let composed = |d: &Document| compose_with_db(d, d.story(sid).unwrap(), &frame_specs(d, sid), &ComposeOptions::default(), db);
    let first = |d: &Document| composed(d).frames[0].lines[0].clone();
    let with = first(&d);
    assert!(composed(&d).frames[0].lines.len() > 1);
    let lacks_tatweel = with.glyphs.iter().all(|g| g.face.glyph_for('\u{0640}') == 0);
    if require_fallback {
        assert!(lacks_tatweel);
    }
    // Spaces keep (about) their natural width; the joins took the extra length.
    let space_w = |l: &Line| l.glyphs.iter().filter(|g| g.len > 0 && text[g.byte..].starts_with(' ')).map(|g| g.adv).fold(0.0, f64::max);
    d.story_mut(sid).unwrap().paras[0].para.kashidas = Some(false);
    let without = first(&d);
    if lacks_tatweel {
        // No legal elongation can be drawn: enabling kashidas must preserve
        // the same space justification as disabling them.
        assert_eq!(with.glyphs.len(), without.glyphs.len());
        for (a, b) in with.glyphs.iter().zip(&without.glyphs) {
            assert_eq!((a.byte, a.gid, a.x, a.adv), (b.byte, b.gid, b.x, b.adv));
        }
    } else {
        assert!(space_w(&with) < space_w(&without) - 0.5, "{} {}", space_w(&with), space_w(&without));
    }
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
fn warichu_stacks_the_run_inside_the_line_and_closes_up() {
    let text = "ABCDEFGHZ";
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, text, ParaFormat::default()).unwrap();
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    let z0 = plain.iter().find(|g| g.byte == 8 && g.len > 0).unwrap().x;
    let sx0 = plain.iter().find(|g| g.byte == 0 && g.len > 0).unwrap().sx;
    d.story_mut(sid).unwrap().format_chars(0..8, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let run: Vec<_> = l.glyphs.iter().filter(|g| g.byte < 8 && g.len > 0).collect();
    assert_eq!(run.len(), 8, "the note keeps one glyph per letter");
    let size = cs.styles[run[0].style as usize].size;
    assert!((run[0].sx - sx0 * 0.5).abs() < 1e-6, "half the parent size");
    let y_of = |slice: &[&PlacedGlyph]| slice.iter().map(|g| g.y).sum::<f64>() / slice.len() as f64;
    let (y_top, y_bot) = (y_of(&run[..4]), y_of(&run[4..]));
    assert!((y_bot - y_top - size * 0.5).abs() < 0.05 * size, "two lines one small em apart: {y_top} {y_bot} {size}");
    assert!((run[0].x - run[4].x).abs() < 1e-6, "left alignment shares the start");
    let z = l.glyphs.iter().find(|g| g.byte == 8 && g.len > 0).unwrap();
    let right = run.iter().map(|g| g.x + g.adv).fold(f64::MIN, f64::max);
    assert!((z.x - right).abs() < 1e-3, "the next character follows the note: {} {right}", z.x);
    assert!(z.x < z0 - 1.0, "the note is narrower than the full-size run: {} {z0}", z.x);
    assert!((l.end_x - (z.x + z.adv)).abs() < 1e-3, "the line-end caret follows the close-up");
}

#[test]
fn warichu_break_minimum_keeps_a_short_run_on_one_line() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, "ABCD", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..4, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_lines = Some(2);
        f.over.warichu_chars_before_break = Some(3);
        f.over.warichu_chars_after_break = Some(3);
    });
    let l = &compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0];
    let run: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte < 4).collect();
    assert_eq!(run.len(), 4);
    let y0 = run[0].y;
    assert!(run.iter().all(|g| (g.y - y0).abs() < 1e-6), "not enough characters to break");
}

#[test]
fn warichu_break_minimums_apply_to_the_first_and_last_line() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, "ABCDEFGH", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..8, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_lines = Some(2);
        f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
        f.over.warichu_chars_before_break = Some(1);
        f.over.warichu_chars_after_break = Some(5);
    });
    let l = &compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0];
    let run: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte < 8).collect();
    let y0 = run[0].y;
    let first = run.iter().filter(|g| (g.y - y0).abs() < 1e-6).count();
    let second = run.len() - first;
    assert_eq!(first + second, run.len());
    assert!(first >= 1 && second >= 5, "before 1 and after 5 on eight letters: {first} {second}");
}

#[test]
fn warichu_rows_balance_by_width() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, "IIIIWWWW", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..8, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_lines = Some(2);
        f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
    });
    let l = &compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0];
    let run: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte < 8).collect();
    let y0 = run[0].y;
    let width = |pred: bool| run.iter().filter(|g| ((g.y - y0).abs() < 1e-6) == pred).map(|g| g.adv).sum::<f64>();
    let (top, bot) = (width(true), width(false));
    let widest = run.iter().map(|g| g.adv).fold(0.0_f64, f64::max);
    assert!((top - bot).abs() <= widest + 0.05, "rows differ by at most one glyph: {top} {bot}");
}

#[test]
fn warichu_negative_spacing_tightens_the_rows() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, "ABCDEFGH", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..8, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
        f.over.warichu_line_spacing = Some(-1.0);
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let run: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte < 8).collect();
    let size = cs.styles[run[0].style as usize].size;
    let gap = run[4].y - run[0].y;
    assert!((gap - (size * 0.5 - 1.0)).abs() < 0.05, "one point tighter than a small em: {gap} {size}");
}

#[test]
fn warichu_tab_leaders_stay_in_the_gap() {
    let frame = |warichu: bool| {
        let mut doc = Document::new(&designcraft_doc::build::NewDocument::default());
        let lid = doc.default_layer();
        let pf = ParaFormat {
            para: ParaAttrs {
                tabs: Some(vec![designcraft_doc::TabStop {
                    position: 200.0,
                    align: designcraft_doc::TabAlign::Right,
                    leader: ".".into(),
                    align_on: String::new(),
                }]),
                ..Default::default()
            },
            ..Default::default()
        };
        let (_, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 300.0, 100.0), lid, "Intro\tABCDEFGH", pf).unwrap();
        if warichu {
            let at = "Intro\t".len();
            doc.story_mut(sid).unwrap().format_chars(at..at + 8, |f| {
                f.over.warichu = Some(true);
                f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
            });
        }
        let cs = compose_story(&doc, sid, &ComposeOptions::default());
        let line = cs.frames[0].lines[0].clone();
        let dots: Vec<f64> = line.glyphs.iter().filter(|g| g.len == 0 && g.visible).map(|g| g.x).collect();
        (dots, line)
    };
    let (plain, _) = frame(false);
    let (noted, line) = frame(true);
    assert!(plain.len() > 5 && plain.len() == noted.len(), "the leader is not rebuilt or dropped: {} {}", plain.len(), noted.len());
    for (a, b) in plain.iter().zip(&noted) {
        assert!((a - b).abs() < 1e-6, "a leader before the note stays put: {a} {b}");
    }
    let note_x = line.glyphs.iter().filter(|g| g.len > 0 && g.byte >= "Intro\t".len()).map(|g| g.x).fold(f64::INFINITY, f64::min);
    assert!(noted.iter().all(|x| *x < note_x), "leaders stay ahead of the note");
}

#[test]
fn warichu_hit_caret_and_selection_follow_each_row() {
    let text = "ABCDEFGHZ";
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, text, ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..8, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let at = |byte: usize| l.glyphs.iter().find(|g| g.byte == byte && g.len > 0).unwrap();
    let top = at(0);
    let bot = at(4);
    let z = at(8);
    let (_, _, y0, asc, _) = caret(&cs, 0).unwrap();
    let (_, _, y4, _, _) = caret(&cs, 4).unwrap();
    let (_, x_end, y_end, _, _) = caret(&cs, text.len()).unwrap();
    assert!((y0 - (l.baseline + top.y)).abs() < 1e-6 && (y4 - (l.baseline + bot.y)).abs() < 1e-6, "caret sits on the row");
    assert!(asc < l.ascent * 0.8, "the row caret is shorter than the parent line");
    assert!((y_end - l.baseline).abs() < 1e-6 && (x_end - l.end_x).abs() < 1e-3, "the line end stays on the parent baseline");
    let click = |g: &PlacedGlyph| hit(&cs, 0, designcraft_geom::Point::new(g.x + g.adv * 0.25, l.baseline + g.y)).unwrap();
    assert_eq!(click(top), 0);
    assert_eq!(click(bot), 4);
    assert_eq!(hit(&cs, 0, designcraft_geom::Point::new(z.x + z.adv * 0.25, l.baseline)).unwrap(), 8);
    let up = adjacent_row(l, bot.x + bot.adv * 0.25, l.baseline + bot.y, true).unwrap();
    assert!(hit(&cs, 0, designcraft_geom::Point::new(bot.x + bot.adv * 0.25, up)).unwrap() < 4);
    assert!(adjacent_row(l, top.x + 0.1, l.baseline + top.y, true).is_none(), "the top row does not move up inside the note");
    assert!(adjacent_row(l, z.x + 0.1, l.baseline, false).is_none(), "the character after the note is not inside a row");
    let quads = highlight_quads(l, 0, 8, false);
    assert_eq!(quads.len(), 2, "one band per row");
    let mid_y = |q: &[designcraft_geom::Point; 4]| (q[0].y + q[2].y) / 2.0;
    assert!(mid_y(&quads[0]) < l.baseline && mid_y(&quads[1]) > l.baseline, "the bands sit on either side of the parent baseline");
    let style = &cs.styles[top.style as usize];
    assert!((rule_baseline(style, l, top) - (l.baseline + top.y)).abs() < 1e-9);
    let z_style = &cs.styles[z.style as usize];
    assert!(!z_style.warichu && (rule_baseline(z_style, l, z) - l.baseline).abs() < 1e-9);
}

#[test]
fn tate_chu_yoko_sets_digits_across_one_em() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 400.0), lid, "令和12年", ParaFormat::default()).unwrap();
    let at = "令和".len();
    d.story_mut(sid).unwrap().format_chars(at..at + 2, |f| f.over.tate_chu_yoko = Some(true));
    let glyphs = |d: &Document| compose_story(d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    // Horizontal text ignores it.
    assert!(glyphs(&d).iter().all(|g| g.tcy.is_none()));
    d.story_mut(sid).unwrap().vertical = true;
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
    // Offsets move the turned group: the Y offset along the line, the X offset across it.
    let origin = |d: &Document| {
        let cs = compose_story(d, sid, &ComposeOptions::default());
        let line = &cs.frames[0].lines[0];
        let g = line.glyphs.iter().find(|g| g.tcy.is_some()).unwrap();
        g.vertical_xf(line.baseline).unwrap() * Point::new(g.x, line.baseline + g.y)
    };
    let plain = origin(&d);
    d.story_mut(sid).unwrap().format_chars(at..at + 2, |f| {
        f.over.tate_chu_yoko_x_offset = Some(2.0);
        f.over.tate_chu_yoko_y_offset = Some(3.0);
    });
    let moved = origin(&d);
    assert!(((moved.x - plain.x) - 3.0).abs() < 1e-6 && ((moved.y - plain.y).abs() - 2.0).abs() < 1e-6, "{plain:?} {moved:?}");
}

#[test]
fn vertical_frames_compose_in_the_turned_box() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (fid, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 400.0), lid, "縦書きの文章です。Latin", ParaFormat::default()).unwrap();
    if let Some(tf) = d.item_mut(fid).and_then(|i| i.text_frame_mut()) {
        tf.options.inset = [0.0; 4];
    }
    d.story_mut(sid).unwrap().vertical = true;
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
    let xf = d.text_xf(d.item(fid).unwrap());
    let p0 = xf * designcraft_geom::Point::new(l.glyphs[0].x, l.baseline);
    let p1 = xf * designcraft_geom::Point::new(l.glyphs[3].x, l.baseline);
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
fn spanish_text_gets_spanish_hyphenation() {
    let text = "La transición democrática fue una construcción colectiva extraordinariamente compleja y desesperadamente necesaria.";
    let narrow = Rect::new(36.0, 36.0, 120.0, 700.0);
    let (mut d, sid, _) = doc_with(text, narrow, ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    let n = st.len();
    st.format_chars(0..n, |f| f.over.language = Some("Spanish".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let hy: Vec<&str> = lines.iter().filter(|l| l.hyphenated).map(|l| text[l.range.clone()].trim_end()).collect();
    assert!(!hy.is_empty(), "Spanish hyphenates");
    for l in &hy {
        let last: String = l.chars().rev().take_while(|c| c.is_alphabetic()).collect::<Vec<_>>().into_iter().rev().collect();
        let whole = text[text.find(l).unwrap() + l.len() - last.len()..].split(|c: char| !c.is_alphabetic()).next().unwrap();
        let es = hyphen::Lang::for_language("Spanish").unwrap();
        let pts = hyphen::hyphen_points_in(whole, &hyphen::Limits::default(), es);
        assert!(pts.contains(&last.chars().count()), "break {last}- in {whole} is a Bezos point");
    }
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
    let Some(face) = db.fallback_for('ب', db.face(designcraft_fonts::DEFAULT_FAMILY, "Regular").id(), None) else { return };
    if !face.covers('ُ') {
        return;
    }
    let text = "بُبَ";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 600.0, 200.0), ParaAttrs::default());
    d.settings.glyph_fallback = true;
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
    d.settings.glyph_fallback = true;
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
    d.settings.glyph_fallback = true;
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
    d.settings.glyph_fallback = true;
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
    d.settings.glyph_fallback = true;
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
    let (mut d, sid, _) = doc_with(
        text,
        Rect::new(0.0, 0.0, 180.0, 100.0),
        ParaAttrs { direction: Some(designcraft_doc::TextDirection::RightToLeft), align: Some(Align::FullyJustified), ..Default::default() },
    );
    d.settings.glyph_fallback = true;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.frames[0].lines[0].glyphs.iter().all(|g| g.gid != g.face.glyph_for('\u{0640}') || g.len > 0));
}

#[test]
fn arabic_explicit_isolated_forms_disable_contextual_joining() {
    let text = "بب";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 180.0, 100.0), ParaAttrs::default());
    d.settings.glyph_fallback = true;
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

#[test]
fn language_picks_localized_forms_through_the_word_cache() {
    // Source Serif 4 has a Turkish `locl` form of `i`; the same words in English and Turkish
    // (both shaped word by word through the cache) keep their own forms.
    let text = "in in in in";
    let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 500.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(6..text.len(), |f| f.over.language = Some("Turkish".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let gid_at = |byte: usize| all_lines(&cs).iter().flat_map(|l| l.glyphs.iter()).find(|g| g.byte == byte).map(|g| g.gid).unwrap();
    assert_eq!(gid_at(0), gid_at(3), "English i");
    assert_eq!(gid_at(6), gid_at(9), "Turkish i");
    assert_ne!(gid_at(0), gid_at(6), "Turkish i takes its localized form");
    // A single word is shaped directly, in its language too.
    let (mut d, sid, _) = doc_with("i", Rect::new(36.0, 36.0, 500.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.language = Some("Turkish".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(all_lines(&cs)[0].glyphs[0].gid, gid_at(6), "Turkish i, shaped on its own");
}

#[test]
fn chinese_and_japanese_take_their_localized_forms() {
    // Noto Sans CJK SC (craft-fonts) draws 直 differently for Japanese (`locl`).
    let Some(noto) = designcraft_fonts::CRAFT_FONTS.iter().find(|f| f.family == "Noto Sans CJK SC") else { return };
    FontDb::global().add_font(noto.bytes.to_vec());
    let gid = |language: &str| {
        let (mut d, sid, _) = doc_with("直", Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
        d.story_mut(sid).unwrap().format_chars(0.."直".len(), |f| {
            f.over.font_family = Some(noto.family.into());
            f.over.language = Some(language.into());
        });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let g = &all_lines(&cs)[0].glyphs[0];
        assert_eq!(g.face.family, noto.family);
        g.gid
    };
    let chinese = gid("Chinese: Simplified");
    assert_eq!(chinese, FontDb::global().face(noto.family, noto.style).glyph_for('直'));
    assert_ne!(gid("Japanese"), chinese, "Japanese 直");
}

#[test]
fn cjk_fallback_follows_the_language() {
    use designcraft_fonts::testing::font_with;
    // Stand-ins for Hiragino Mincho ProN (the Japanese chain's first system font) and Songti SC
    // (first in the Simplified Chinese chain); the real ones when installed.
    let db = designcraft_fonts::FontDb::global();
    db.add_font(font_with("Hiragino Mincho ProN", &['直']).unwrap());
    db.add_font(font_with("Songti SC", &['直']).unwrap());
    // (face of the Latin letter, face of the ideograph)
    let faces_of = |language: &str| {
        let (mut d, sid, _) = doc_with("a直", Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
        d.settings.glyph_fallback = true;
        d.story_mut(sid).unwrap().format_chars(0..4, |f| f.over.language = Some(language.into()));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let face_at = |byte: usize| all_lines(&cs).iter().flat_map(|l| l.glyphs.iter()).find(|g| g.byte == byte).map(|g| g.face).unwrap();
        (face_at(0), face_at(1))
    };
    assert_eq!(faces_of("Chinese: Simplified").1.family, "Songti SC");
    // Japanese: the craft-fonts faces lead the chain (Mincho first), then the system fonts.
    let japanese = designcraft_fonts::japanese_document_fonts().first().map_or("Hiragino Mincho ProN", |f| f.family);
    assert_eq!(faces_of("Japanese").1.family, japanese);
    // Without a CJK language, the language-blind search.
    let (primary, han) = faces_of("English: USA");
    assert_eq!(Some(han.family.clone()), db.fallback_for('直', primary.id(), None).map(|f| f.family.clone()));
}

#[test]
fn missing_glyphs_are_the_fonts_box_unless_fallback_is_on() {
    use designcraft_fonts::testing::font_with;
    // Some font draws 語 when fallback fonts are allowed (a system CJK font, or this stand-in).
    designcraft_fonts::FontDb::global().add_font(font_with("DC Test Missing Glyph Helper", &['語']).unwrap());
    let text = "a語";
    let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.language = Some("Japanese".into()));
    let glyph_at = |d: &Document, byte: usize| {
        let cs = compose_story(d, sid, &ComposeOptions::default());
        all_lines(&cs).iter().flat_map(|l| l.glyphs.iter()).find(|g| g.byte == byte).map(|g| (g.face, g.gid, g.adv)).unwrap()
    };
    assert!(!d.settings.glyph_fallback, "new documents draw missing glyphs as InDesign does");
    let (face, gid, adv) = glyph_at(&d, 1);
    assert_eq!((face.family.as_str(), gid), (designcraft_fonts::DEFAULT_FAMILY, 0), "Source Serif 4's .notdef");
    assert!((adv - face.advance(0) * 12.0 / face.upem).abs() < 1e-6, "its advance: {adv}");
    d.settings.glyph_fallback = true;
    let (face, gid, _) = glyph_at(&d, 1);
    assert_ne!(face.family, designcraft_fonts::DEFAULT_FAMILY);
    assert_ne!(gid, 0, "a fallback font draws it");
}

#[test]
fn kenten_missing_from_the_font_are_its_box_unless_fallback_is_on() {
    use designcraft_fonts::testing::font_with;
    // Some font draws the sesame dot when fallback fonts are allowed (a system font, or this one).
    designcraft_fonts::FontDb::global().add_font(font_with("DC Test Kenten Helper", &['\u{FE45}']).unwrap());
    let (mut d, sid, _) = doc_with("ab", Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.len();
    d.story_mut(sid).unwrap().format_chars(0..2, |f| f.over.kenten = Some(true));
    let marks = |d: &Document| {
        let cs = compose_story(d, sid, &ComposeOptions::default());
        cs.frames[0].lines[0].glyphs[plain..].iter().map(|g| (g.face.family.clone(), g.gid)).collect::<Vec<_>>()
    };
    let missing = marks(&d);
    assert_eq!(missing.len(), 2);
    assert!(missing.iter().all(|(family, gid)| family == designcraft_fonts::DEFAULT_FAMILY && *gid == 0), "{missing:?}");
    d.settings.glyph_fallback = true;
    let drawn = marks(&d);
    assert_eq!(drawn.len(), 2);
    assert!(drawn.iter().all(|(family, gid)| family != designcraft_fonts::DEFAULT_FAMILY && *gid != 0), "{drawn:?}");
}

#[test]
fn each_document_shapes_words_in_its_own_fonts() {
    use designcraft_fonts::testing::font_with_glyph;
    const FAMILY: &str = "DocFont Word Cache";
    // Two documents' fonts of one name: the same words, drawn as `X` in one and `O` in the other.
    let root = std::env::temp_dir().join(format!("dc-compose-docfonts-{}", std::process::id()));
    let mut scopes = Vec::new();
    for (name, glyph) in [("x", 'X'), ("o", 'O')] {
        let folder = root.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("word.ttf"), font_with_glyph(FAMILY, &['a', 'b', ' '], glyph).unwrap()).unwrap();
        scopes.push((designcraft_fonts::FontDb::global().load_document_fonts(&folder).scope, glyph));
    }
    let text = "ab ab ab ab";
    let mut faces = Vec::new();
    for (scope, glyph) in scopes {
        let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
        d.font_scope = scope;
        d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.font_family = Some(FAMILY.into()));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let glyphs: Vec<&PlacedGlyph> =
            all_lines(&cs).into_iter().flat_map(|l| l.glyphs.iter()).filter(|g| text[g.byte..].starts_with('a')).collect();
        assert_eq!(glyphs.len(), 4);
        let face = glyphs[0].face;
        let drawn = designcraft_fonts::FontDb::global().face(designcraft_fonts::FALLBACK_FAMILY, "Regular").glyph_for(glyph);
        assert!(glyphs.iter().all(|g| g.face == face && g.gid == drawn), "scope {scope}: {glyph}");
        faces.push(face);
    }
    assert!(faces[0] != faces[1] && faces.iter().all(|f| f.family == FAMILY));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn composite_fonts_draw_from_the_documents_fonts() {
    use designcraft_doc::cjk::{CompositeFont, CompositeFontEntry};
    use designcraft_fonts::testing::font_with_glyph;
    const FAMILY: &str = "DocFont Composite Member";
    // A composite font whose digits come from a font only the document has.
    let folder = std::env::temp_dir().join(format!("dc-compose-composite-docfont-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("digits.ttf"), font_with_glyph(FAMILY, &['1'], 'X').unwrap()).unwrap();
    let scope = FontDb::global().load_document_fonts(&folder).scope;
    let (mut d, sid, _) = doc_with("A1B", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    d.font_scope = scope;
    std::sync::Arc::make_mut(&mut d.styles).composite_fonts.push(CompositeFont {
        name: "Mixed".into(),
        entries: vec![
            CompositeFontEntry { family: designcraft_fonts::DEFAULT_FAMILY.into(), ..Default::default() },
            CompositeFontEntry { characters: "0123456789".into(), family: FAMILY.into(), ..Default::default() },
        ],
    });
    d.story_mut(sid).unwrap().format_chars(0..3, |f| f.over.font_family = Some("CompositeFont/Mixed".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let digit = cs.frames[0].lines[0].glyphs.iter().find(|g| g.byte == 1).unwrap();
    assert_eq!(digit.face.family, FAMILY);
    assert_ne!(digit.gid, 0);
    assert!(!cs.styles.iter().any(|s| s.missing_font), "the document's font isn't missing");
    let _ = std::fs::remove_dir_all(&folder);
}

const KO_WORDS: &str = "한국어 문장은 띄어쓰기 단위로 줄을 바꿉니다 한국어 문장은 띄어쓰기 단위로 줄을 바꿉니다";
const KO_HANJA: &str = "大韓民國 憲法은 國民의 權利를 保障한다 大韓民國 憲法은 國民의 權利를 保障한다";
const KO_PUNCT: &str = "가나다라마。바사아자차」카타파하가、나다라마바。사아자차카」타파하가나、다라마바사。아자차카타」파하가나다、";
/// Syllables for words wider than the frame.
const KO_LONG: &str = "가나다라마바사아자차카타파하」";

/// A Korean paragraph in a narrow frame (70 pt), set in a stand-in font covering the samples.
fn korean_doc(text: &str, language: &str, para: ParaAttrs) -> (Document, StoryId) {
    let chars: Vec<char> = [KO_WORDS, KO_HANJA, KO_PUNCT, KO_LONG].concat().chars().collect();
    let font = designcraft_fonts::testing::font_with("DC Test Korean", &chars).unwrap();
    designcraft_fonts::FontDb::global().add_font(font);
    let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 106.0, 400.0), para);
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.font_family = Some("DC Test Korean".into());
        f.over.language = Some(language.into());
    });
    (d, sid)
}

/// The lines (after the first) that start inside a space-separated word.
fn lines_starting_mid_word(d: &Document, sid: StoryId, text: &str) -> Vec<String> {
    let cs = compose_story(d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.len() > 2, "wraps: {} lines", lines.len());
    lines.iter().skip(1).filter(|l| !text[..l.range.start].ends_with(' ')).map(|l| text[l.range.clone()].to_string()).collect()
}

#[test]
fn korean_breaks_at_spaces() {
    for composer in [designcraft_doc::Composer::Paragraph, designcraft_doc::Composer::SingleLine] {
        for align in [Align::Left, Align::LeftJustified] {
            let para = ParaAttrs { composer: Some(composer), align: Some(align), ..Default::default() };
            // By script: Hangul is set word by word whatever the language.
            for language in ["Korean", "English: USA"] {
                let (d, sid) = korean_doc(KO_WORDS, language, para.clone());
                let mid = lines_starting_mid_word(&d, sid, KO_WORDS);
                assert!(mid.is_empty(), "{composer:?} {align:?} {language}: {mid:?}");
            }
        }
    }
}

#[test]
fn korean_breaks_at_spaces_under_a_kinsoku_set() {
    let set = designcraft_doc::cjk::Kinsoku::named("KoreanKinsoku");
    assert!(set.is_some());
    for composer in [designcraft_doc::Composer::Paragraph, designcraft_doc::Composer::SingleLine] {
        let para = ParaAttrs { composer: Some(composer), kinsoku: Some(set.clone()), ..Default::default() };
        let (d, sid) = korean_doc(KO_WORDS, "Korean", para.clone());
        let mid = lines_starting_mid_word(&d, sid, KO_WORDS);
        assert!(mid.is_empty(), "{composer:?}: {mid:?}");
        let (d, sid) = korean_doc(KO_WORDS, "Korean", ParaAttrs { korean_char_breaks: Some(true), ..para });
        assert!(!lines_starting_mid_word(&d, sid, KO_WORDS).is_empty(), "{composer:?}: breaks between syllables on request");
    }
}

#[test]
fn korean_character_breaks_on_request() {
    let para = ParaAttrs { korean_char_breaks: Some(true), composer: Some(designcraft_doc::Composer::SingleLine), ..Default::default() };
    let (d, sid) = korean_doc(KO_WORDS, "Korean", para);
    assert!(!lines_starting_mid_word(&d, sid, KO_WORDS).is_empty(), "breaks between syllables");
}

#[test]
fn korean_by_its_locale_code_breaks_at_spaces() {
    let para = ParaAttrs { composer: Some(designcraft_doc::Composer::SingleLine), ..Default::default() };
    let (d, sid) = korean_doc(KO_HANJA, "ko_KR", para);
    let mid = lines_starting_mid_word(&d, sid, KO_HANJA);
    assert!(mid.is_empty(), "{mid:?}");
}

#[test]
fn hanja_in_korean_text_breaks_at_spaces() {
    let para = ParaAttrs { composer: Some(designcraft_doc::Composer::SingleLine), ..Default::default() };
    let (d, sid) = korean_doc(KO_HANJA, "Korean", para.clone());
    let mid = lines_starting_mid_word(&d, sid, KO_HANJA);
    assert!(mid.is_empty(), "{mid:?}");
    // Han in Japanese text breaks between characters.
    let (d, sid) = korean_doc(KO_HANJA, "Japanese", para);
    assert!(!lines_starting_mid_word(&d, sid, KO_HANJA).is_empty());
}

#[test]
fn korean_lines_keep_kinsoku() {
    // Words of 9 to 14 syllables and a closing bracket: in one of them the bracket is what
    // overflows the frame.
    let syllables: Vec<char> = KO_LONG.chars().collect();
    let long = (9..=14).map(|n| format!("{}」 가나", syllables[..n].iter().collect::<String>()));
    for text in std::iter::once(KO_PUNCT.to_string()).chain(long) {
        let text = text.as_str();
        for char_breaks in [false, true] {
            for composer in [designcraft_doc::Composer::Paragraph, designcraft_doc::Composer::SingleLine] {
                let para = ParaAttrs { korean_char_breaks: Some(char_breaks), composer: Some(composer), ..Default::default() };
                let (d, sid) = korean_doc(text, "Korean", para);
                let cs = compose_story(&d, sid, &ComposeOptions::default());
                let lines = all_lines(&cs);
                assert!(lines.len() > 1, "wraps");
                let texts: Vec<&str> = lines.iter().map(|l| &text[l.range.clone()]).collect();
                assert!(texts.iter().all(|t| !t.starts_with(['、', '。', '」'])), "kinsoku ({char_breaks}, {composer:?}): {texts:?}");
            }
        }
    }
}

/// A synthetic font for vertical text: Source Sans 3 glyphs (CJK characters drawn as Latin ones)
/// with vertical metrics, every glyph one em down the line and 120 units below its vertical
/// origin, except ヸ (drawn as O): 1.024 em. No vertical forms. Added to the global database once.
fn vertical_test_font() -> &'static str {
    use designcraft_fonts::testing::{font_mapping, with_vmtx};
    const FAMILY: &str = "DC Test Vertical";
    static ADDED: std::sync::Once = std::sync::Once::new();
    ADDED.call_once(|| {
        let glyphs = [('一', 'X'), ('二', 'X'), ('日', 'X'), ('本', 'X'), ('ま', 'o'), ('ヸ', 'O'), ('、', ','), ('A', 'A'), ('b', 'b')];
        let base = font_mapping(FAMILY, &glyphs).unwrap();
        let scratch = designcraft_fonts::FontDb::with_font_dirs(vec![]);
        scratch.add_font(base.clone());
        let wi = u16::try_from(scratch.face(FAMILY, "Regular").glyph_for('ヸ')).unwrap();
        designcraft_fonts::FontDb::global().add_font(with_vmtx(&base, (1000, 120), &[(wi, 1024, 100)]).unwrap());
    });
    FAMILY
}

/// Shippori Mincho (vertical forms, `vpal`) when built with craft-fonts; else `None`, with a note.
fn shippori() -> Option<&'static str> {
    if !designcraft_fonts::japanese_fonts().any(|f| f.family == "Shippori Mincho") {
        eprintln!("skipped Shippori Mincho: built without craft-fonts (set CRAFT_FONTS_DIR to a checkout)");
        return None;
    }
    Some("Shippori Mincho")
}

/// The fonts vertical layout is checked in: the synthetic one, and Shippori Mincho with craft-fonts.
fn vertical_families() -> Vec<&'static str> {
    std::iter::once(vertical_test_font()).chain(shippori()).collect()
}

/// A vertical frame (no inset) with `text` in `family` at 20 pt (and `over`), and its first line
/// with the frame's first column.
fn vertical_line(family: &str, text: &str, over: impl Fn(&mut designcraft_doc::CharFormat)) -> (Line, Rect) {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 500.0), lid, text, ParaFormat::default()).unwrap();
    if let Some(tf) = d.item_mut(fid).and_then(|i| i.text_frame_mut()) {
        tf.options.inset = [0.0; 4];
    }
    d.story_mut(sid).unwrap().vertical = true;
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.font_family = Some(family.into());
        f.over.size = Some(20.0);
        over(f);
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = &cs.frames[0];
    assert!(ft.vertical);
    let line = ft.lines[0].clone();
    assert!(line.glyphs.iter().all(|g| g.face.family == family), "{family}: {:?}", line.glyphs.iter().map(|g| &g.face.family).collect::<Vec<_>>());
    (line, ft.columns[0])
}

/// Text-space bounds of a glyph's ink as drawn in a vertical frame.
fn vertical_ink(g: &PlacedGlyph, baseline: f64) -> Rect {
    use designcraft_geom::{Affine, Shape};
    let ink = designcraft_fonts::FontDb::global().outline(&g.face, g.gid).bounding_box();
    let mut a = Affine::translate((g.x, baseline + g.y)) * Affine::scale_non_uniform(g.sx, g.sy);
    if let Some(turn) = g.vertical_xf(baseline) {
        a = turn * a;
    }
    a.transform_rect_bbox(ink)
}

/// The em box in text space: along the line from x (page down), across from its top (text -y,
/// page right) to its bottom → (right, left, centre).
fn em_across(l: &Line, g: &PlacedGlyph) -> (f64, f64, f64) {
    let (top, bottom) = g.face.em_box();
    let right = l.baseline + g.y - top * g.sy;
    let left = l.baseline + g.y - bottom * g.sy;
    (right, left, (right + left) / 2.0)
}

#[test]
fn upright_glyphs_hang_on_the_line_centre() {
    for family in vertical_families() {
        let (l, _) = vertical_line(family, "一", |_| {});
        let ichi = &l.glyphs[0];
        assert!(ichi.upright, "{family}");
        // An ideograph is centred across the line and inside its em along it.
        let (_, _, centre) = em_across(&l, ichi);
        let ink = vertical_ink(ichi, l.baseline);
        assert!(((ink.y0 + ink.y1) / 2.0 - centre).abs() < 0.05 * 20.0, "{family}: {ink:?} {centre}");
        assert!(ink.x0 > ichi.x && ink.x1 < ichi.x + ichi.adv, "{family}: {ink:?}");
    }
    // The synthetic glyph's top is its top side bearing (120 units) below its vertical origin.
    let (l, _) = vertical_line(vertical_test_font(), "一", |_| {});
    let ink = vertical_ink(&l.glyphs[0], l.baseline);
    assert!((ink.x0 - l.glyphs[0].x - 2.4).abs() < 1e-6, "{ink:?} {}", l.glyphs[0].x);
}

#[test]
fn upright_punctuation_takes_its_vertical_form_and_place() {
    // A font with vertical forms: Shippori Mincho.
    let Some(family) = shippori() else { return };
    let (l, _) = vertical_line(family, "一、", |_| {});
    let comma = &l.glyphs[1];
    assert!(comma.upright);
    assert_ne!(comma.gid, comma.face.glyph_for('、'), "vertical form (vert)");
    let (right, _, centre) = em_across(&l, comma);
    // The comma's ink sits in the upper right quarter of its em box on the page.
    let ink = vertical_ink(comma, l.baseline);
    assert!(ink.x0 >= comma.x - 1e-6 && ink.x1 < comma.x + comma.adv / 2.0, "upper half: {ink:?} {} {}", comma.x, comma.adv);
    assert!(ink.y0 >= right - 1e-6 && ink.y1 < centre, "right half: {ink:?} {right} {centre}");
}

#[test]
fn latin_in_vertical_text_turns_as_a_run() {
    // Shippori Mincho's `vrt2` has turned Latin forms; a turned run must not use them.
    for family in vertical_families() {
        let (l, _) = vertical_line(family, "日本Ab", |_| {});
        let latin: Vec<_> = l.glyphs.iter().filter(|g| g.byte >= "日本".len()).collect();
        let face = latin[0].face;
        assert_eq!(latin.iter().map(|g| g.gid).collect::<Vec<_>>(), [face.glyph_for('A'), face.glyph_for('b')], "{family}");
        assert!(latin.iter().all(|g| !g.upright && g.vertical_xf(l.baseline).is_none()), "{family}");
        assert!((latin[0].adv - face.advance(latin[0].gid) * latin[0].sx).abs() < 1e-9, "{family}: horizontal advance");
    }
}

#[test]
fn upright_glyphs_advance_by_their_vertical_metrics() {
    // ヸ is 1.024 em tall in both fonts.
    for family in vertical_families() {
        let (l, _) = vertical_line(family, "一ヸ一", |_| {});
        let g = &l.glyphs;
        assert_eq!(g[1].face.v_advance(g[1].gid), 1024.0, "{family}");
        assert!((g[1].x - g[0].x - 20.0).abs() < 1e-9, "{family}");
        assert!((g[2].x - g[1].x - 20.48).abs() < 1e-9, "{family}: {} {}", g[1].x, g[2].x);
    }
    let Some(family) = shippori() else { return };
    // Shippori's ヸ is an em wide.
    let (l, _) = vertical_line(family, "ヸ", |_| {});
    assert_eq!(l.glyphs[0].face.advance(l.glyphs[0].gid), 1000.0);
    // Proportional vertical metrics (`vpal`): ま takes 0.974 em.
    let (l, _) = vertical_line(family, "まま", |f| f.over.otf_features = Some(vec!["vpal".into()]));
    assert!((l.glyphs[1].x - l.glyphs[0].x - 19.48).abs() < 1e-9, "{:?}", l.glyphs.iter().map(|g| g.x).collect::<Vec<_>>());
    let (l, _) = vertical_line(family, "まま", |_| {});
    assert!((l.glyphs[1].x - l.glyphs[0].x - 20.0).abs() < 1e-9);
}

#[test]
fn vertical_lines_fit_the_em_box() {
    // First baseline at the ascent: an upright line's ascent is its em box top, so the em box
    // touches the frame's right edge. The synthetic font's em box is Source Sans 3's BASE one
    // (0.83 em, -0.17 em); Shippori Mincho's its OS/2 typo metrics (0.88 em, -0.12 em).
    let expected = [(vertical_test_font(), 16.6, 3.4)].into_iter().chain(shippori().map(|f| (f, 17.6, 2.4)));
    for (family, ascent, descent) in expected {
        let (l, col) = vertical_line(family, "一二", |_| {});
        assert!((l.ascent - ascent).abs() < 1e-9 && (l.descent - descent).abs() < 1e-9, "{family}: {} {}", l.ascent, l.descent);
        assert!((l.baseline - col.y0 - ascent).abs() < 1e-9, "{family}: {} {}", l.baseline, col.y0);
    }
}

/// Lines of `text` set in a frame `width` wide, with `tracking` on every character.
fn tracked_lines(text: &str, width: f64, para: ParaAttrs, tracking: f64) -> Vec<Line> {
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, width, 4000.0), para);
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.tracking = Some(tracking));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset(), "overset at width {width}");
    all_lines(&cs).into_iter().cloned().collect()
}

#[test]
fn a_word_longer_than_the_line_breaks_at_the_column_edge() {
    use designcraft_doc::Composer;
    let word = "n".repeat(60);
    let long = format!("Then {word} and on");
    for tracking in [0.0, 740.0] {
        // Natural advances, from one unbroken line.
        let natural: HashMap<usize, f64> =
            tracked_lines(&long, 1e5, ParaAttrs::default(), tracking)[0].glyphs.iter().map(|g| (g.byte, g.adv)).collect();
        for composer in [Composer::Paragraph, Composer::SingleLine] {
            for align in [Align::Left, Align::LeftJustified] {
                let para = ParaAttrs { composer: Some(composer), align: Some(align), hyphenate: Some(false), ..Default::default() };
                let lines = tracked_lines(&long, 72.0, para, tracking);
                let what = format!("{composer:?} {align:?} tracking {tracking}");
                assert!(lines.len() >= 4, "{what}: {} lines", lines.len());
                for l in &lines {
                    let ink: Vec<_> = l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).collect();
                    let right = ink.iter().map(|g| g.x + g.adv.min(natural[&g.byte])).fold(l.x0, f64::max);
                    assert!(l.end_x <= l.x1 + 0.02 && right <= l.x1 + 0.02, "{what}: line {:?} ends at {} past {}", l.range, l.end_x, l.x1);
                    for w in ink.windows(2) {
                        assert!(w[1].x >= w[0].x + natural[&w[0].byte] - 1e-6, "{what}: glyphs overlap at {}", w[1].byte);
                    }
                }
            }
        }
    }
}

#[test]
fn a_line_that_exactly_fills_the_measure_fits() {
    use designcraft_doc::Composer;
    let text = "#knowyourplastic";
    let one = &tracked_lines(text, 1e5, ParaAttrs::default(), 0.0)[0];
    // The measure a rounding error short of the natural width.
    let width = one.end_x - one.x0 - 0.005;
    for composer in [Composer::Paragraph, Composer::SingleLine] {
        for align in [Align::Left, Align::LeftJustified] {
            let para = ParaAttrs { composer: Some(composer), align: Some(align), ..Default::default() };
            let lines = tracked_lines(text, width, para, 0.0);
            assert_eq!(lines.len(), 1, "{composer:?} {align:?}: {:?}", lines.iter().map(|l| l.range.clone()).collect::<Vec<_>>());
        }
    }
}

#[test]
fn justified_line_with_a_tab_justifies_the_text_after_its_last_tab() {
    // InDesign justifies only the text after a line's last (left) tab; tab stops stay aligned.
    let stop = |align| designcraft_doc::TabStop { position: 100.0, align, leader: String::new(), align_on: String::new() };
    let text = "Name\t42 and some words\u{2028}more";
    let x_of = |l: &Line, byte: usize| l.glyphs.iter().find(|g| g.byte == byte && g.len > 0).map(|g| g.x).unwrap();
    let left = ParaAttrs { align: Some(Align::LeftJustified), tabs: Some(vec![stop(TabAlign::Left)]), ..Default::default() };
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), left.clone());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = all_lines(&cs)[0];
    assert!((l.end_x - l.x1).abs() < 0.6, "line with a left tab ends at {} not {}", l.end_x, l.x1);
    assert!((x_of(l, 5) - 100.0).abs() < 0.5, "text after the tab starts at the stop: {}", x_of(l, 5));
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs { align: Some(Align::Left), ..left });
    let ragged = compose_story(&d, sid, &ComposeOptions::default());
    let r = all_lines(&ragged)[0];
    for byte in 0..4 {
        assert!((x_of(l, byte) - x_of(r, byte)).abs() < 1e-6, "text before the tab keeps its natural position");
    }

    // After a right tab the line stays as set.
    let right = ParaAttrs { align: Some(Align::LeftJustified), tabs: Some(vec![stop(TabAlign::Right)]), ..Default::default() };
    let (d, sid, _) = doc_with("Name\t42\u{2028}more", Rect::new(0.0, 0.0, 300.0, 100.0), right);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = all_lines(&cs)[0];
    assert!((l.end_x - 100.0).abs() < 0.5, "right-tab line ends at its stop: {}", l.end_x);

    // A footnote's number and tab separator: its first line is justified like the others.
    let (mut d, sid, fid) = doc_with("Body text.", Rect::new(0.0, 0.0, 200.0, 400.0), ParaAttrs::default());
    let note = ParaFormat { para: ParaAttrs { align: Some(Align::LeftJustified), ..Default::default() }, ..Default::default() };
    d.story_mut(sid).unwrap().insert_note(4, LOREM, note);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let n = &cs.frame(fid).unwrap().notes[0];
    let lines = all_lines(&n.text);
    assert!(lines.len() > 2);
    for l in &lines[..lines.len() - 1] {
        assert!((l.end_x - l.x1).abs() < 0.6, "footnote line ends at {} not {}", l.end_x, l.x1);
    }
    assert!((x_of(lines[0], 0) - 36.0).abs() < 0.5, "note text starts at the default tab stop: {}", x_of(lines[0], 0));
}

#[test]
fn list_first_line_wraps_at_the_column_edge_after_its_label() {
    use designcraft_doc::{ListType, TabStop};
    // A hanging indent (left 18, first line −18): the label sits at the column start, a tab
    // after it reaches the left indent (an implicit stop when no explicit stop comes first), and
    // the first line wraps at the same right edge as the others.
    let right_edge = |l: &Line, source: &str| {
        let space = |g: &&PlacedGlyph| g.len > 0 && source.get(g.byte..).and_then(|s| s.chars().next()).is_some_and(char::is_whitespace);
        l.glyphs.iter().filter(|g| g.visible && !space(g)).map(|g| g.x + g.adv).fold(0.0, f64::max)
    };
    let text_start = |l: &Line| l.glyphs.iter().find(|g| g.len > 0 && g.visible).map_or(f64::NAN, |g| g.x);
    let stop = |position: f64| TabStop { position, align: TabAlign::Left, leader: String::new(), align_on: String::new() };
    let text = format!("{LOREM}\n{LOREM}");
    // (list, separator, explicit tab stops, where line 0's text starts past the column start:
    // None = right after the label, wherever that is).
    let cases = [
        (ListType::Bullets, "\t", None, Some(18.0)),
        (ListType::Numbers, "\t", None, Some(18.0)),
        (ListType::Numbers, "\t", Some(vec![stop(12.0)]), Some(12.0)),
        (ListType::Numbers, "\t", Some(vec![stop(30.0)]), Some(18.0)),
        (ListType::Bullets, " ", None, None),
        (ListType::Numbers, "\u{2003}", None, None),
    ];
    for (list, sep, tabs, first_at) in cases {
        let para = ParaAttrs {
            list_type: Some(list),
            list_separator: Some(sep.into()),
            left_indent: Some(18.0),
            first_line_indent: Some(-18.0),
            tabs: tabs.clone(),
            ..Default::default()
        };
        let (d, sid, _) = doc_with(&text, Rect::new(36.0, 36.0, 300.0, 400.0), para);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let col = cs.frames[0].columns[0];
        let lines = &cs.frames[0].lines;
        let what = format!("{list:?} {sep:?} {tabs:?}");
        assert!(lines.len() >= 6, "{what}: {} lines", lines.len());
        for (i, l) in lines.iter().enumerate() {
            let right = right_edge(l, &text);
            assert!(right <= col.x1 + 0.01, "{what}: line {i} ends at {right}, past the column's {}", col.x1);
            let start = text_start(l) - col.x0;
            if !l.first_in_para {
                assert!((start - 18.0).abs() < 0.01, "{what}: line {i} text starts at {start}, not the left indent");
            } else if let Some(at) = first_at {
                assert!((start - at).abs() < 0.01, "{what}: line {i} text starts at {start}, not {at}");
            } else {
                assert!(start > 0.0, "{what}: line {i} text starts at {start}");
            }
            if l.first_in_para {
                assert!(l.glyphs.first().is_some_and(|g| g.len == 0 && (g.x - col.x0).abs() < 0.01), "{what}: the label leads line {i}");
            }
        }
    }
}

fn ruled(text: &str, rect: Rect, columns: u32, edit: impl FnOnce(&mut Document, StoryId)) -> (Document, ItemId, Vec<Rect>) {
    let (mut d, sid, fid) = doc_with(text, rect, ParaAttrs::default());
    {
        let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
        o.columns = columns;
        o.gutter = 12.0;
        o.inset = [10.0, 6.0, 20.0, 6.0];
        o.column_rule = true;
        o.column_rule_weight = 2.0;
        o.column_rule_color = "[Black]".into();
    }
    edit(&mut d, sid);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let rules = cs.frames[0].decos.iter().filter(|dc| dc.color == "[Black]").map(|dc| dc.rect).collect();
    (d, fid, rules)
}

#[test]
fn column_rules_sit_in_the_gutters_inside_the_insets() {
    // Two columns in 0..300 with 6 pt side insets: the gutter is centred on 150.
    let (_, _, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 2, |_, _| {});
    assert_eq!(rules.len(), 1, "{rules:?}");
    let r = rules[0];
    assert!((r.center().x - 150.0).abs() < 1e-9 && (r.width() - 2.0).abs() < 1e-9, "{r:?}");
    assert!((r.y0 - 10.0).abs() < 1e-9 && (r.y1 - 180.0).abs() < 1e-9, "{r:?}");
    // Three columns: one rule per gutter, each centred between its two columns.
    let (_, _, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 3, |_, _| {});
    assert_eq!(rules.len(), 2, "{rules:?}");
    let w = (288.0 - 24.0) / 3.0;
    for (i, r) in rules.iter().enumerate() {
        let edge = 6.0 + w * (i + 1) as f64 + 12.0 * i as f64;
        assert!((r.center().x - (edge + 6.0)).abs() < 1e-9, "{i}: {r:?}");
    }
    // Off, or a single column: no rule.
    let (_, _, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 1, |_, _| {});
    assert!(rules.is_empty());
    let (_, _, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 2, |d, sid| {
        let fid = d.story(sid).unwrap().frames[0];
        d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.column_rule = false;
    });
    assert!(rules.is_empty());
}

#[test]
fn column_rules_centre_on_unequal_gutters_and_ignore_bad_weights() {
    let cols = [Rect::new(200.0, 0.0, 300.0, 50.0), Rect::new(0.0, 0.0, 100.0, 50.0), Rect::new(110.0, 0.0, 180.0, 50.0)];
    let weight = |w: f64| TextFrameOptions { column_rule_weight: w, ..Default::default() };
    let rules = column_rule_rects(&cols, &[], &weight(1.0));
    let centres: Vec<f64> = rules.iter().map(|r| r.center().x).collect();
    assert_eq!(centres, [105.0, 190.0]);
    assert!(column_rule_rects(&cols, &[], &weight(f64::NAN)).is_empty());
    assert!(column_rule_rects(&cols, &[], &weight(-1.0)).is_empty());
    assert!((column_rule_rects(&cols, &[], &weight(1e12))[0].width() - 1000.0).abs() < 1e-9);
}

#[test]
fn column_rules_in_right_to_left_and_vertical_frames() {
    let (_, _, rules) = ruled("نص", Rect::new(0.0, 0.0, 300.0, 200.0), 2, |d, sid| {
        d.story_mut(sid).unwrap().direction = designcraft_doc::TextDirection::RightToLeft;
    });
    assert_eq!(rules.len(), 1);
    assert!((rules[0].center().x - 150.0).abs() < 1e-9, "{rules:?}");
    // Vertical type: the columns stack top to bottom, so the rule runs across the frame.
    let (d, fid, rules) = ruled("縦書き", Rect::new(100.0, 100.0, 400.0, 300.0), 2, |d, sid| d.story_mut(sid).unwrap().vertical = true);
    assert_eq!(rules.len(), 1);
    let r = d.text_xf(d.item(fid).unwrap()).transform_rect_bbox(rules[0]);
    // Text area: x 106..394, y 110..280; the gutter is centred on y 195.
    assert!((r.x0 - 106.0).abs() < 1e-6 && (r.x1 - 394.0).abs() < 1e-6, "{r:?}");
    assert!((r.center().y - 195.0).abs() < 1e-6 && (r.height() - 2.0).abs() < 1e-6, "{r:?}");
}

#[test]
fn column_rules_break_around_paragraphs_that_span_columns() {
    let text = format!("A heading that spans both columns\n{LOREM} {LOREM}");
    let (d, fid, rules) = ruled(&text, Rect::new(0.0, 0.0, 300.0, 400.0), 2, |d, sid| {
        d.story_mut(sid).unwrap().paras[0].para.span_columns = Some(designcraft_doc::SpanColumns::Span(0));
    });
    let sid = d.item(fid).unwrap().text_frame().unwrap().story;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let span: Vec<&Line> = cs.frames[0].lines.iter().filter(|l| l.para == 0).collect();
    assert!(!span.is_empty() && span.iter().all(|l| l.x0 < 150.0 && l.x1 > 150.0), "the heading spans");
    let (top, bottom) = (span[0].baseline - span[0].ascent, span[span.len() - 1].baseline + span[span.len() - 1].descent);
    assert!(!rules.is_empty());
    for r in &rules {
        assert!(r.y1 <= top + 1e-9 || r.y0 >= bottom - 1e-9, "rule {r:?} cuts the span {top}..{bottom}");
    }
    // The rule resumes below the span and runs to the bottom of the columns.
    assert!(rules.iter().any(|r| (r.y0 - bottom).abs() < 1e-9 && (r.y1 - 380.0).abs() < 1e-9), "{rules:?}");
}

#[test]
fn column_rules_take_their_offset_insets_and_tint() {
    let (d, fid, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 2, |d, sid| {
        let fid = d.story(sid).unwrap().frames[0];
        let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
        o.column_rule_offset = 3.0;
        o.column_rule_top_inset = 15.0;
        o.column_rule_bottom_inset = 25.0;
        o.column_rule_tint = 0.4;
    });
    // Gutter centre 150 moved 3 pt; columns 10..180 shortened to 25..155.
    assert_eq!(rules.len(), 1, "{rules:?}");
    let r = rules[0];
    assert!((r.center().x - 153.0).abs() < 1e-9 && (r.y0 - 25.0).abs() < 1e-9 && (r.y1 - 155.0).abs() < 1e-9, "{r:?}");
    let sid = d.item(fid).unwrap().text_frame().unwrap().story;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.frames[0].decos.iter().any(|dc| dc.rect == r && (dc.tint - 0.4).abs() < 1e-6));
    // Insets that meet leave no rule.
    let (_, _, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 2, |d, sid| {
        let fid = d.story(sid).unwrap().frames[0];
        let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
        o.column_rule_top_inset = 100.0;
        o.column_rule_bottom_inset = 100.0;
    });
    assert!(rules.is_empty(), "{rules:?}");
}

// ---------- span columns ----------

/// A story of `before` body paragraphs, a heading spanning all columns, and `after` body
/// paragraphs, in a three-column frame. Returns the heading's paragraph index.
fn span_doc(before: usize, after: usize, rect: Rect) -> (Document, StoryId, ItemId, usize) {
    let mut paras: Vec<&str> = vec![LOREM; before];
    paras.push("A heading across the columns");
    paras.extend(vec![LOREM; after]);
    let (mut d, sid, fid) = doc_with(&paras.join("\n"), rect, ParaAttrs::default());
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 3;
    d.story_mut(sid).unwrap().paras[before].para.span_columns = Some(SpanColumns::Span(0));
    (d, sid, fid, before)
}

/// The line's leading slot (glyph ascent and descent may reach into the neighbouring lines).
fn line_box(l: &Line) -> (f64, f64) {
    (l.baseline - 0.75 * l.leading, l.baseline + 0.25 * l.leading)
}

/// No two lines of a frame share any area.
fn assert_no_overlap(ft: &FrameText) {
    for (i, a) in ft.lines.iter().enumerate() {
        for b in &ft.lines[i + 1..] {
            let ((at, ab), (bt, bb)) = (line_box(a), line_box(b));
            let x_overlap = a.x0 < b.x1 - 0.5 && b.x0 < a.x1 - 0.5;
            let y_overlap = at < bb - 0.5 && bt < ab - 0.5;
            assert!(
                !(x_overlap && y_overlap),
                "lines overlap: para {} col {} y {:.1}..{:.1} and para {} col {} y {:.1}..{:.1}",
                a.para,
                a.column,
                at,
                ab,
                b.para,
                b.column,
                bt,
                bb
            );
        }
    }
}

/// The heading spans the frame; text before it sits above it and text after it below it.
fn assert_span_layout(ft: &FrameText, h: usize) {
    let heading: Vec<&Line> = ft.lines.iter().filter(|l| l.para == h).collect();
    assert!(!heading.is_empty());
    let (c0, cn) = (ft.columns[0], ft.columns[ft.columns.len() - 1]);
    for l in &heading {
        assert!((l.x0 - c0.x0.min(cn.x0)).abs() < 1e-6 && (l.x1 - c0.x1.max(cn.x1)).abs() < 1e-6, "heading spans {}..{}", l.x0, l.x1);
    }
    let top = heading.iter().map(|l| line_box(l).0).fold(f64::INFINITY, f64::min);
    let bottom = heading.iter().map(|l| line_box(l).1).fold(f64::NEG_INFINITY, f64::max);
    for l in ft.lines.iter().filter(|l| l.para < h) {
        assert!(line_box(l).1 <= top + 0.5, "para {} col {} ends at {} below the heading top {top}", l.para, l.column, line_box(l).1);
    }
    for l in ft.lines.iter().filter(|l| l.para > h) {
        assert!(line_box(l).0 >= bottom - 0.5, "para {} col {} starts at {} above the heading bottom {bottom}", l.para, l.column, line_box(l).0);
    }
    assert_no_overlap(ft);
}

#[test]
fn text_after_a_spanning_paragraph_flows_below_it() {
    let (d, sid, _, h) = span_doc(2, 3, Rect::new(0.0, 0.0, 540.0, 300.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let ft = &cs.frames[0];
    assert_span_layout(ft, h);
    // The text before is balanced across the three columns above the heading.
    let before: Vec<&Line> = ft.lines.iter().filter(|l| l.para < h).collect();
    let per_col = |c: u32| before.iter().filter(|l| l.column == c).count();
    let counts = [per_col(0), per_col(1), per_col(2)];
    assert!(counts.iter().all(|&n| n > 0), "{counts:?}");
    assert!(counts.iter().max().unwrap() - counts.iter().min().unwrap() <= 1, "{counts:?}");
    // The text after starts right below the heading in every column, on the same baseline.
    let bottom = ft.lines.iter().filter(|l| l.para == h).map(|l| l.baseline).fold(f64::NEG_INFINITY, f64::max);
    let firsts: Vec<f64> = (0..3).map(|c| ft.lines.iter().find(|l| l.para > h && l.column == c).map(|l| l.baseline).unwrap()).collect();
    for b in &firsts {
        assert!(*b > bottom && *b < bottom + 40.0, "{firsts:?} after {bottom}");
    }
    assert!((firsts[1] - firsts[2]).abs() < 1e-6, "{firsts:?}");
}

#[test]
fn spanning_heading_kept_with_next_and_spanning_two_of_three_columns() {
    let (mut d, sid, _, h) = span_doc(2, 3, Rect::new(0.0, 0.0, 540.0, 300.0));
    d.story_mut(sid).unwrap().paras[h].para.keep_with_next = Some(2);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert_span_layout(&cs.frames[0], h);
    // Spanning two of three columns: the third column beside the heading stays empty.
    d.story_mut(sid).unwrap().paras[h].para.span_columns = Some(SpanColumns::Span(2));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = &cs.frames[0];
    let heading = ft.lines.iter().find(|l| l.para == h).unwrap();
    assert!((heading.x0 - ft.columns[0].x0).abs() < 1e-6 && (heading.x1 - ft.columns[1].x1).abs() < 1e-6);
    assert_no_overlap(ft);
    let bottom = heading.baseline + 0.25 * heading.leading;
    assert!(ft.lines.iter().filter(|l| l.para > h).all(|l| line_box(l).0 >= bottom - 0.5));
    assert!((0..3).all(|c| ft.lines.iter().any(|l| l.para > h && l.column == c)));
}

#[test]
fn spanning_paragraph_at_the_top_and_end_of_a_frame() {
    let (d, sid, _, h) = span_doc(0, 4, Rect::new(0.0, 0.0, 540.0, 300.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let ft = &cs.frames[0];
    assert_span_layout(ft, h);
    assert!(ft.lines[0].para == h && ft.lines[0].baseline < 20.0);
    assert!((1..3).all(|c| ft.lines.iter().any(|l| l.column == c)));

    let (d, sid, _, h) = span_doc(3, 0, Rect::new(0.0, 0.0, 540.0, 300.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert_span_layout(&cs.frames[0], h);
}

#[test]
fn text_after_a_spanning_paragraph_threads_into_the_next_frame() {
    let (mut d, sid, f1, h) = span_doc(2, 8, Rect::new(0.0, 0.0, 540.0, 200.0));
    let lid = d.default_layer();
    let (f2, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 300.0, 540.0, 700.0), lid, "", ParaFormat::default()).unwrap();
    d.item_mut(f2).unwrap().text_frame_mut().unwrap().options.columns = 3;
    d.thread(f1, f2).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset(), "overset at {:?}", cs.overset_at);
    assert_span_layout(&cs.frames[0], h);
    let (a, b) = (&cs.frames[0], &cs.frames[1]);
    assert!((0..3).all(|c| a.lines.iter().any(|l| l.para > h && l.column == c)));
    assert!(!b.lines.is_empty() && b.lines[0].para > h && b.lines[0].baseline < 320.0);
    assert_no_overlap(b);
    // Every line of the story is set once, in order.
    let lines = all_lines(&cs);
    for w in lines.windows(2) {
        assert!(w[1].range.start >= w[0].range.end, "{:?} then {:?}", w[0].range, w[1].range);
    }
    assert_eq!(lines.last().unwrap().range.end, d.story(sid).unwrap().text.len());
}

#[test]
fn spanning_paragraph_in_rtl_and_vertical_frames() {
    let (mut d, sid, _, h) = span_doc(2, 4, Rect::new(0.0, 0.0, 540.0, 500.0));
    d.story_mut(sid).unwrap().direction = designcraft_doc::TextDirection::RightToLeft;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert_span_layout(&cs.frames[0], h);

    let (mut d, sid, _, h) = span_doc(2, 4, Rect::new(0.0, 0.0, 540.0, 540.0));
    d.story_mut(sid).unwrap().vertical = true;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.frames[0].lines.is_empty());
    assert_no_overlap(&cs.frames[0]);
    assert!(cs.frames[0].lines.iter().any(|l| l.para == h));
}

// ---------- split columns ----------

/// A story of a body paragraph, `split` paragraphs in split columns and `after` body paragraphs.
/// Returns the index of the first split paragraph.
fn split_doc(split: usize, after: usize, rect: Rect, cfg: (u32, f64, f64)) -> (Document, StoryId, ItemId, usize) {
    let mut paras: Vec<&str> = vec![LOREM; 1 + split + after];
    paras[0] = "An introduction set at the full measure of the column, before the split block.";
    let (mut d, sid, fid) = doc_with(&paras.join("\n"), rect, ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    for p in &mut st.paras[1..=split] {
        p.para.span_columns = Some(SpanColumns::Split(cfg.0));
        p.para.split_inside_gutter = Some(cfg.1);
        p.para.split_outside_gutter = Some(cfg.2);
    }
    (d, sid, fid, 1)
}

/// The sub-columns of `col`: (x0, x1) of each.
fn sub_columns(col: Rect, (n, inside, outside): (u32, f64, f64)) -> Vec<(f64, f64)> {
    let w = (col.width() - 2.0 * outside - inside * (n - 1) as f64) / n as f64;
    (0..n).map(|k| col.x0 + outside + k as f64 * (w + inside)).map(|x| (x, x + w)).collect()
}

/// The split paragraphs' lines, by sub-column.
fn split_lines<'a>(ft: &'a FrameText, paras: std::ops::Range<usize>, subs: &[(f64, f64)]) -> Vec<Vec<&'a Line>> {
    let mut by_sub = vec![Vec::new(); subs.len()];
    for l in ft.lines.iter().filter(|l| paras.contains(&l.para)) {
        let k = subs.iter().position(|&(x0, x1)| (l.x0 - x0).abs() < 1e-6 && (l.x1 - x1).abs() < 1e-6);
        let k = k.unwrap_or_else(|| panic!("line of para {} at {}..{} is in no sub-column of {subs:?}", l.para, l.x0, l.x1));
        by_sub[k].push(l);
    }
    by_sub
}

#[test]
fn split_paragraphs_are_set_in_balanced_sub_columns() {
    for cfg in [(2, 12.0, 10.0), (3, 6.0, 0.0)] {
        let (d, sid, _, s) = split_doc(2, 1, Rect::new(0.0, 0.0, 400.0, 700.0), cfg);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        assert!(!cs.is_overset());
        let ft = &cs.frames[0];
        assert_no_overlap(ft);
        let by_sub = split_lines(ft, s..s + 2, &sub_columns(ft.columns[0], cfg));
        let counts: Vec<usize> = by_sub.iter().map(Vec::len).collect();
        assert!(counts.iter().all(|&n| n > 0), "{cfg:?}: {counts:?}");
        assert!(counts.iter().max().unwrap() - counts.iter().min().unwrap() <= 1, "{cfg:?}: balanced {counts:?}");
        // Every sub-column starts on the same baseline, below the paragraph before the block.
        let firsts: Vec<f64> = by_sub.iter().map(|v| v[0].baseline).collect();
        assert!(firsts.iter().all(|b| (b - firsts[0]).abs() < 1e-6), "{cfg:?}: {firsts:?}");
        let intro = ft.lines.iter().filter(|l| l.para == 0).map(|l| line_box(l).1).fold(f64::NEG_INFINITY, f64::max);
        assert!(firsts[0] > intro && firsts[0] < intro + 20.0, "{cfg:?}: block at {} after {intro}", firsts[0]);
        // The text after the block continues at the full measure below its deepest sub-column.
        let bottom = by_sub.iter().flatten().map(|l| line_box(l).1).fold(f64::NEG_INFINITY, f64::max);
        let next = ft.lines.iter().find(|l| l.para == s + 2).unwrap();
        assert!(line_box(next).0 >= bottom - 0.5 && line_box(next).0 < bottom + 20.0, "{cfg:?}: {} after {bottom}", line_box(next).0);
        assert!((next.x0 - ft.columns[0].x0).abs() < 1e-6 && (next.x1 - ft.columns[0].x1).abs() < 1e-6);
    }
}

#[test]
fn split_block_in_the_second_column_and_at_the_story_end() {
    let cfg = (2, 12.0, 0.0);
    let (mut d, sid, fid, s) = split_doc(3, 0, Rect::new(0.0, 0.0, 540.0, 400.0), cfg);
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    d.story_mut(sid).unwrap().paras[s].para.start_paragraph = Some(StartParagraph::NextColumn);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let ft = &cs.frames[0];
    assert_no_overlap(ft);
    assert!(ft.lines.iter().filter(|l| l.para >= s).all(|l| l.column == 1));
    let by_sub = split_lines(ft, s..s + 3, &sub_columns(ft.columns[1], cfg));
    // Nothing follows the block: it fills its first sub-column to the bottom of the frame, then
    // the next.
    let (a, b) = (&by_sub[0], &by_sub[1]);
    assert!(!a.is_empty() && !b.is_empty() && a.len() > b.len() + 1, "{} and {} lines", a.len(), b.len());
    let low = a.last().unwrap();
    assert!(low.baseline + low.descent > 400.0 - low.leading, "the first sub-column ends at {}", low.baseline);
    assert!((a[0].baseline - b[0].baseline).abs() < 1e-6);
}

#[test]
fn split_block_flows_into_the_next_column() {
    let cfg = (2, 12.0, 0.0);
    let (mut d, sid, fid, s) = split_doc(6, 1, Rect::new(0.0, 0.0, 540.0, 200.0), cfg);
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = &cs.frames[0];
    assert_no_overlap(ft);
    for c in 0..2 {
        let in_col: Vec<&Line> = ft.lines.iter().filter(|l| l.column == c as u32).collect();
        let lines: Vec<&Line> = in_col.iter().copied().filter(|l| (s..s + 6).contains(&l.para)).collect();
        let subs = sub_columns(ft.columns[c], cfg);
        assert!(subs.iter().all(|&(x0, x1)| lines.iter().any(|l| (l.x0 - x0).abs() < 1e-6 && (l.x1 - x1).abs() < 1e-6)), "column {c}");
    }
    // The block continues at the top of the second column.
    let top = ft.lines.iter().find(|l| l.column == 1).unwrap();
    assert!((s..s + 6).contains(&top.para) && top.baseline < 20.0);
}

/// Each paragraph's list label as text: its label glyphs (laid before the paragraph's own text)
/// matched back to characters of the label font; tabs and other invisible glyphs are skipped.
fn list_labels(cs: &ComposedStory, alphabet: &str) -> Vec<String> {
    all_lines(cs)
        .iter()
        .filter(|l| l.first_in_para)
        .map(|l| {
            l.glyphs
                .iter()
                .take_while(|g| g.len == 0)
                .filter(|g| g.visible)
                .map(|g| alphabet.chars().find(|c| g.face.glyph_for(*c) == g.gid).unwrap_or('?'))
                .collect()
        })
        .collect()
}

const LABEL_CHARS: &str = "0123456789ABCDIVXabcdivx.\u{2022} Tabel";

fn numbered(style: designcraft_doc::NumberStyle, expression: &str) -> ParaAttrs {
    ParaAttrs {
        list_type: Some(designcraft_doc::ListType::Numbers),
        number_style: Some(style),
        number_expression: Some(expression.into()),
        ..Default::default()
    }
}

#[test]
fn numbered_lists_follow_their_format_and_expression() {
    use designcraft_doc::NumberStyle as N;
    let cases: [(N, &str, [&str; 3]); 4] = [
        (N::UpperLetters, "^#.^t", ["A.", "B.", "C."]),
        (N::LowerRoman, "^#.^t", ["i.", "ii.", "iii."]),
        (N::ArabicThreeDigits, "^#.^t", ["001.", "002.", "003."]),
        (N::Arabic, "Tabel ^#^t", ["Tabel 1", "Tabel 2", "Tabel 3"]),
    ];
    for (style, expression, want) in cases {
        let (d, sid, _) = doc_with("One\nTwo\nThree", Rect::new(0.0, 0.0, 300.0, 300.0), numbered(style, expression));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        assert_eq!(list_labels(&cs, LABEL_CHARS), want, "{style:?} {expression}");
    }
}

#[test]
fn numbering_from_the_largest_start_number_does_not_overflow() {
    let (mut d, sid, _) = doc_with("One\nTwo", Rect::new(0.0, 0.0, 300.0, 300.0), numbered(designcraft_doc::NumberStyle::Arabic, "^#."));
    d.story_mut(sid).unwrap().paras[0].para.start_at = Some(Some(u32::MAX));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let max = format!("{}.", u32::MAX);
    assert_eq!(list_labels(&cs, LABEL_CHARS), [max.clone(), max]);
}

#[test]
fn empty_paragraphs_get_no_bullet_or_number() {
    let bullets = ParaAttrs { list_type: Some(designcraft_doc::ListType::Bullets), ..Default::default() };
    let (d, sid, _) = doc_with("One\n\nTwo\n", Rect::new(0.0, 0.0, 300.0, 300.0), bullets);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(list_labels(&cs, LABEL_CHARS), ["\u{2022}", "", "\u{2022}", ""]);
    // An empty paragraph doesn't use up a number, in a story's own list or a named one.
    for name in ["", "Steps"] {
        let attrs = ParaAttrs { list_name: Some(name.into()), ..numbered(designcraft_doc::NumberStyle::Arabic, "^#.^t") };
        let (mut d, sid, _) = doc_with("One\n\nTwo\n", Rect::new(0.0, 0.0, 300.0, 300.0), attrs);
        if !name.is_empty() {
            d.settings.lists.push(designcraft_doc::NumberedList { name: name.into(), continue_across_stories: false });
        }
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        assert_eq!(list_labels(&cs, LABEL_CHARS), ["1.", "", "2.", ""], "list `{name}`");
    }
}

#[test]
fn tight_word_spacing_does_not_isolate_a_word() {
    // A narrow word-spacing range (100–105%) with some letter spacing and glyph scaling, no
    // hyphenation: lines that need more stretch must open their word spaces, not letter-space a
    // single word across the measure.
    let para = ParaAttrs {
        align: Some(Align::LeftJustified),
        hyphenate: Some(false),
        first_line_indent: Some(9.0),
        word_space_min: Some(1.0),
        word_space_desired: Some(1.03),
        word_space_max: Some(1.05),
        letter_space_min: Some(-0.05),
        letter_space_max: Some(0.05),
        glyph_scale_min: Some(0.99),
        glyph_scale_max: Some(1.01),
        ..Default::default()
    };
    // Short words: any two fit on a line together.
    let text = CORPUS[..2].join(" ");
    for w in (200..=320).step_by(5) {
        let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, w as f64, 20000.0), para.clone());
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let lines = all_lines(&cs);
        for l in &lines[..lines.len() - 1] {
            let words = text[l.range.clone()].split_whitespace().count();
            assert!(words > 1, "width {w}: a single-word line {:?}", &text[l.range.clone()]);
        }
    }
}

#[test]
fn fixed_width_spaces_do_not_stretch_on_justified_lines() {
    // Long words in a narrow column: lines stretch past the maximum word spacing. Only the word
    // spaces take that stretch; em, en and thin spaces keep their width.
    let words = ["characteristically", "misunderstanding", "unquestionably", "responsibility", "administration", "photographer"];
    let seps = [" ", "\u{2003}", " ", "\u{2002}", " ", "\u{2009}"];
    let text: String = (0..60).map(|i| format!("{}{}", words[i % words.len()], seps[i % seps.len()])).collect();
    let text = text.trim_end().to_string();
    let fixed = |c: char| matches!(c, '\u{2003}' | '\u{2002}' | '\u{2009}');
    // (character, advance) of each fixed space, per line, and whether the line ends the paragraph.
    let spaces = |align: Align, w: f64| -> Vec<(Vec<(char, f64)>, bool)> {
        let para = ParaAttrs { align: Some(align), hyphenate: Some(false), ..Default::default() };
        let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, w, 20000.0), para);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs)
            .iter()
            .map(|l| {
                let found = l.glyphs.iter().filter_map(|g| text[g.byte..].chars().next().filter(|&c| fixed(c)).map(|c| (c, g.adv))).collect();
                (found, l.last_in_para)
            })
            .collect()
    };
    let mut checked = 0;
    for w in [190.0, 230.0, 270.0] {
        let natural: Vec<(char, f64)> = spaces(Align::Left, w).into_iter().flat_map(|l| l.0).collect();
        for (found, last) in spaces(Align::LeftJustified, w) {
            if last {
                continue;
            }
            for (c, a) in found {
                let Some(&(_, n)) = natural.iter().find(|x| x.0 == c) else { continue };
                assert!((a - n).abs() < 1e-6, "width {w}: U+{:04X} is {a:.2} wide, not {n:.2}", c as u32);
                checked += 1;
            }
        }
    }
    assert!(checked > 5, "{checked} fixed spaces on justified lines");
}
