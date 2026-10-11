//! Right-to-left text in exported PDFs: content streams list it in reading order with marks after
//! their letters, lines with right-to-left text carry their reading-order text as ActualText, and
//! glyphs stay where composition put them.

use designcraft_compose::{Cache, ComposeOptions, compose_story};
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Align, Document, ParaAttrs, ParaFormat, SpreadRef, TextDirection};
use designcraft_geom::Rect;

use crate::tests::extract_text;
use crate::*;

const HEBREW: &str = "DC Test PDF Hebrew";
const ARABIC: &str = "DC Test PDF Arabic";

/// A stand-in Hebrew font: each letter and point has a glyph of its own (Latin capitals and
/// combining accents of the bundled face), so the ToUnicode map is unambiguous.
fn hebrew_font() {
    let letters = ['ד', 'ו', 'ש', 'ר', 'ה', 'י', 'א', 'ל', 'ב', 'ם', 'צ', 'ח', 'ק', 'ע'];
    let points = ['\u{5B7}', '\u{5B0}', '\u{5B8}', '\u{5B4}', '\u{5B2}', '\u{5B9}'];
    let accents = ['\u{300}', '\u{301}', '\u{302}', '\u{303}', '\u{304}', '\u{308}'];
    let mut map: Vec<(char, char)> = letters.iter().zip('A'..).map(|(h, l)| (*h, l)).collect();
    map.extend(points.iter().copied().zip(accents));
    map.extend(" &abcdef".chars().map(|c| (c, c)));
    designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::font_mapping(HEBREW, &map).expect("font"));
}

/// A stand-in Arabic font with a tatweel, so justification inserts kashidas.
fn arabic_font() {
    let map = [('ب', 'b'), ('س', 's'), ('م', 'm'), ('ا', 'a'), ('ل', 'l'), ('ه', 'h'), ('\u{640}', '_'), (' ', ' ')];
    designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::font_mapping(ARABIC, &map).expect("font"));
}

/// One frame of `text` in `family` with paragraph attributes `para`.
fn doc(text: &str, family: &str, r: Rect, para: ParaAttrs) -> Document {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    d.add_text_frame(SpreadRef::Doc(0), r, lid, text, ParaFormat { para, ..ParaFormat::default() }).unwrap();
    let sid = *d.stories.keys().next().unwrap();
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.font_family = Some(family.into()));
    d
}

fn rtl() -> ParaAttrs {
    ParaAttrs { direction: Some(TextDirection::RightToLeft), ..ParaAttrs::default() }
}

/// The ActualText of every line span (marked content with an MCID) on the first page.
fn line_actual_texts(bytes: &[u8]) -> Vec<String> {
    use hayro_syntax::content::ops::TypedInstruction;
    use hayro_syntax::object::{Dict, Object};
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).expect("parse");
    let page = &pdf.pages()[0];
    let mut out = Vec::new();
    let mut ops = page.typed_operations();
    while let Some(op) = ops.next() {
        let TypedInstruction::BeginMarkedContentWithProperties(op) = op else { continue };
        let Object::Dict(props) = op.1 else { continue };
        let props: &Dict<'_> = props;
        if !props.contains_key(b"MCID") {
            continue;
        }
        let Some(s) = props.get::<hayro_syntax::object::String>(b"ActualText") else { continue };
        let units: Vec<u16> = s.as_bytes().chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
        let text = String::from_utf16(&units).expect("UTF-16");
        out.push(text.trim_start_matches('\u{FEFF}').to_string());
    }
    out
}

#[test]
fn hebrew_extracts_in_reading_order_with_points_on_their_letters() {
    hebrew_font();
    let lines = ["דוד & שרה", "ישראל אַבְרָהָם יִצְחָק וְיַעֲקֹב", "דוד abc שרה"];
    let d = doc(&lines.join("\n"), HEBREW, Rect::new(36.0, 36.0, 576.0, 300.0), rtl());
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    // Glyph by glyph (ToUnicode, content order): each point right after its letter, once.
    assert_eq!(extract_text(&bytes)[0], lines.concat());
    // Viewers that order text by position read each line's ActualText.
    assert_eq!(line_actual_texts(&bytes), lines);
    // Tagged, the span is the line's tag under its paragraph.
    let tagged = export_pdf(&d, &Cache::new(), &PdfOptions { tagged: true, ..PdfOptions::default() }).unwrap();
    assert_eq!(line_actual_texts(&tagged), lines);
    assert_eq!(extract_text(&tagged)[0], lines.concat());
}

#[test]
fn hebrew_words_in_left_to_right_lines_extract_in_reading_order() {
    hebrew_font();
    let text = "abc דוד & שרה def";
    let d = doc(text, HEBREW, Rect::new(36.0, 36.0, 576.0, 300.0), ParaAttrs::default());
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert_eq!(extract_text(&bytes)[0], text);
    assert_eq!(line_actual_texts(&bytes), [text]);
    // A line without right-to-left text has no line span.
    let d = doc("abc def", HEBREW, Rect::new(36.0, 36.0, 576.0, 300.0), ParaAttrs::default());
    assert!(line_actual_texts(&export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap()).is_empty());
}

/// Device origin of every glyph drawn on the first page.
fn glyph_origins(bytes: &[u8]) -> Vec<kurbo::Point> {
    use hayro_interpret::font::Glyph;
    use hayro_interpret::{
        BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask,
        interpret_page,
    };
    struct Rec(Vec<kurbo::Point>);
    impl Device<'_> for Rec {
        fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
        fn set_blend_mode(&mut self, _: BlendMode) {}
        fn draw_path(&mut self, _: &kurbo::BezPath, _: kurbo::Affine, _: &Paint<'_>, _: &PathDrawMode) {}
        fn push_clip_path(&mut self, _: &ClipPath) {}
        fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
        fn draw_glyph(&mut self, g: &Glyph<'_>, xf: kurbo::Affine, gxf: kurbo::Affine, _: &Paint<'_>, _: &GlyphDrawMode) {
            if let Glyph::Outline(_) = g {
                self.0.push((xf * gxf) * kurbo::Point::ZERO);
            }
        }
        fn draw_image(&mut self, _: Image<'_, '_>, _: kurbo::Affine) {}
        fn pop_clip_path(&mut self) {}
        fn pop_transparency_group(&mut self) {}
    }
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).expect("parse");
    let cache = InterpreterCache::new();
    let mut ctx = Context::new(kurbo::Affine::IDENTITY, kurbo::Rect::new(0.0, 0.0, 1.0, 1.0), &cache, pdf.xref(), InterpreterSettings::default());
    let mut rec = Rec(Vec::new());
    interpret_page(&pdf.pages()[0], &mut ctx, &mut rec);
    rec.0
}

#[test]
fn pointed_letters_are_drawn_where_composed() {
    hebrew_font();
    let text = "יִצְחָק אַבְרָהָם\nיִצְחָק אַבְרָהָם";
    let mut d = doc(text, HEBREW, Rect::new(36.0, 36.0, 576.0, 300.0), rtl());
    let sid = *d.stories.keys().next().unwrap();
    // The second paragraph is skewed: points keep their place relative to their letters.
    let second = text.find('\n').unwrap() + 1;
    d.story_mut(sid).unwrap().format_chars(second..text.len(), |f| f.over.skew = Some(12.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let composed: Vec<kurbo::Point> =
        cs.frames[0].lines.iter().flat_map(|l| l.glyphs.iter().filter(|g| g.visible).map(|g| kurbo::Point::new(g.x, l.baseline + g.y))).collect();
    assert!(composed.iter().any(|p| (p.y - composed[0].y).abs() > 1.0 && (p.y - composed[0].y).abs() < 20.0), "the points sit off the baseline");
    let drawn = glyph_origins(&export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap());
    assert_eq!(drawn.len(), composed.len());
    // The page places the frame (y grows upwards on the page): match the two through their means.
    let mean = |ps: &[kurbo::Point]| ps.iter().fold(kurbo::Vec2::ZERO, |a, p| a + p.to_vec2()) / ps.len() as f64;
    let (c, k) = (mean(&composed), mean(&drawn));
    let key = |x: f64, y: f64| ((x * 100.0).round() as i64, (y * 100.0).round() as i64);
    let mut want: Vec<(i64, i64)> = composed.iter().map(|p| key(p.x - c.x, c.y - p.y)).collect();
    let mut got: Vec<(i64, i64)> = drawn.iter().map(|p| key(p.x - k.x, p.y - k.y)).collect();
    want.sort_unstable();
    got.sort_unstable();
    let off: Vec<_> = want.iter().zip(&got).filter(|(w, g)| (w.0 - g.0).abs() > 1 || (w.1 - g.1).abs() > 1).collect();
    assert!(off.is_empty(), "{off:?}");
}

#[test]
fn kashidas_extract_as_no_text() {
    arabic_font();
    let text = "بسم الله بسم الله بسم الله بسم الله بسم الله بسم الله بسم الله";
    let para = ParaAttrs { align: Some(Align::FullyJustified), ..rtl() };
    let d = doc(text, ARABIC, Rect::new(36.0, 36.0, 186.0, 300.0), para);
    let sid = *d.stories.keys().next().unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let kashida = |g: &designcraft_compose::PlacedGlyph| g.len == 0 && g.gid == g.face.glyph_for('\u{640}');
    assert!(cs.frames[0].lines.iter().any(|l| l.glyphs.iter().any(kashida)), "justification inserts kashidas");
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    let words: Vec<String> = extract_text(&bytes)[0].split([' ', '\u{FFFD}']).filter(|w| !w.is_empty()).map(str::to_string).collect();
    assert!(words.iter().all(|w| w == "بسم" || w == "الله"), "{words:?}");
    let spans = line_actual_texts(&bytes);
    assert!(spans.len() > 1 && spans.iter().all(|t| !t.contains('\u{640}') && text.contains(t.trim())), "{spans:?}");
}

#[test]
fn arabic_ligatures_extract_their_letters() {
    // Lam-alef in a system font with Arabic (skipped without one).
    let text = "لا سلام";
    let mut d = doc(text, designcraft_fonts::DEFAULT_FAMILY, Rect::new(36.0, 36.0, 576.0, 300.0), rtl());
    d.settings.glyph_fallback = true;
    let sid = *d.stories.keys().next().unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let glyphs = &cs.frames[0].lines[0].glyphs;
    if glyphs.iter().any(|g| g.gid == 0) || !glyphs.iter().any(|g| g.byte == 0 && g.len == "لا".len()) {
        return;
    }
    let bytes = export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert_eq!(extract_text(&bytes)[0], text);
    assert_eq!(line_actual_texts(&bytes), [text]);
}
