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

/// With a required pack, the data-gated tests below must not pass by skipping.
#[test]
fn required_data_pack_is_available() {
    if option_env!("CRAFT_KHMER_REQUIRED") == Some("1") {
        assert!(craft_khmer::data_available(), "required Khmer data must activate the composition tests");
    }
}

#[test]
fn justified_khmer_fills_only_lines_with_real_spaces_without_moving_attached_glyphs() {
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
                    if !line.last_in_para && PROSE[line.range.clone()].trim_end().contains(' ') {
                        assert!((line.end_x - line.x1).abs() < 0.6, "{composer:?}/{align:?}/{width}: {} != {}", line.end_x, line.x1);
                    }
                    assert!(line.end_x <= line.x1 + 0.02, "{composer:?}/{align:?}/{width}: overflow");
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
        let natural_end = all_lines(&natural)[0].end_x;
        assert!((line.end_x - natural_end).abs() < 0.001, "a ZWSP permits a break; it never opens a gap");
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

const WORD: &str = "ភាសាខ្មែរ";

/// A font with fixed metrics, so these tests depend on neither a host font nor the data pack.
fn synthetic_font() -> &'static str {
    const FAMILY: &str = "DC Test Khmer";
    static ADDED: std::sync::Once = std::sync::Once::new();
    ADDED.call_once(|| {
        let glyphs = [('ភ', 'a'), ('ា', 'b'), ('ស', 'c'), ('ខ', 'd'), ('្', 'e'), ('ម', 'f'), ('ែ', 'g'), ('រ', 'h'), (' ', ' ')];
        designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::font_mapping(FAMILY, &glyphs).unwrap());
    });
    FAMILY
}
fn synthetic(text: &str, width: f64, composer: Composer, align: Align) -> (Document, StoryId) {
    let family = synthetic_font();
    let (mut doc, sid) = fixture(text, width, composer, align);
    doc.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.font_family = Some(family.into()));
    (doc, sid)
}
fn word_width(composer: Composer) -> f64 {
    let (doc, sid) = synthetic(WORD, 10000.0, composer, Align::Left);
    all_lines(&compose_story(&doc, sid, &ComposeOptions::default()))[0].end_x
}
/// (advance, horizontal scale) of each glyph at its source byte, in reading order.
fn glyph_metrics(cs: &ComposedStory) -> BTreeMap<usize, Vec<(f64, f64)>> {
    let mut out: BTreeMap<usize, Vec<(f64, f64)>> = BTreeMap::new();
    for g in cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs) {
        out.entry(g.byte).or_default().push((g.adv, g.sx));
    }
    out
}
fn glyph_scaling_limits(doc: &mut Document, sid: StoryId, text: &str) {
    doc.story_mut(sid).unwrap().format_paras(0..text.len(), |f| {
        f.para.word_space_min = Some(1.0);
        f.para.word_space_desired = Some(1.0);
        f.para.word_space_max = Some(1.0);
        f.para.letter_space_min = Some(0.0);
        f.para.letter_space_desired = Some(0.0);
        f.para.letter_space_max = Some(0.0);
        f.para.glyph_scale_min = Some(0.96);
        f.para.glyph_scale_desired = Some(1.0);
        f.para.glyph_scale_max = Some(1.04);
        f.para.single_word_justify = Some(Align::Left);
    });
}

#[test]
fn word_boundaries_never_open_a_gap_and_real_spaces_take_the_length() {
    // Lines without a real space (their boundaries are authored), then lines with them.
    let text = format!("{} {}", [WORD; 24].join("\u{200b}"), [WORD; 24].join(" "));
    for composer in [Composer::Paragraph, Composer::SingleLine] {
        let width = word_width(composer) * 7.5;
        let (wide, sid) = synthetic(&text, 100000.0, composer, Align::Left);
        let natural = glyph_metrics(&compose_story(&wide, sid, &ComposeOptions::default()));
        let space = text.find(' ').map(|at| natural[&at][0].0).unwrap();
        for align in [Align::LeftJustified, Align::FullyJustified] {
            let (doc, sid) = synthetic(&text, width, composer, align);
            let cs = compose_story(&doc, sid, &ComposeOptions::default());
            let (mut spaced, mut unspaced, mut grown) = (0, 0, false);
            for line in all_lines(&cs) {
                for g in &line.glyphs {
                    if g.rendered_char == ' ' {
                        grown |= g.adv > space + 0.01;
                    } else {
                        let same = natural[&g.byte].iter().any(|r| (r.0 - g.adv).abs() < 1e-6);
                        assert!(same, "{composer:?}/{align:?}: a letter widened at {}", g.byte);
                    }
                }
                if line.last_in_para {
                    continue;
                }
                let source = &text[line.range.clone()];
                if source.trim_end().contains(' ') {
                    spaced += 1;
                    assert!((line.end_x - line.x1).abs() < 0.02, "{composer:?}/{align:?}: a line with a real space fills the measure");
                } else {
                    unspaced += 1;
                    let end = line.range.start + source.trim_end().len();
                    let natural_width: f64 = natural.range(line.range.start..end).flat_map(|(_, m)| m).map(|m| m.0).sum();
                    assert!((line.end_x - line.x0 - natural_width).abs() < 1e-6, "{composer:?}/{align:?}: no real space, no justification");
                    assert!(line.end_x <= line.x1 + 0.02);
                }
            }
            assert!(spaced > 0 && unspaced > 0 && grown, "{composer:?}/{align:?}: exercise both kinds of line ({spaced}/{unspaced}/{grown})");
            assert_eq!(doc.story(sid).unwrap().text, text, "layout never inserts spaces");
        }
    }
}

#[test]
fn a_real_space_added_by_the_author_justifies_the_line() {
    for composer in [Composer::Paragraph, Composer::SingleLine] {
        let width = word_width(composer) * 3.0;
        for (text, fills) in [(format!("{WORD}\u{200b}{WORD}"), false), (format!("{WORD} {WORD}"), true)] {
            let (doc, sid) = synthetic(&text, width, composer, Align::FullyJustified);
            let cs = compose_story(&doc, sid, &ComposeOptions::default());
            let lines = all_lines(&cs);
            assert_eq!(lines.len(), 1);
            assert_eq!((lines[0].end_x - lines[0].x1).abs() < 0.02, fills, "{composer:?}: {text}");
        }
    }
}

/// Maximum glyph scaling, with a space taking the rest of a line far wider than the limits reach.
fn scaled_to_maximum(text: &str, family: Option<&str>, composer: Composer) -> (ComposedStory, ComposedStory) {
    let build = |width: f64, align: Align| {
        let (mut doc, sid) = fixture(text, width, composer, align);
        if let Some(family) = family {
            doc.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.font_family = Some(family.into()));
        }
        glyph_scaling_limits(&mut doc, sid, text);
        compose_story(&doc, sid, &ComposeOptions::default())
    };
    let natural = build(10000.0, Align::Left);
    let width = all_lines(&natural)[0].end_x * 1.5;
    (natural, build(width, Align::FullyJustified))
}

#[test]
fn glyph_scaling_stays_in_its_limits_and_real_spaces_take_the_rest() {
    let text = format!("{WORD} {WORD}");
    for composer in [Composer::Paragraph, Composer::SingleLine] {
        let (natural, cs) = scaled_to_maximum(&text, Some(synthetic_font()), composer);
        let lines = all_lines(&cs);
        assert_eq!(lines.len(), 1);
        assert!((lines[0].end_x - lines[0].x1).abs() < 0.02, "{composer:?}");
        let reference = glyph_metrics(&natural);
        for (byte, glyphs) in glyph_metrics(&cs) {
            for (g, original) in glyphs.iter().zip(&reference[&byte]) {
                if text[byte..].starts_with(' ') {
                    assert!(g.0 > original.0 * 2.0, "{composer:?}: the space takes the rest");
                } else {
                    assert!((g.0 - original.0 * 1.04).abs() < 1e-6, "{composer:?}: {byte} past the maximum");
                    assert!((g.1 - original.1 * 1.04).abs() < 1e-6, "{composer:?}: {byte} past the maximum");
                }
            }
        }
    }
}

#[test]
fn glyph_scaling_keeps_attached_marks_on_their_base() {
    if !khmer_font_available() {
        return;
    }
    let text = "ក្តី ខ្មែរ";
    let (natural, cs) = scaled_to_maximum(text, None, Composer::Paragraph);
    let reference = cluster_geometry(&natural);
    let metrics = glyph_metrics(&natural);
    assert!(metrics.values().flatten().any(|m| m.0 == 0.0), "exercise a zero-advance mark");
    for (byte, cluster) in cluster_geometry(&cs) {
        for (a, b) in cluster.iter().zip(&reference[&byte]) {
            assert_eq!(a.0, b.0);
            assert!((a.1 - b.1 * 1.04).abs() < 1e-6, "an attached glyph left its base at {byte}: {a:?} / {b:?}");
            assert!((a.2 - b.2).abs() < 1e-6, "glyph scaling is horizontal");
        }
    }
    for (byte, glyphs) in glyph_metrics(&cs) {
        if !text[byte..].starts_with(' ') {
            for (g, original) in glyphs.iter().zip(&metrics[&byte]) {
                assert!((g.1 - original.1 * 1.04).abs() < 1e-6, "every glyph of a cluster scales together at {byte}");
            }
        }
    }
}

#[test]
fn glyph_scaling_never_stretches_a_line_without_a_space_and_shrinks_only_to_its_minimum() {
    let text = format!("{WORD}\u{200b}{WORD}");
    for composer in [Composer::Paragraph, Composer::SingleLine] {
        for align in [Align::LeftJustified, Align::FullyJustified] {
            let compose = |width: f64| {
                let (mut doc, sid) = synthetic(&text, width, composer, align);
                glyph_scaling_limits(&mut doc, sid, &text);
                compose_story(&doc, sid, &ComposeOptions::default())
            };
            let natural_width = word_width(composer) * 2.0;
            // Room to spare: no stretch, flush with the start of the line.
            let roomy = compose(natural_width * 1.02);
            let line = all_lines(&roomy)[0];
            assert!((line.end_x - natural_width).abs() < 1e-6, "{composer:?}/{align:?}: {} != {natural_width}", line.end_x);
            // Slightly too wide, as the paragraph's last line too: glyph scaling fits it.
            let tight = natural_width / 1.02;
            let squeezed = compose(tight);
            assert_eq!(squeezed.line_count(), 1, "{composer:?}/{align:?}");
            assert!((all_lines(&squeezed)[0].end_x - tight).abs() < 0.02, "{composer:?}/{align:?}: {}", all_lines(&squeezed)[0].end_x);
            // Past the minimum it wraps at a boundary instead.
            let narrow = natural_width / 1.1;
            let wrapped = compose(narrow);
            assert!(wrapped.line_count() >= 2, "{composer:?}/{align:?}");
            for line in all_lines(&wrapped) {
                assert!(line.end_x <= narrow + 0.02, "{composer:?}/{align:?}: {} > {narrow}", line.end_x);
            }
        }
    }
}

#[test]
fn single_word_justification_places_a_khmer_line_without_a_space() {
    let text = format!("{WORD}\u{200b}{WORD}");
    for composer in [Composer::Paragraph, Composer::SingleLine] {
        let width = word_width(composer) * 3.0;
        let natural = word_width(composer) * 2.0;
        // The first line is not the last, so Single Word Justification decides its place.
        let forced = format!("{text}\u{2028}{text}");
        for (setting, start) in
            [(Align::Left, 0.0), (Align::FullyJustified, 0.0), (Align::Center, (width - natural) / 2.0), (Align::Right, width - natural)]
        {
            let (mut doc, sid) = synthetic(&forced, width, composer, Align::FullyJustified);
            doc.story_mut(sid).unwrap().format_paras(0..forced.len(), |f| f.para.single_word_justify = Some(setting));
            let cs = compose_story(&doc, sid, &ComposeOptions::default());
            let line = all_lines(&cs)[0];
            assert!((line.glyphs[0].x - start).abs() < 1e-6, "{composer:?}/{setting:?}: {} != {start}", line.glyphs[0].x);
            assert!((line.end_x - start - natural).abs() < 1e-6, "{composer:?}/{setting:?}: letters keep their width");
        }
    }
}

#[test]
fn zero_width_characters_in_other_scripts_keep_their_letter_spacing() {
    let text = "alpha\u{200b}beta gamma\u{2060}delta";
    for composer in [Composer::Paragraph, Composer::SingleLine] {
        let (mut doc, sid, _) = doc_with(
            text,
            Rect::new(0.0, 0.0, 1000.0, 1000.0),
            ParaAttrs { composer: Some(composer), align: Some(Align::FullyJustified), ..Default::default() },
        );
        doc.story_mut(sid).unwrap().format_paras(0..text.len(), |f| {
            f.para.letter_space_max = Some(0.5);
            f.para.word_space_max = Some(1.0);
        });
        let cs = compose_story(&doc, sid, &ComposeOptions::default());
        let line = all_lines(&cs)[0];
        let before_zwsp = line.glyphs.iter().find(|g| g.byte == 4).unwrap();
        let interior = line.glyphs.iter().find(|g| g.byte == 3).unwrap();
        assert!(before_zwsp.adv > interior.adv * 0.5, "{composer:?}: the gap before a ZWSP spreads as on any other letter");
        let (plain, sid2, _) = doc_with("a", Rect::new(0.0, 0.0, 1000.0, 1000.0), ParaAttrs::default());
        let natural = all_lines(&compose_story(&plain, sid2, &ComposeOptions::default()))[0].glyphs[0].adv;
        assert!(before_zwsp.adv > natural + 0.01, "{composer:?}: {} <= {natural}", before_zwsp.adv);
    }
}

#[test]
fn word_count_uses_khmer_boundaries_and_leaves_other_text_alone() {
    const PHRASE: &str = "ខ្ញុំស្រឡាញ់ភាសាខ្មែរ";
    for (text, words) in [("", 0), ("Hello world", 2), ("one - two", 3), ("日本語の文字", 1)] {
        assert_eq!(word_count(text), words, "{text}: whitespace counting is unchanged");
    }
    assert_eq!(word_count("ខ្មែរ\u{200b}ភាសា"), 2, "authored breaks work even without a dictionary");
    assert_eq!(word_count("ខ្មែរ ភាសា"), 2);
    assert_eq!(word_count("ៗ។! ា្"), 0, "detached marks and signs are not words");
    if craft_khmer::data_available() {
        assert_eq!(word_count(PHRASE), 4, "Khmer counts by word boundaries, not real spaces");
        assert_eq!(word_count(&format!("{PHRASE}\r\n{PHRASE}\n")), 8);
        assert_eq!(word_count(&format!("Hello{PHRASE}world ១២៣ 42")), 8);
        assert_eq!(word_count("អ្នកចម្រៀង ក្រុមហ៊ុន អ្វីៗ"), 4);
        assert_eq!(word_count(PHRASE), 4, "a cached paragraph counts the same");
    } else {
        assert_eq!(word_count(PHRASE), 1);
    }
}
