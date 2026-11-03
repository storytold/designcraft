use super::*;
use std::collections::BTreeMap;

const PROSE: &str =
    "ទ្រង់បានបង្កើតរបស់ខ្លះមកឲ្យអាចកម្រើកបាន និងខ្លះទៀតមិនអាចកម្រើកបានខ្លះស្អាត និងខ្លះទៀតមិនស្អាត ហើយរបស់ទាំងអស់នោះសុទ្ធតែមានគុណសម្បត្តិខុសៗគ្នា ប៉ុន្តែគ្មាននិចលភាព ឬក៏ស្មារតីសោះឡើយ។";

fn khmer_font_available() -> bool {
    let db = designcraft_fonts::FontDb::global();
    let face = db.face("Khmer Sangam MN", "Regular");
    let available = face.glyph_for('ក') != 0 || db.fallback_for('ក', face.id(), None).is_some();
    if !available {
        eprintln!("skipped Khmer layout: install a Khmer font or supply craft-fonts");
    }
    available
}

fn fixture(text: &str, width: f64, composer: Composer, align: Align) -> (Document, StoryId) {
    let (mut doc, sid, _) = doc_with(
        text,
        Rect::new(0.0, 0.0, width, 4000.0),
        ParaAttrs { composer: Some(composer), align: Some(align), hyphenate: Some(false), ..Default::default() },
    );
    doc.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.font_family = Some("Khmer Sangam MN".into());
        f.over.language = Some("Khmer".into());
        f.over.size = Some(14.0);
    });
    (doc, sid)
}
fn cluster_geometry(cs: &ComposedStory) -> BTreeMap<usize, Vec<(u32, f64, f64)>> {
    let mut groups: BTreeMap<usize, Vec<(u32, f64, f64)>> = BTreeMap::new();
    for g in cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).filter(|g| g.visible) {
        groups.entry(g.byte).or_default().push((g.gid, g.x, g.y));
    }
    for cluster in groups.values_mut() {
        let first = cluster[0].1;
        for g in cluster {
            g.1 -= first;
        }
    }
    groups
}

#[test]
fn justified_khmer_fills_lines_without_moving_attached_glyphs() {
    if !craft_khmer::data_available() || !khmer_font_available() {
        return;
    }
    let (natural_doc, sid) = fixture(PROSE, 10000.0, Composer::Paragraph, Align::Left);
    let natural = compose_story(&natural_doc, sid, &ComposeOptions::default());
    let reference = cluster_geometry(&natural);
    assert!(reference.values().any(|cluster| cluster.len() > 1), "exercise an actual multi-glyph Khmer cluster");
    for composer in [Composer::Paragraph, Composer::SingleLine] {
        for align in [Align::LeftJustified, Align::FullyJustified] {
            for width in [120.0, 220.0, 320.0] {
                let (doc, sid) = fixture(PROSE, width, composer, align);
                let cs = compose_story(&doc, sid, &ComposeOptions::default());
                assert!(!cs.is_overset());
                assert!(cs.line_count() >= 2);
                let last = all_lines(&cs).last().copied().unwrap();
                assert_eq!(last.range.end, PROSE.len());
                if align == Align::LeftJustified {
                    assert!(last.end_x < last.x1 - 0.5, "ordinary justified final line stays ragged");
                }
                for line in all_lines(&cs) {
                    if !line.last_in_para || align == Align::FullyJustified {
                        assert!((line.end_x - line.x1).abs() < 0.6, "{composer:?}/{align:?}/{width}: {} != {}", line.end_x, line.x1);
                    }
                    let start = PROSE.get(line.range.start..).unwrap().chars().next();
                    assert!(!start.is_some_and(|c| matches!(c, 'ៗ' | '។' | '\u{17b4}'..='\u{17d3}')));
                }
                for (byte, cluster) in cluster_geometry(&cs) {
                    let original = &reference[&byte];
                    assert_eq!(cluster.len(), original.len(), "cluster at {byte}");
                    for (a, b) in cluster.iter().zip(original) {
                        assert_eq!(a.0, b.0);
                        assert!((a.1 - b.1).abs() < 0.001 && (a.2 - b.2).abs() < 0.001, "mark moved at {byte}: {a:?} / {b:?}");
                    }
                }
                assert_eq!(doc.story(sid).unwrap().text, PROSE, "layout never inserts spaces");
            }
        }
    }
}

#[test]
fn khmer_authored_boundaries_survive_formatting_and_emergency_wraps() {
    if !khmer_font_available() {
        return;
    }
    let manual = "មូល\u{200b}ដ្ឋាន";
    let split = "មូល\u{200b}".len();
    for composer in [Composer::Paragraph, Composer::SingleLine] {
        let (wide, sid) = fixture(manual, 1000.0, composer, Align::Left);
        let natural = compose_story(&wide, sid, &ComposeOptions::default());
        let prefix_width: f64 = all_lines(&natural)[0].glyphs.iter().filter(|g| g.byte < split).map(|g| g.adv).sum();
        let (mut doc, sid) = fixture(manual, prefix_width + 0.1, composer, Align::Left);
        doc.story_mut(sid).unwrap().format_chars(split..manual.len(), |f| f.over.fill = Some("[Black]".into()));
        let cs = compose_story(&doc, sid, &ComposeOptions::default());
        assert_eq!(all_lines(&cs)[0].range.end, split, "manual opportunity inside a model word");
        assert_eq!(all_lines(&cs)[1].range.start, split);
        assert!(cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).filter(|g| g.rendered_char == '\u{200b}').all(|g| !g.visible));
        let (doc, sid) = fixture(manual, 150.0, composer, Align::FullyJustified);
        let justified = compose_story(&doc, sid, &ComposeOptions::default());
        let line = all_lines(&justified)[0];
        assert!((line.end_x - line.x1).abs() < 0.02, "manual ZWSP also supplies a justification gap");
        for (byte, cluster) in cluster_geometry(&justified) {
            let reference = cluster_geometry(&natural);
            for (a, b) in cluster.iter().zip(&reference[&byte]) {
                assert_eq!(a.0, b.0);
                assert!((a.1 - b.1).abs() < 0.001 && (a.2 - b.2).abs() < 0.001);
            }
        }
        for text in ["ខ្មែរ\u{2060}ភាសា", "គ្រប់ៗគ្នា។", "ក្ដៅខ្លាំង", "ក\u{200b}ិខ្មែរ"]
        {
            let (mut doc, sid) = fixture(text, 8.0, composer, Align::LeftJustified);
            // A shaping boundary inside the first syllable must not become a line
            // boundary: source clusters still protect the subscript/vowel.
            doc.story_mut(sid).unwrap().format_chars(3..text.len(), |f| f.over.size = Some(14.1));
            let cs = compose_story(&doc, sid, &ComposeOptions::default());
            for line in all_lines(&cs).into_iter().filter(|l| l.range.end < text.len()) {
                assert!(!line.range.is_empty(), "{composer:?}: a shaped cluster must not split across lines");
                let before = text[..line.range.end].chars().next_back().unwrap();
                let after = text[line.range.end..].chars().next().unwrap();
                assert!(!matches!(before, '\u{17d2}' | '\u{2060}'));
                assert!(!matches!(after, 'ៗ' | '។' | '\u{2060}' | '\u{17b4}'..='\u{17d3}'));
            }
            assert_eq!(doc.story(sid).unwrap().text, text);
        }
        let (mut doc, sid) = fixture("គ្រប់ៗគ្នា", 8.0, composer, Align::Left);
        doc.story_mut(sid).unwrap().format_chars(0.."គ្រប់ៗគ្នា".len(), |f| f.over.no_break = Some(true));
        assert_eq!(compose_story(&doc, sid, &ComposeOptions::default()).line_count(), 1);
    }
}

#[test]
fn misspelled_khmer_still_wraps_and_single_words_stay_intact() {
    if !craft_khmer::data_available() || !khmer_font_available() {
        return;
    }
    let text = "ខ្ញុំស្រឡាញ់ភាសាខ្មែររររររសួស្តីពិភពលោក";
    let (wide, sid) = fixture(text, 1000.0, Composer::Paragraph, Align::Left);
    let reference = cluster_geometry(&compose_story(&wide, sid, &ComposeOptions::default()));
    for composer in [Composer::Paragraph, Composer::SingleLine] {
        let (doc, sid) = fixture(text, 65.0, composer, Align::LeftJustified);
        let cs = compose_story(&doc, sid, &ComposeOptions::default());
        let lines = all_lines(&cs);
        assert!(lines.len() >= 3 && !cs.is_overset());
        assert_eq!(lines.last().unwrap().range.end, text.len());
        for line in &lines {
            assert!(line.end_x <= line.x1 + 0.02, "{composer:?}: {:?} overflowed", line.range);
        }
        for (byte, cluster) in cluster_geometry(&cs) {
            assert_eq!(cluster.len(), reference[&byte].len());
            for (a, b) in cluster.iter().zip(&reference[&byte]) {
                assert_eq!(a.0, b.0);
                assert!((a.1 - b.1).abs() < 0.001 && (a.2 - b.2).abs() < 0.001, "mark moved at {byte}");
            }
        }
        // Even expansive H&J settings must not manufacture Khmer letter gaps,
        // on either a forced line or the final line.
        for text in ["កសិករ", "កសិករ\u{2028}ខ្មែរ"] {
            let (natural, sid) = fixture(text, 100.0, composer, Align::Left);
            let natural = compose_story(&natural, sid, &ComposeOptions::default());
            for align in [Align::LeftJustified, Align::FullyJustified] {
                let (mut doc, sid) = fixture(text, 100.0, composer, align);
                doc.story_mut(sid).unwrap().format_paras(0..text.len(), |f| {
                    f.para.letter_space_desired = Some(0.2);
                    f.para.letter_space_max = Some(0.6);
                    f.para.glyph_scale_min = Some(0.9);
                    f.para.glyph_scale_max = Some(1.1);
                });
                let cs = compose_story(&doc, sid, &ComposeOptions::default());
                assert_eq!(cs.line_count(), natural.line_count());
                for (line, original) in all_lines(&cs).into_iter().zip(all_lines(&natural)) {
                    assert!((line.end_x - original.end_x).abs() < 0.001, "Khmer letters must never spread");
                }
            }
        }
    }
}

#[test]
fn khmer_mixed_text_keeps_other_scripts_and_arabic_word_joiner_keeps_joining() {
    let wide = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    for text in ["The quick brown fox jumps over the lazy dog.", "日本語の文字組版を確認します。"] {
        let (doc, sid, _) = doc_with(text, wide, ParaAttrs::default());
        let original = compose_story(&doc, sid, &ComposeOptions::default());
        let mixed = format!("{text} ខ្មែរ");
        let (doc, sid, _) = doc_with(&mixed, wide, ParaAttrs::default());
        let combined = compose_story(&doc, sid, &ComposeOptions::default());
        let geometry = |story: &ComposedStory| {
            all_lines(story)[0].glyphs.iter().filter(|g| g.byte < text.len()).map(|g| (g.gid, g.x, g.y, g.adv, g.sx)).collect::<Vec<_>>()
        };
        assert_eq!(geometry(&original), geometry(&combined));
    }
    let mut composed = Vec::new();
    for text in ["ببب", "بب\u{2060}ب"] {
        let (mut doc, sid, _) = doc_with(text, wide, ParaAttrs::default());
        doc.settings.glyph_fallback = true;
        let cs = compose_story(&doc, sid, &ComposeOptions::default());
        if all_lines(&cs)[0].glyphs.iter().any(|g| g.gid == 0) {
            return; // No Arabic font on this host.
        }
        composed.push(all_lines(&cs)[0].glyphs.iter().filter(|g| g.visible).map(|g| (g.gid, g.adv)).collect::<Vec<_>>());
    }
    assert_eq!(composed[0], composed[1], "WJ must not interrupt Arabic shaping");
}
