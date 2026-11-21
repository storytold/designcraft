//! Justified Arabic puts its one kashida per word at the join typography prefers, and none where
//! every join is forbidden (#2486).

use std::collections::BTreeSet;

use designcraft_compose::{ComposeOptions, compose_story};
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Align, Document, ParaAttrs, ParaFormat, SpreadRef, TextDirection};
use designcraft_fonts::FontDb;
use designcraft_fonts::testing::font_mapping;
use designcraft_geom::Rect;

const FAMILY: &str = "DC Kashida Rules";

/// Installs a font that draws every Arabic letter used below as a Latin letter, and the tatweel as
/// an underscore, so the test does not depend on the fonts of the machine.
fn install_font() {
    let letters = "ابتثحدرزسصضعغفقكلمنهةوىي";
    let mut glyphs: Vec<(char, char)> = letters.chars().zip('a'..='z').collect();
    glyphs.extend([('\u{0640}', '_'), (' ', ' ')]);
    FontDb::global().add_font(font_mapping(FAMILY, &glyphs).unwrap());
}

/// The positions (in letters) of the letters before which kashidas are drawn when `word`, repeated
/// on justified lines of several widths, is stretched.
fn kashida_letters(word: &str) -> BTreeSet<usize> {
    let text = vec![word; 40].join(" ");
    let mut found = BTreeSet::new();
    for width in (60..=140).step_by(4) {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let para = ParaAttrs { align: Some(Align::RightJustified), direction: Some(TextDirection::RightToLeft), ..Default::default() };
        let rect = Rect::new(0.0, 0.0, f64::from(width), 800.0);
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), rect, lid, &text, ParaFormat { para, ..Default::default() }).unwrap();
        d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.font_family = Some(FAMILY.into()));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        assert!(cs.frames[0].lines.len() > 2, "{word} at {width}");
        for g in cs.frames[0].lines.iter().flat_map(|l| &l.glyphs) {
            if g.len == 0 && g.gid == g.face.glyph_for('\u{0640}') {
                let start = text[..g.byte].rfind(' ').map_or(0, |s| s + 1);
                found.insert(text[start..g.byte].chars().count());
            }
        }
    }
    found
}

#[test]
fn kashidas_follow_the_typographic_rules() {
    install_font();
    let words = [
        ("تصوم", None),
        ("نقود", None),
        ("يوم", None),
        ("لبني", Some(1)),
        ("هين", Some(0)),
        ("الكتاب", Some(3)),
        ("مدرسة", Some(3)),
        ("بكر", Some(0)),
        ("بلد", Some(0)),
        ("بتم", Some(0)),
        ("بيت", Some(0)),
        ("بنت", Some(0)),
        ("كبير", Some(1)),
        ("تبر", Some(0)),
        ("سير", Some(0)),
    ];
    for (word, expected) in words {
        assert_eq!(kashida_letters(word), expected.into_iter().collect::<BTreeSet<usize>>(), "{word}");
    }
}
