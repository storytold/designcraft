//! Frame grids: characters in cells, rows, alignment, vertical grids.

use designcraft_doc::build::NewDocument;
use designcraft_doc::{Align, Document, FrameGrid, GridAlignment, Item, ParaAttrs, ParaFormat, SpreadRef};
use designcraft_geom::Rect;

use super::*;

/// A synthetic CJK font: Han characters, kana, hangul and full-width punctuation one em wide (drawn as Source
/// Sans 3 glyphs), Latin letters at their own widths. Added to the global database once.
fn japanese_test_font() -> &'static str {
    use designcraft_fonts::testing::{font_mapping, with_advances};
    const FAMILY: &str = "DC Test Japanese";
    static ADDED: std::sync::Once = std::sync::Once::new();
    ADDED.call_once(|| {
        let wide = [
            ('漢', 'X'),
            ('字', 'H'),
            ('か', 'o'),
            ('な', 'n'),
            ('、', ','),
            ('。', '.'),
            ('「', '['),
            ('」', ']'),
            ('・', '-'),
            ('\u{3000}', ' '),
            // Simplified and Traditional Chinese, Korean.
            ('汉', 'X'),
            ('语', 'H'),
            ('漢', 'X'),
            ('語', 'H'),
            ('，', ','),
            ('《', '['),
            ('》', ']'),
            // (Drawn as glyphs the source face doesn't kern, as hangul syllables aren't.)
            ('한', 'X'),
            ('글', 'H'),
        ];
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

fn lines(d: &Document, sid: StoryId) -> Vec<Line> {
    compose_story(d, sid, &ComposeOptions::default()).frames.iter().flat_map(|f| f.lines.clone()).collect()
}

/// A grid of 10 pt cells, 2 pt apart along the line and 5 pt between lines.
fn grid() -> FrameGrid {
    FrameGrid { size: 10.0, char_aki: 2.0, line_aki: 5.0, ..Default::default() }
}

/// A frame grid of `chars` × `lines` cells (one column) with `text`.
fn grid_doc(text: &str, g: FrameGrid, chars: u32, lines: u32, vertical: bool) -> (Document, StoryId, ItemId) {
    let (w, h) = g.area_size(chars, lines, 1, 0.0, vertical);
    let (mut d, sid, fid) = japanese(text, Rect::new(0.0, 0.0, w, h), ParaAttrs::default());
    if let Some(tf) = d.item_mut(fid).and_then(Item::text_frame_mut) {
        tf.options.frame_grid = Some(Box::new(g));
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
    // Every character (punctuation included) starts a cell: x = n × 12.
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

/// Chinese (Simplified and Traditional) and Korean text is set the same way: every Han
/// character, hangul syllable and full-width punctuation mark takes a cell, across and down.
#[test]
fn chinese_and_korean_text_takes_one_character_to_a_cell() {
    // (Ten characters, broken where kinsoku allows: no closing mark starts the second line.)
    for text in ["汉语，汉语《汉》语。", "漢語，漢語《漢》語。", "한글。한글한글한글。"] {
        for vertical in [false, true] {
            let (d, sid, _) = grid_doc(text, grid(), 5, 3, vertical);
            let cs = compose_story(&d, sid, &ComposeOptions::default());
            assert!(!cs.is_overset(), "{text}");
            let ls: Vec<&Line> = cs.frames[0].lines.iter().collect();
            assert_eq!(ls.len(), 2, "{text}");
            for l in &ls {
                let glyphs: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0).collect();
                assert_eq!(glyphs.len(), if std::ptr::eq(*l, ls[0]) { 5 } else { text.chars().count() - 5 }, "{text}");
                for (n, g) in glyphs.iter().enumerate() {
                    assert!((g.x - n as f64 * 12.0).abs() < 1e-6, "{text} vertical {vertical}: {:?}", (n, g.x));
                }
            }
        }
    }
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
fn hostile_frame_grids_compose_without_panicking() {
    let g = FrameGrid { size: f64::NAN, char_aki: -1e12, line_aki: f64::INFINITY, h_scale: 0.0, ..grid() };
    let (mut d, sid, fid) = japanese("漢、漢\n「漢」", Rect::new(0.0, 0.0, 100.0, 100.0), ParaAttrs::default());
    if let Some(tf) = d.item_mut(fid).and_then(Item::text_frame_mut) {
        tf.options.frame_grid = Some(Box::new(g));
    }
    let _ = compose_story(&d, sid, &ComposeOptions::default());
    let _ = GridAlignment::ALL;
}
