#[cfg(all(test, target_os = "linux"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(super) struct SysFallback {
    pub(super) enabled: bool,
    /// Rescan generation: an older in-flight lookup must not restore a cleared miss.
    pub(super) generation: u64,
    pub(super) misses: std::collections::HashSet<char>,
    /// CJK chain families already looked up in the catalog (loaded, or not installed).
    pub(super) chain_tried: std::collections::HashSet<&'static str>,
}

#[cfg(all(test, target_os = "linux"))]
pub(super) struct FallbackSnapshotHook {
    pub(super) reached: std::sync::mpsc::SyncSender<()>,
    pub(super) resume: Arc<std::sync::Barrier>,
}

/// Families tried (when installed) for characters the loaded fonts lack: CJK, symbols, emoji.
#[cfg(not(target_arch = "wasm32"))]
pub(super) const SYSTEM_FALLBACKS: &[&str] = &[
    "Helvetica Neue",
    "Arial",
    "Segoe UI",
    "Noto Sans",
    "DejaVu Sans",
    "PingFang SC",
    "Hiragino Sans",
    "Hiragino Kaku Gothic ProN",
    "Apple SD Gothic Neo",
    "Heiti SC",
    "STHeiti",
    "Microsoft YaHei",
    "Yu Gothic",
    "Malgun Gothic",
    "Noto Sans CJK SC",
    "Noto Sans CJK JP",
    "Arial Unicode MS",
    "Apple Symbols",
    "Segoe UI Symbol",
    "Noto Sans Symbols",
    "Noto Sans Symbols2",
    "Noto Emoji",
    "Segoe UI Emoji",
    "Apple Color Emoji",
    "Noto Color Emoji",
];

/// CJK fallback chains: the families tried first (user-added, else installed) for characters of
/// each language that the primary font lacks. Each lists macOS, then Windows, then Linux /
/// cross-platform families, serif first; a platform only has its own installed. Japanese tries
/// the craft-fonts Japanese faces before these ([`japanese_chain`]).
const JAPANESE_FALLBACKS: &[&str] = &[
    "Hiragino Mincho ProN",
    "YuMincho",
    "Hiragino Sans",
    "Yu Mincho",
    "MS Mincho",
    "Yu Gothic",
    "Noto Serif CJK JP",
    "Source Han Serif JP",
    "Noto Serif JP",
    "Noto Sans CJK JP",
    "Source Han Sans JP",
];

const SIMPLIFIED_CHINESE_FALLBACKS: &[&str] = &[
    "Songti SC",
    "STSong",
    "PingFang SC",
    "SimSun",
    "Microsoft YaHei",
    "Noto Serif CJK SC",
    "Source Han Serif SC",
    "Noto Serif SC",
    "Noto Sans CJK SC",
    "Source Han Sans SC",
];

const TRADITIONAL_CHINESE_FALLBACKS: &[&str] = &[
    "Songti TC",
    "PingFang TC",
    "PMingLiU",
    "MingLiU",
    "Microsoft JhengHei",
    "Noto Serif CJK TC",
    "Source Han Serif TC",
    "Noto Serif TC",
    "Noto Sans CJK TC",
    "Source Han Sans TC",
];

/// Korean fallbacks for a missing **sans** (Dotum/Gothic) face, system faces first.
///
/// [`KOREAN_FALLBACKS`] is serif-first, so on Windows the first installed entry is Batang and a
/// document whose Korean text asked for KoPub Dotum or BM Dohyeon came out in a serif face --
/// 5,298 characters of one school document, turning it from sans to serif (#1692).
pub(super) const KOREAN_SANS_FALLBACKS: &[&str] = &[
    "Malgun Gothic",
    "Apple SD Gothic Neo",
    "Dotum",
    "Gulim",
    "Noto Sans CJK KR",
    "Source Han Sans K",
    "Noto Sans KR",
    "Noto Serif CJK KR",
    "Batang",
];

/// Whether a Korean family name says the face is sans (`Some(true)`) or serif (`Some(false)`).
/// `None` when the name does not say, so the caller decides.
///
/// Korean family names carry the style: Dotum (돋움), Gulim (굴림) and Gothic (고딕) are sans;
/// Batang (바탕), Myungjo/Myeongjo (명조) and Gungsuh (궁서) are serif. A missing font's name is
/// the only thing left to go on, since the font itself is not there to classify (#1692).
pub fn sans_by_name(family: &str) -> Option<bool> {
    let lower = family.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    // A Korean serif name comes first: "Myungjo Gothic" does not exist, but a name holding both
    // reads as the serif it names, and "Sans" inside "Noto Sans Myungjo" would otherwise win.
    if has(&["batang", "myungjo", "myeongjo", "gungsuh", "\u{BC14}\u{D5D5}", "\u{BA85}\u{C870}", "\u{AD81}\u{C11C}"]) {
        return Some(false);
    }
    // Then the word itself, which outranks every sans indicator: "Sweet Gothic Serif" is a serif
    // family whatever "Gothic" suggests. "Sans serif" and "sans-serif" end with the word and do
    // not mean it, so the phrase goes before the word is looked for (both noted in review).
    if lower.replace("sans-serif", "sans").replace("sans serif", "sans").contains("serif") {
        return Some(false);
    }
    if has(&[
        "dotum",
        "gothic",
        "gulim",
        "sans",
        "headline",
        "graphic",
        "dohyeon",
        "\u{B3CB}\u{C6C0}",
        "\u{ACE0}\u{B515}",
        "\u{AD74}\u{B9BC}",
        "\u{ADF8}\u{B798}\u{D53D}",
    ]) {
        return Some(true);
    }
    None
}

pub(super) const KOREAN_FALLBACKS: &[&str] = &[
    "AppleMyungjo",
    "Apple SD Gothic Neo",
    "Batang",
    "Malgun Gothic",
    "Noto Serif CJK KR",
    "Source Han Serif K",
    "Noto Serif KR",
    "Noto Sans CJK KR",
    "Source Han Sans K",
];

/// Fonts made for Malayalam (macOS, Windows, Noto, Linux). A font that only maps the characters
/// (Arial Unicode MS, first by path on macOS) sets them without conjuncts or vowel signs.
const MALAYALAM_FALLBACKS: &[&str] = &[
    "Malayalam Sangam MN",
    "Malayalam MN",
    "Nirmala UI",
    "Kartika",
    "Noto Sans Malayalam",
    "Noto Serif Malayalam",
    "Manjari",
    "Rachana",
    "Meera",
    "Lohit Malayalam",
];

/// The fallback chain of a script that only fonts made for it set well, whatever the language.
pub(super) fn script_chain(c: char) -> Option<&'static [&'static str]> {
    match c as u32 {
        0x0D00..=0x0D7F => Some(MALAYALAM_FALLBACKS),
        _ => None,
    }
}

/// The Japanese fallback chain: the craft-fonts Japanese faces' families in document order
/// (Mincho first; none without craft-fonts), then [`JAPANESE_FALLBACKS`].
fn japanese_chain() -> &'static [&'static str] {
    static CHAIN: std::sync::OnceLock<Vec<&str>> = std::sync::OnceLock::new();
    CHAIN.get_or_init(|| {
        let mut chain: Vec<&'static str> = Vec::new();
        for f in crate::japanese_document_fonts() {
            if !chain.contains(&f.family) {
                chain.push(f.family);
            }
        }
        chain.extend_from_slice(JAPANESE_FALLBACKS);
        chain
    })
}

/// The CJK fallback chain for `c` in `language` (a BCP 47 tag). The script decides where it is
/// unambiguous (Hangul is Korean, kana Japanese, Bopomofo Traditional Chinese); ideographs and CJK
/// punctuation follow the language, and have no chain without a CJK one.
pub(super) fn cjk_chain(c: char, language: Option<&str>, prefer_sans: Option<bool>) -> Option<&'static [&'static str]> {
    let korean = if prefer_sans == Some(true) { KOREAN_SANS_FALLBACKS } else { KOREAN_FALLBACKS };
    match c as u32 {
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xFFA0..=0xFFDC => return Some(korean),
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F | 0x1B000..=0x1B16F => return Some(japanese_chain()),
        0x3100..=0x312F | 0x31A0..=0x31BF => return Some(TRADITIONAL_CHINESE_FALLBACKS),
        // Radicals, CJK symbols and punctuation, kanbun, strokes, enclosed and compatibility
        // characters, ideographs, compatibility and vertical forms, full-width forms.
        0x2E80..=0x2FDF
        | 0x3000..=0x303F
        | 0x3190..=0x319F
        | 0x31C0..=0x31EF
        | 0x3200..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF
        | 0xFE10..=0xFE1F
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFFEF
        | 0x20000..=0x3FFFF => {}
        _ => return None,
    }
    // The language decides, whatever the region (`ja-JP`, `ko-KR`).
    let language = language?;
    match language.split('-').next()? {
        "ja" => Some(japanese_chain()),
        "ko" => Some(korean),
        "zh" if language.starts_with("zh-Hant") => Some(TRADITIONAL_CHINESE_FALLBACKS),
        "zh" => Some(SIMPLIFIED_CHINESE_FALLBACKS),
        _ => None,
    }
}
