//! Japanese composition (mojikumi, line edges, justification) and frame grids.

use designcraft_doc::build::NewDocument;
use designcraft_doc::{Align, Document, FrameGrid, GridAlignment, Item, ParaAttrs, ParaFormat, SpreadRef};
use designcraft_geom::Rect;

use super::*;

/// A synthetic Japanese font: kanji, kana and full-width punctuation one em wide (drawn as Source
/// Sans 3 glyphs), Latin letters at their own widths. Added to the global database once.
fn japanese_test_font() -> &'static str {
    use designcraft_fonts::testing::{font_mapping, with_advances};
    const FAMILY: &str = "DC Test Japanese";
    static ADDED: std::sync::Once = std::sync::Once::new();
    ADDED.call_once(|| {
        let wide =
            [('漢', 'X'), ('字', 'H'), ('か', 'o'), ('な', 'n'), ('、', ','), ('。', '.'), ('「', '['), ('」', ']'), ('・', '-'), ('\u{3000}', ' ')];
        let latin = [('A', 'A'), ('B', 'B'), ('x', 'x'), (' ', ' ')];
        let base = font_mapping(FAMILY, &[&wide[..], &latin[..]].concat()).unwrap_or_default();
        let scratch = designcraft_fonts::FontDb::with_font_dirs(vec![]);
        scratch.add_font(base.clone());
        let face = scratch.face(FAMILY, "Regular");
        // Latin keeps its widths (the face's units are its own, Source Sans 3 has 1000 to the em).
        let keep: Vec<(u16, u16)> = latin
            .iter()
            .filter_map(|(c, _)| {
                let gid = face.glyph_for(*c);
                Some((u16::try_from(gid).ok()?, face.advance(gid) as u16))
            })
            .collect();
        if let Some(f) = with_advances(&base, 1000, &keep) {
            designcraft_fonts::FontDb::global().add_font(f);
        }
    });
    FAMILY
}

/// A document with `text` in the Japanese test font at 10 pt in a frame of `rect`.
fn japanese(text: &str, rect: Rect, para: ParaAttrs) -> (Document, StoryId, ItemId) {
    let family = japanese_test_font();
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), rect, lid, text, ParaFormat { para, ..Default::default() }).unwrap();
    let st = d.story_mut(sid).unwrap();
    let n = st.len();
    st.format_chars(0..n, |f| {
        f.over.font_family = Some(family.into());
        f.over.font_style = Some("Regular".into());
        f.over.size = Some(10.0);
    });
    (d, sid, fid)
}

fn with_set(set: &str) -> ParaAttrs {
    ParaAttrs { mojikumi: Some(set.into()), hyphenate: Some(false), ..Default::default() }
}

fn lines(d: &Document, sid: StoryId) -> Vec<Line> {
    compose_story(d, sid, &ComposeOptions::default()).frames.iter().flat_map(|f| f.lines.clone()).collect()
}

/// x of the glyph for byte `b` on `line`.
fn x_at(line: &Line, b: usize) -> f64 {
    line.glyphs.iter().find(|g| g.byte == b && g.len > 0).map_or(f64::NAN, |g| g.x)
}

#[test]
fn solid_setting_keeps_full_width_punctuation() {
    let text = "漢」「漢";
    let (d, sid, _) = japanese(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    let l = &lines(&d, sid)[0];
    assert!((l.end_x - l.x0 - 40.0).abs() < 1e-6, "{}", l.end_x - l.x0);
}

#[test]
fn consecutive_brackets_collapse_to_half_an_em() {
    let text = "漢」「漢";
    for set in ["lineEndHalf", "fullWidth"] {
        let (d, sid, _) = japanese(text, Rect::new(0.0, 0.0, 300.0, 100.0), with_set(set));
        let l = &lines(&d, sid)[0];
        // 漢 10 + 」 half 5 + 二分 aki 5 + 「 half 5 + 漢 10 (JLREQ §3.1.4).
        assert!((l.end_x - l.x0 - 35.0).abs() < 1e-6, "{set}: {}", l.end_x - l.x0);
        // The opening bracket's full-width box starts after the closing one's half-width ink.
        let open = text.find('「').unwrap();
        assert!((x_at(l, open) - (l.x0 + 15.0)).abs() < 1e-6, "{set}: {}", x_at(l, open));
    }
}

#[test]
fn full_stop_and_closing_bracket_touch() {
    let text = "漢。」漢";
    let (d, sid, _) = japanese(text, Rect::new(0.0, 0.0, 300.0, 100.0), with_set("lineEndHalf"));
    let l = &lines(&d, sid)[0];
    // 漢 10 + 。 5 + 」 5 + aki 5 + 漢 10.
    assert!((l.end_x - l.x0 - 35.0).abs() < 1e-6, "{}", l.end_x - l.x0);
}

#[test]
fn half_width_set_sets_punctuation_tight() {
    let text = "漢、漢「漢」漢";
    let (d, sid, _) = japanese(text, Rect::new(0.0, 0.0, 300.0, 100.0), with_set("halfWidth"));
    let l = &lines(&d, sid)[0];
    assert!((l.end_x - l.x0 - (4.0 * 10.0 + 3.0 * 5.0)).abs() < 1e-6, "{}", l.end_x - l.x0);
}

#[test]
fn a_comma_ends_a_line_half_width() {
    // Nine kanji and a comma: 100 pt solid, 95 pt with the comma half-width at the line end.
    let text = format!("{}、{}", "漢".repeat(9), "漢".repeat(3));
    let rect = Rect::new(0.0, 0.0, 97.5, 200.0);
    let (d, sid, _) = japanese(&text, rect, ParaAttrs { hyphenate: Some(false), ..Default::default() });
    let solid = lines(&d, sid);
    assert_eq!(&text[solid[0].range.clone()], "漢".repeat(8), "solid: the comma can't start a line, so a kanji goes with it");
    let (d, sid, _) = japanese(&text, rect, with_set("lineEndHalf"));
    let set = lines(&d, sid);
    assert_eq!(&text[set[0].range.clone()], format!("{}、", "漢".repeat(9)));
    assert!(set[0].end_x <= set[0].x1 + 1e-6);
    // Full-width punctuation keeps the comma's space at the line end too.
    let (d, sid, _) = japanese(&text, rect, with_set("fullWidth"));
    assert_eq!(&text[lines(&d, sid)[0].range.clone()], "漢".repeat(8));
}

#[test]
fn an_opening_bracket_starts_a_line_flush() {
    // Ten kanji fill the first line; the bracket starts the second.
    let text = format!("{}「漢」", "漢".repeat(10));
    let rect = Rect::new(0.0, 0.0, 100.0, 200.0);
    let open = text.find('「').unwrap();
    let (d, sid, _) = japanese(&text, rect, with_set("lineEndHalf"));
    let ls = lines(&d, sid);
    assert_eq!(ls[1].range.start, open);
    // Tentsuki: the half-width ink at the line start (its full-width box starts half an em out).
    assert!((x_at(&ls[1], open) - (ls[1].x0 - 5.0)).abs() < 1e-6, "{}", x_at(&ls[1], open));
    let (d, sid, _) = japanese(&text, rect, with_set("fullWidth"));
    let ls = lines(&d, sid);
    assert!((x_at(&ls[1], open) - ls[1].x0).abs() < 1e-6, "full width keeps the bracket's space");
    // A paragraph starting with a bracket keeps it under any set.
    let (d, sid, _) = japanese("「漢」", rect, with_set("lineEndHalf"));
    let l = &lines(&d, sid)[0];
    assert!((x_at(l, 0) - l.x0).abs() < 1e-6);
}

#[test]
fn japanese_and_western_get_a_quarter_em() {
    let text = "漢AB漢";
    let (d, sid, _) = japanese(text, Rect::new(0.0, 0.0, 300.0, 100.0), with_set("lineEndHalf"));
    let l = &lines(&d, sid)[0];
    let (_, solid_sid, _) = japanese(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    let _ = solid_sid;
    let a = x_at(l, text.find('A').unwrap());
    assert!((a - (l.x0 + 12.5)).abs() < 1e-6, "{a}");
    let b = x_at(l, text.rfind('漢').unwrap());
    let w_ab = b - a;
    // A and B are set solid; the kanji after them is another 2.5 pt away.
    let ab: f64 = l.glyphs.iter().filter(|g| matches!(text[g.byte..].chars().next(), Some('A' | 'B'))).map(|g| g.adv).sum();
    assert!((w_ab - ab).abs() < 1e-6, "{w_ab} {ab}");
}

#[test]
fn justified_japanese_lines_fill_the_measure() {
    let text = format!("{}、{}。「{}」{}", "漢".repeat(7), "かな".repeat(6), "字".repeat(5), "漢".repeat(20));
    let (d, sid, _) = japanese(&text, Rect::new(0.0, 0.0, 103.0, 400.0), ParaAttrs { align: Some(Align::LeftJustified), ..with_set("lineEndHalf") });
    let ls = lines(&d, sid);
    assert!(ls.len() > 3);
    for l in &ls[..ls.len() - 1] {
        assert!((l.end_x - l.x1).abs() < 0.05, "{:?} ends at {} not {}", &text[l.range.clone()], l.end_x, l.x1);
        let first = text[l.range.clone()].chars().next().unwrap_or(' ');
        assert!(!"、。」".contains(first), "kinsoku: {:?}", &text[l.range.clone()]);
    }
}

fn grid() -> FrameGrid {
    FrameGrid { size: 10.0, char_aki: 2.0, line_aki: 5.0, ..Default::default() }
}

/// A frame grid of `chars` × `lines` cells (one column) with `text`.
fn grid_doc(text: &str, g: FrameGrid, chars: u32, lines: u32, vertical: bool) -> (Document, StoryId, ItemId) {
    let (w, h) = g.area_size(chars, lines, 1, 0.0, vertical);
    let (mut d, sid, fid) = japanese(text, Rect::new(0.0, 0.0, w, h), ParaAttrs::default());
    if let Some(tf) = d.item_mut(fid).and_then(Item::text_frame_mut) {
        tf.options.frame_grid = Some(g);
    }
    d.story_mut(sid).unwrap().vertical = vertical;
    (d, sid, fid)
}

#[test]
fn frame_grid_puts_one_character_in_each_cell() {
    let text = "漢、漢漢「漢」漢漢";
    let (d, sid, _) = grid_doc(text, grid(), 5, 3, false);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let ls: Vec<&Line> = cs.frames[0].lines.iter().collect();
    assert_eq!(ls.len(), 2);
    // Every character (punctuation included, no mojikumi) starts a cell: x = n × 12.
    for l in &ls {
        for (n, g) in l.glyphs.iter().filter(|g| g.len > 0).enumerate() {
            assert!((g.x - n as f64 * 12.0).abs() < 1e-6, "{:?}", (n, g.x));
        }
    }
    // Lines are a line pitch (cell 10 + line aki 5) apart, centred in their rows.
    assert!((ls[1].baseline - ls[0].baseline - 15.0).abs() < 1e-6);
    let g = &cs.frames[0].lines[0].glyphs[0];
    let (top, bottom) = g.face.em_box();
    let k = 10.0 / g.face.units_per_em();
    let centre = ls[0].baseline - (top + bottom) / 2.0 * k;
    assert!((centre - 5.0).abs() < 1e-3, "em box centre {centre}");
}

#[test]
fn frame_grid_overflow_follows_its_rows() {
    // 5 × 2 cells hold ten characters; the eleventh is overset.
    let (d, sid, _) = grid_doc(&"漢".repeat(11), grid(), 5, 2, false);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(cs.frames[0].lines.len(), 2);
    assert!(cs.is_overset());
    let (d, sid, _) = grid_doc(&"漢".repeat(10), grid(), 5, 2, false);
    assert!(!compose_story(&d, sid, &ComposeOptions::default()).is_overset());
}

#[test]
fn a_larger_heading_takes_whole_rows() {
    let text = "漢漢\n漢漢";
    let (mut d, sid, _) = grid_doc(text, grid(), 5, 6, false);
    d.story_mut(sid).unwrap().format_chars(0..6, |f| f.over.size = Some(22.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ls = &cs.frames[0].lines;
    assert_eq!(ls.len(), 2);
    // 22 pt needs two rows (10 + 5 + 10 = 25 ≥ 22); the body line starts in the third row.
    let g = ls[1].glyphs.iter().find(|g| g.len > 0).unwrap();
    let (top, bottom) = g.face.em_box();
    let k = 10.0 / g.face.units_per_em();
    let centre = ls[1].baseline - (top + bottom) / 2.0 * k;
    assert!((centre - (30.0 + 5.0)).abs() < 1e-3, "{centre}");
}

#[test]
fn frame_grid_line_alignment_overrides_paragraphs() {
    let g = FrameGrid { line_align: Some(Align::Right), ..grid() };
    let (d, sid, _) = grid_doc("漢漢", g, 5, 2, false);
    let l = &lines(&d, sid)[0];
    let first = l.glyphs.iter().find(|g| g.len > 0).unwrap();
    // Two cells of 12 pt (the last one's aki past the measure) end the line.
    assert!((first.x - (l.x1 + 2.0 - 24.0)).abs() < 1e-6, "{}", first.x);
}

#[test]
fn vertical_frame_grids_set_lines_along_the_turned_box() {
    let (d, sid, fid) = grid_doc(&"漢".repeat(7), grid(), 5, 3, true);
    let it = d.item(fid).unwrap();
    // Five cells down (5 × 12 − 2), three lines across (3 × 15 − 5).
    assert!((it.bounds().width() - 40.0).abs() < 1e-6 && (it.bounds().height() - 58.0).abs() < 1e-6);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert_eq!(cs.frames[0].lines.len(), 2);
    for (n, g) in cs.frames[0].lines[0].glyphs.iter().filter(|g| g.len > 0).enumerate() {
        assert!((g.x - n as f64 * 12.0).abs() < 1e-6);
    }
}

#[test]
fn grid_mojikumi_comes_from_the_grid_unless_the_paragraph_has_one() {
    let text = "漢」「漢";
    let g = FrameGrid { mojikumi: "lineEndHalf".into(), ..grid() };
    let (d, sid, _) = grid_doc(text, g.clone(), 10, 2, false);
    let l = &lines(&d, sid)[0];
    // 漢 12 + 」 (cell 12 − blank 5) + aki 5 + 「 (12 − 5) + 漢 12 = 43.
    assert!((l.end_x - l.x0 - 43.0).abs() < 1e-6, "{}", l.end_x - l.x0);
    let (mut d, sid, _) = grid_doc(text, g, 10, 2, false);
    d.story_mut(sid).unwrap().paras[0].para.mojikumi = Some("halfWidth".into());
    let l = &lines(&d, sid)[0];
    assert!((l.end_x - l.x0 - 38.0).abs() < 1e-6, "{}", l.end_x - l.x0);
}

#[test]
fn hostile_frame_grids_compose_without_panicking() {
    let g = FrameGrid { size: f64::NAN, char_aki: -1e12, line_aki: f64::INFINITY, h_scale: 0.0, ..grid() };
    let (mut d, sid, fid) = japanese("漢、漢\n「漢」", Rect::new(0.0, 0.0, 100.0, 100.0), ParaAttrs::default());
    if let Some(tf) = d.item_mut(fid).and_then(Item::text_frame_mut) {
        tf.options.frame_grid = Some(g);
    }
    let _ = compose_story(&d, sid, &ComposeOptions::default());
    let _ = GridAlignment::ALL;
}

/// Justified Japanese set solid (no mojikumi set, as in a frame grid) used to have no material to
/// justify with: every inexact line was equally bad, and the composer put a two-character line
/// first. Lines must fill the measure and the first one keep its characters.
#[test]
fn justified_solid_japanese_doesnt_strand_the_first_line() {
    let text = format!("「{}」は、{}。", "漢".repeat(10), "字".repeat(40));
    for grid in [false, true] {
        let (d, sid) = if grid {
            let (mut d, sid, _) = grid_doc(&text, FrameGrid { size: 10.0, char_aki: 0.0, line_aki: 5.0, ..Default::default() }, 12, 8, false);
            d.story_mut(sid).unwrap().paras[0].para.align = Some(Align::LeftJustified);
            (d, sid)
        } else {
            let (d, sid, _) =
                japanese(&text, Rect::new(0.0, 0.0, 115.0, 400.0), ParaAttrs { align: Some(Align::LeftJustified), ..Default::default() });
            (d, sid)
        };
        let ls = lines(&d, sid);
        assert!(text[ls[0].range.clone()].chars().count() >= 10, "grid {grid}: first line {:?}", &text[ls[0].range.clone()]);
        for l in &ls[..ls.len() - 1] {
            assert!((l.end_x - l.x1).abs() < 0.6, "grid {grid}: {:?} ends at {} not {}", &text[l.range.clone()], l.end_x, l.x1);
        }
    }
}

/// Justified Japanese pushes in before it pushes out (JLREQ §3.8, oikomi first): a line one
/// character over the measure that has punctuation to compress takes the character, even set
/// solid; it isn't pushed to the next line with the rest spread out.
#[test]
fn justified_japanese_pushes_in_before_pushing_out() {
    // 24 characters, then a full stop that can't start a line: the closing bracket and the comma
    // compress to take it in.
    let text = format!("「{}」漢、{}。{}", "漢".repeat(10), "漢".repeat(10), "字".repeat(30));
    for (grid, set) in [(true, ""), (false, ""), (false, "lineEndHalf")] {
        let (d, sid) = if grid {
            let (mut d, sid, _) = grid_doc(&text, FrameGrid { size: 10.0, char_aki: 0.0, line_aki: 5.0, ..Default::default() }, 24, 8, false);
            d.story_mut(sid).unwrap().paras[0].para.align = Some(Align::LeftJustified);
            (d, sid)
        } else {
            let (d, sid, _) = japanese(&text, Rect::new(0.0, 0.0, 240.0, 400.0), ParaAttrs { align: Some(Align::LeftJustified), ..with_set(set) });
            (d, sid)
        };
        let ls = lines(&d, sid);
        assert_eq!(text[ls[0].range.clone()].chars().count(), 25, "grid {grid} set {set:?}: {:?}", &text[ls[0].range.clone()]);
        assert!((ls[0].end_x - ls[0].x1).abs() < 0.6, "grid {grid} set {set:?}: ends at {}", ls[0].end_x);
    }
}
