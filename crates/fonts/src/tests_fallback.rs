//! CJK fallback follows the character's language (and the script where it is unambiguous).

use super::FontDb;
use crate::testing::font_with;

const HAN: char = '直';
const HANGUL: char = '한';
const KANA: char = 'あ';
const BOPOMOFO: char = 'ㄅ';

/// A database of the bundled fonts (and the craft-fonts Japanese faces, when built with them)
/// only, no system fonts, plus `fonts` as (family, chars).
fn db_with(fonts: &[(&str, &[char])]) -> FontDb {
    let db = FontDb::with_font_dirs(vec![]);
    for (family, chars) in fonts {
        assert_eq!(db.add_font(font_with(family, chars).unwrap()), 1, "{family}");
    }
    db
}

fn family(db: &FontDb, c: char, language: Option<&str>) -> Option<String> {
    db.fallback_for(c, 0, language).map(|f| f.family.clone())
}

/// The families the Japanese chain finds, in order: the craft-fonts Japanese faces (none without
/// craft-fonts), then `installed`, the stand-ins for system fonts (in chain order).
fn japanese_chain_with(installed: &[&'static str]) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for f in crate::japanese_document_fonts() {
        if !v.contains(&f.family) {
            v.push(f.family);
        }
    }
    v.extend_from_slice(installed);
    v
}

/// What the language-blind search finds first for CJK text when `added` is the first added font
/// covering it: the craft-fonts Japanese faces load ahead of added fonts.
fn first_loaded(added: &'static str) -> &'static str {
    japanese_chain_with(&[added])[0]
}

#[test]
fn ideographs_follow_the_language() {
    let db = db_with(&[("Songti SC", &[HAN]), ("Songti TC", &[HAN]), ("AppleMyungjo", &[HAN]), ("Hiragino Mincho ProN", &[HAN])]);
    assert_eq!(family(&db, HAN, Some("zh-Hans")).as_deref(), Some("Songti SC"));
    assert_eq!(family(&db, HAN, Some("zh")).as_deref(), Some("Songti SC"), "plain Chinese is Simplified");
    assert_eq!(family(&db, HAN, Some("zh-Hant")).as_deref(), Some("Songti TC"));
    assert_eq!(family(&db, HAN, Some("ko")).as_deref(), Some("AppleMyungjo"));
    assert_eq!(family(&db, HAN, Some("ko-KR")).as_deref(), Some("AppleMyungjo"), "the language decides, whatever the region");
    let ja = japanese_chain_with(&["Hiragino Mincho ProN"])[0];
    assert_eq!(family(&db, HAN, Some("ja")).as_deref(), Some(ja), "the craft-fonts faces lead the Japanese chain, then system fonts");
    assert_eq!(family(&db, HAN, Some("ja-JP")).as_deref(), Some(ja));
    // CJK punctuation follows the language too.
    let db = db_with(&[("Other CJK", &['、']), ("Songti SC", &['、'])]);
    assert_eq!(family(&db, '、', Some("zh-Hans")).as_deref(), Some("Songti SC"));
    assert_eq!(family(&db, '、', None).as_deref(), Some(first_loaded("Other CJK")));
}

#[test]
fn ideographs_without_a_cjk_language_fall_back_as_before() {
    let db = db_with(&[("Other CJK", &[HAN]), ("Songti SC", &[HAN]), ("AppleMyungjo", &[HAN])]);
    for language in [None, Some("en-US"), Some("tr"), Some("not a tag")] {
        assert_eq!(family(&db, HAN, language).as_deref(), Some(first_loaded("Other CJK")), "{language:?}");
    }
}

#[test]
fn the_script_decides_where_it_is_unambiguous() {
    // A non-chain font loaded first would win the language-blind search.
    let db = db_with(&[
        ("Other CJK", &[HANGUL, KANA, BOPOMOFO]),
        ("AppleMyungjo", &[HANGUL]),
        ("Songti TC", &[BOPOMOFO]),
        ("Yu Mincho", &[KANA]),
        ("Hiragino Mincho ProN", &[KANA]),
    ]);
    for language in [None, Some("en-US"), Some("ja"), Some("zh-Hans")] {
        assert_eq!(family(&db, HANGUL, language).as_deref(), Some("AppleMyungjo"), "Hangul is Korean: {language:?}");
        assert_eq!(family(&db, BOPOMOFO, language).as_deref(), Some("Songti TC"), "Bopomofo is Traditional Chinese: {language:?}");
    }
    // Hiragino Mincho ProN comes before Yu Mincho in the chain, whatever order they loaded in.
    let chain = japanese_chain_with(&["Hiragino Mincho ProN", "Yu Mincho"]);
    for language in [None, Some("zh-Hans"), Some("ko")] {
        assert_eq!(family(&db, KANA, language).as_deref(), Some(chain[0]), "kana is Japanese: {language:?}");
    }
    // Past the faces the chain can't use (here its first, as the primary font), the next.
    let first = db.face(chain[0], "Regular");
    assert_eq!(db.fallback_for(KANA, first.id(), None).map(|f| f.family.clone()).as_deref(), Some(chain[1]));
}

#[test]
fn without_a_chain_font_the_fallback_is_unchanged() {
    let db = db_with(&[("Other CJK", &[HANGUL])]);
    // Without craft-fonts nothing covers the ideograph; with it, its first Japanese face does.
    let craft = japanese_chain_with(&[]).first().copied();
    for language in ["zh-Hans", "zh-Hant", "ko", "ja"] {
        assert_eq!(family(&db, HAN, Some(language)).as_deref(), craft, "{language}");
    }
    assert_eq!(family(&db, HANGUL, Some("ko")).as_deref(), Some("Other CJK"));
    assert_eq!(family(&db, BOPOMOFO, Some("zh-Hant")), None, "nothing covers it");
}

#[test]
fn chain_fonts_load_from_the_font_folders_when_first_needed() {
    let dir = std::env::temp_dir().join(format!("dc-cjk-fallback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("songti.ttf"), font_with("Songti SC", &[HAN]).unwrap()).unwrap();
    std::fs::write(dir.join("myungjo.ttf"), font_with("AppleMyungjo", &[HAN]).unwrap()).unwrap();
    // An added font covers the ideograph, so the language-blind search never scans the folder.
    let other = || font_with("Other CJK", &[HAN]).unwrap();

    let db = FontDb::with_font_dirs(vec![dir.clone()]);
    db.add_font(other());
    assert_eq!(family(&db, HAN, None).as_deref(), Some(first_loaded("Other CJK")));
    assert!(!db.is_loaded("Songti SC"), "no language, no chain font loaded");
    assert_eq!(family(&db, HAN, Some("zh-Hans")).as_deref(), Some("Songti SC"));
    assert!(!db.is_loaded("AppleMyungjo"), "only what the chain needed");
    assert_eq!(family(&db, HAN, None).as_deref(), Some(first_loaded("Other CJK")), "once loaded, still not used without the language");

    // With the system fallback off, installed chain fonts aren't loaded.
    let db = FontDb::with_font_dirs(vec![dir]);
    db.add_font(other());
    db.set_system_fallback(false);
    assert_eq!(family(&db, HAN, Some("ko")).as_deref(), Some(first_loaded("Other CJK")));
    assert!(!db.is_loaded("AppleMyungjo"));
}

const KA: char = 'ക';
const VIRAMA: char = '\u{0D4D}';
const AA: char = '\u{0D3E}';

#[test]
fn malayalam_takes_a_font_made_for_it() {
    // Arial Unicode MS maps Malayalam but can't shape it (no conjuncts); loaded first, it won.
    let db = db_with(&[("Arial Unicode MS", &[KA, VIRAMA, AA]), ("Malayalam Sangam MN", &[KA, VIRAMA, AA])]);
    for c in [KA, VIRAMA, AA] {
        for language in [None, Some("en-US"), Some("ml")] {
            assert_eq!(family(&db, c, language).as_deref(), Some("Malayalam Sangam MN"), "{c:?} {language:?}");
        }
    }
    // Past the Malayalam fonts, the language-blind search as before.
    let sangam = db.face("Malayalam Sangam MN", "Regular");
    assert_eq!(db.fallback_for(KA, sangam.id(), None).map(|f| f.family.clone()).as_deref(), Some("Arial Unicode MS"));
}

#[test]
fn malayalam_fonts_load_from_the_font_folders_when_first_needed() {
    let dir = std::env::temp_dir().join(format!("dc-malayalam-fallback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("noto.ttf"), font_with("Noto Sans Malayalam", &[KA]).unwrap()).unwrap();
    let db = FontDb::with_font_dirs(vec![dir]);
    db.add_font(font_with("Arial Unicode MS", &[KA]).unwrap());
    assert!(!db.is_loaded("Noto Sans Malayalam"));
    assert_eq!(family(&db, KA, None).as_deref(), Some("Noto Sans Malayalam"));
}
