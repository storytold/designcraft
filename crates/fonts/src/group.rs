//! Font menu groups: the script or CJK language a font is for and its family name in that
//! language, read from the font's own data.

use skrifa::raw::TableProvider;

/// The font menus' groups, in menu order: Western (and other scripts), then Japanese, Simplified
/// Chinese, Traditional Chinese and Korean. The order is InDesign's font menu as recalled, not
/// observed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FontGroup {
    #[default]
    Western,
    Japanese,
    ChineseSimplified,
    ChineseTraditional,
    Korean,
}

/// A family as the font menus list it.
#[derive(Clone, Debug, PartialEq)]
pub struct FamilyInfo {
    /// The family name documents store (the English or only name).
    pub family: String,
    pub group: FontGroup,
    /// A CJK family's name in its group's language, when the font has one that differs.
    pub native: Option<String>,
}

impl FamilyInfo {
    /// The name the menus show: the native name unless `english` (Show Font Names in English).
    pub fn label(&self, english: bool) -> &str {
        match &self.native {
            Some(n) if !english => n,
            _ => &self.family,
        }
    }

    /// Does `query` (lowercase) find this family? Either name matches.
    pub fn matches(&self, query: &str) -> bool {
        query.is_empty() || self.family.to_lowercase().contains(query) || self.native.as_ref().is_some_and(|n| n.to_lowercase().contains(query))
    }
}

/// Sort families for a font menu: by group, then by the name shown, ignoring case.
pub fn sort_for_menu(families: &mut [FamilyInfo], english: bool) {
    families.sort_by_cached_key(|f| (f.group, f.label(english).to_lowercase()));
}

/// Language IDs naming a group: Windows (platform 3) and Macintosh (platform 1).
fn group_of_language(platform: u16, language: u16) -> Option<FontGroup> {
    match (platform, language) {
        (3, 0x0411) | (1, 11) => Some(FontGroup::Japanese),
        (3, 0x0804 | 0x1004) | (1, 33) => Some(FontGroup::ChineseSimplified),
        (3, 0x0404 | 0x0C04 | 0x1404) | (1, 19) => Some(FontGroup::ChineseTraditional),
        (3, 0x0412) | (1, 23) => Some(FontGroup::Korean),
        _ => None,
    }
}

/// The group of a `meta` script and language tag (`Jpan`, `ja`, `zh-Hant`, `ko-Kore`, …).
fn group_of_tag(tag: &str) -> Option<FontGroup> {
    let tag = tag.trim().to_ascii_lowercase();
    let parts: Vec<&str> = tag.split(['-', '_']).collect();
    let has = |p: &str| parts.contains(&p);
    let first = parts.first().copied().unwrap_or("");
    if has("jpan") || has("hira") || has("kana") || first == "ja" {
        Some(FontGroup::Japanese)
    } else if has("kore") || has("hang") || first == "ko" {
        Some(FontGroup::Korean)
    } else if has("hant") || (first == "zh" && (has("tw") || has("hk") || has("mo"))) {
        Some(FontGroup::ChineseTraditional)
    } else if has("hans") || first == "zh" {
        Some(FontGroup::ChineseSimplified)
    } else {
        None
    }
}

/// A font file of `tables` (tag, bytes): the table directory, sorted by tag, then the tables, each
/// 4-byte aligned. `None` if there are too many or they are too large to address.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn sfnt(mut tables: Vec<([u8; 4], Vec<u8>)>) -> Option<Vec<u8>> {
    tables.sort_by_key(|t| t.0);
    let n = u16::try_from(tables.len()).ok()?;
    let pow = 1u16 << (15 - n.max(1).leading_zeros().min(15));
    let mut out = 0x0001_0000_u32.to_be_bytes().to_vec();
    for v in [n, pow.saturating_mul(16), pow.trailing_zeros() as u16, n.saturating_mul(16).saturating_sub(pow.saturating_mul(16))] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let mut offset = 12 + 16 * tables.len();
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(offset).ok()?.to_be_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).ok()?.to_be_bytes());
        offset = offset.checked_add(data.len().next_multiple_of(4))?;
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    Some(out)
}

/// Caps on what a (possibly damaged) font can make classification read.
const MAX_TAGS: usize = 64;
const MAX_RECORDS: usize = 512;

/// The first group in menu order among `found` (the tie order Japanese, Simplified Chinese,
/// Traditional Chinese, Korean).
fn first(found: impl Iterator<Item = FontGroup>) -> Option<FontGroup> {
    found.min()
}

/// The CJK group the font's `meta` table names: `dlng` (the languages it is designed for), else
/// `slng` (the ones it supports).
fn meta_group(f: &skrifa::FontRef<'_>) -> Option<FontGroup> {
    use skrifa::raw::tables::meta::{DLNG, Metadata, SLNG};
    let meta = f.meta().ok()?;
    let tags = |want| {
        let map = meta.data_maps().iter().find(|m| m.tag() == want)?;
        let Ok(Metadata::ScriptLangTags(tags)) = map.data(meta.offset_data()) else { return None };
        first(tags.iter().take(MAX_TAGS).filter_map(|t| group_of_tag(t.ok()?.as_str())))
    };
    tags(DLNG).or_else(|| tags(SLNG))
}

/// The CJK group of the languages the font's `name` table names its family in (style names
/// don't count: fonts translate those into many languages).
fn name_group(f: &skrifa::FontRef<'_>) -> Option<FontGroup> {
    let name = f.name().ok()?;
    first(
        name.name_record()
            .iter()
            .take(MAX_RECORDS)
            .filter(|r| FAMILY_IDS.contains(&r.name_id().to_u16()))
            .filter_map(|r| group_of_language(r.platform_id(), r.language_id())),
    )
}

/// Name IDs of the family name: typographic family, then family.
const FAMILY_IDS: [u16; 2] = [16, 1];

/// The CJK groups of the OS/2 code page ranges the font lists (JIS, Chinese Simplified, Korean
/// Wansung, Chinese Traditional, Korean Johab), in menu order: empty when it lists code pages
/// but no CJK one, `None` when it lists none at all.
fn code_page_groups(f: &skrifa::FontRef<'_>) -> Option<Vec<FontGroup>> {
    let os2 = f.os2().ok()?;
    let (r1, r2) = (os2.ul_code_page_range_1()?, os2.ul_code_page_range_2().unwrap_or(0));
    if r1 == 0 && r2 == 0 {
        return None;
    }
    let bits = [
        (17, FontGroup::Japanese),
        (18, FontGroup::ChineseSimplified),
        (19, FontGroup::Korean),
        (20, FontGroup::ChineseTraditional),
        (21, FontGroup::Korean),
    ];
    let mut groups: Vec<FontGroup> = bits.into_iter().filter(|(b, _)| r1 & (1 << b) != 0).map(|(_, g)| g).collect();
    groups.sort();
    groups.dedup();
    Some(groups)
}

/// The CJK group a family name's region tag names, as the pan-CJK families carry one per
/// language ("Noto Sans CJK SC", "Source Han Serif TC", "PingFang HK", "Hiragino Sans GB"): a
/// whole word of the name, in capitals.
fn family_tag_group(family: &str) -> Option<FontGroup> {
    family.split(|c: char| c.is_whitespace() || c == '-' || c == '_').find_map(|word| match word {
        "JP" => Some(FontGroup::Japanese),
        "SC" | "CN" | "GB" => Some(FontGroup::ChineseSimplified),
        "TC" | "HK" | "TW" | "MO" | "HC" => Some(FontGroup::ChineseTraditional),
        "KR" => Some(FontGroup::Korean),
        _ => None,
    })
}

/// The group the font declares: its `meta` languages, then its `name` table's languages, then its
/// code pages (the strongest signal first). A font that lists the code pages of several CJK
/// languages (one of a pan-CJK family, covering them all) is the one `family`'s region tag
/// names, else the first in menu order. `None` when it declares nothing at all: no CJK language
/// and no code pages.
pub(crate) fn declared_group(f: &skrifa::FontRef<'_>, family: &str) -> Option<FontGroup> {
    if let Some(g) = meta_group(f).or_else(|| name_group(f)) {
        return Some(g);
    }
    let pages = code_page_groups(f)?;
    let tagged = if pages.len() > 1 { family_tag_group(family).filter(|g| pages.contains(g)) } else { None };
    Some(tagged.or_else(|| pages.first().copied()).unwrap_or(FontGroup::Western))
}

/// The group by what the font covers: kana is Japanese, Hangul Korean, ideographs alone
/// Simplified Chinese.
pub(crate) fn covered_group(f: &skrifa::FontRef<'_>) -> FontGroup {
    use skrifa::MetadataProvider;
    let cmap = f.charmap();
    let any = |cs: &[char]| cs.iter().any(|c| cmap.map(*c).is_some());
    if any(&['あ', 'ア', 'の']) {
        FontGroup::Japanese
    } else if any(&['가', '한', '이']) {
        FontGroup::Korean
    } else if any(&['一', '中', '人']) {
        FontGroup::ChineseSimplified
    } else {
        FontGroup::Western
    }
}

/// The font's group and, for a CJK group, its family name in that language (typographic family,
/// else family) when it differs from `family`.
pub(crate) fn classify(f: &skrifa::FontRef<'_>, family: &str) -> (FontGroup, Option<String>) {
    let group = declared_group(f, family).unwrap_or_else(|| covered_group(f));
    (group, native_family(f, group).filter(|n| n != family))
}

/// The family name (ID 16, else 1) in `group`'s language, from records whose text decodes.
fn native_family(f: &skrifa::FontRef<'_>, group: FontGroup) -> Option<String> {
    if group == FontGroup::Western {
        return None;
    }
    let name = f.name().ok()?;
    let records = name.name_record();
    FAMILY_IDS.into_iter().find_map(|id| {
        records.iter().take(MAX_RECORDS).find_map(|r| {
            if r.name_id().to_u16() != id || !r.is_unicode() || group_of_language(r.platform_id(), r.language_id()) != Some(group) {
                return None;
            }
            let s: String = r.string(name.string_data()).ok()?.chars().take(256).collect();
            let s = s.trim().to_string();
            (!s.is_empty()).then_some(s)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{font_with, with_code_pages, with_meta, with_names, with_table_len};

    const FAMILY: &str = "DC Test Group";

    fn group_of(font: &[u8]) -> (FontGroup, Option<String>) {
        classify(&skrifa::FontRef::new(font).unwrap(), FAMILY)
    }

    /// A font with only Latin letters, its code pages `range1` (and none in range 2).
    fn latin(range1: u32) -> Vec<u8> {
        with_code_pages(font_with(FAMILY, &['a', 'b']).unwrap(), range1, 0).unwrap()
    }

    /// Windows English family records plus `extra` records.
    fn named(font: &[u8], extra: &[(u16, u16, u16, u16, &str)]) -> Vec<u8> {
        let mut records = vec![(3, 1, 0x409, 1, FAMILY), (3, 1, 0x409, 2, "Regular")];
        records.extend_from_slice(extra);
        with_names(font, &records).unwrap()
    }

    #[test]
    fn code_pages_give_the_group() {
        assert_eq!(group_of(&latin(1)).0, FontGroup::Western, "Latin 1 only");
        for (bit, group) in [
            (17, FontGroup::Japanese),
            (18, FontGroup::ChineseSimplified),
            (19, FontGroup::Korean),
            (20, FontGroup::ChineseTraditional),
            (21, FontGroup::Korean),
        ] {
            assert_eq!(group_of(&latin(1 | 1 << bit)).0, group, "bit {bit}");
        }
        // Several: Japanese, Simplified Chinese, Traditional Chinese, Korean.
        assert_eq!(group_of(&latin(1 << 17 | 1 << 19)).0, FontGroup::Japanese);
        assert_eq!(group_of(&latin(1 << 20 | 1 << 21)).0, FontGroup::ChineseTraditional);
    }

    /// A pan-CJK family's fonts list the code pages of every language they cover and name their
    /// language only in the family name ("… CJK SC"): that tag decides, not the menu order.
    #[test]
    fn a_region_tag_in_the_family_name_decides_between_several_code_pages() {
        let pan = 1 << 17 | 1 << 18 | 1 << 19 | 1 << 20 | 1 << 21;
        let group = |family: &str, range1: u32| {
            let font = with_code_pages(font_with(family, &['a', 'b']).unwrap(), range1, 0).unwrap();
            classify(&skrifa::FontRef::new(&font).unwrap(), family).0
        };
        for (family, expected) in [
            ("DC Sans CJK SC R", FontGroup::ChineseSimplified),
            ("DC Sans CJK SC", FontGroup::ChineseSimplified),
            ("DC Sans GB", FontGroup::ChineseSimplified),
            ("DC Serif CJK TC", FontGroup::ChineseTraditional),
            ("DC Sans HK", FontGroup::ChineseTraditional),
            ("DC Sans CJK KR", FontGroup::Korean),
            ("DC Sans CJK JP", FontGroup::Japanese),
            ("DC-Sans-SC", FontGroup::ChineseSimplified),
            // No tag, or not a whole word in capitals: the first in menu order, as before.
            ("DC Sans", FontGroup::Japanese),
            ("DC Script", FontGroup::Japanese),
            ("DC Sans Sc", FontGroup::Japanese),
        ] {
            assert_eq!(group(family, pan), expected, "{family}");
        }
        // The tag names a language the font doesn't list: its code pages decide.
        assert_eq!(group("DC Sans SC", 1 << 17 | 1 << 19), FontGroup::Japanese);
        // One CJK code page is unambiguous, whatever the name says.
        assert_eq!(group("DC Sans SC", 1 << 17), FontGroup::Japanese);
        // No CJK code page: the tag alone doesn't make a font CJK.
        assert_eq!(group("DC Sans SC", 1), FontGroup::Western);
    }

    #[test]
    fn name_languages_give_the_group_and_the_native_name() {
        let jp = named(&latin(1), &[(3, 1, 0x0411, 1, "テスト明朝"), (3, 1, 0x0411, 16, "テスト明朝 Pro")]);
        assert_eq!(group_of(&jp), (FontGroup::Japanese, Some("テスト明朝 Pro".into())), "the typographic family first");
        for (lang, group) in [
            (0x0804, FontGroup::ChineseSimplified),
            (0x1004, FontGroup::ChineseSimplified),
            (0x0404, FontGroup::ChineseTraditional),
            (0x0C04, FontGroup::ChineseTraditional),
            (0x1404, FontGroup::ChineseTraditional),
            (0x0412, FontGroup::Korean),
        ] {
            assert_eq!(group_of(&named(&latin(1), &[(3, 1, lang, 1, "本地名")])), (group, Some("本地名".into())), "{lang:#x}");
        }
        // Macintosh records in a CJK encoding: the language counts, the text doesn't decode.
        assert_eq!(group_of(&named(&latin(1), &[(1, 1, 11, 1, "x")])), (FontGroup::Japanese, None));
        assert_eq!(group_of(&named(&latin(1), &[(1, 3, 23, 1, "x")])).0, FontGroup::Korean);
        // Records in two CJK languages: the menu order decides.
        assert_eq!(group_of(&named(&latin(1), &[(3, 1, 0x0412, 1, "한"), (3, 1, 0x0404, 1, "繁")])).0, FontGroup::ChineseTraditional);
        // Translated style names don't make a Western font CJK.
        assert_eq!(group_of(&named(&latin(1), &[(3, 1, 0x0411, 2, "標準"), (1, 1, 11, 2, "x")])).0, FontGroup::Western);
        // A native name equal to the English one isn't shown twice.
        assert_eq!(group_of(&named(&latin(1), &[(3, 1, 0x0411, 1, FAMILY)])), (FontGroup::Japanese, None));
    }

    #[test]
    fn meta_beats_names_which_beat_code_pages() {
        let all = named(&latin(1 << 17), &[(3, 1, 0x0412, 1, "한글")]);
        assert_eq!(group_of(&all).0, FontGroup::Korean, "the name table over the code pages");
        assert_eq!(group_of(&with_meta(&all, Some("zh-Hant"), None).unwrap()).0, FontGroup::ChineseTraditional, "dlng first");
        assert_eq!(group_of(&with_meta(&all, None, Some("Hans")).unwrap()).0, FontGroup::ChineseSimplified, "slng without dlng");
        assert_eq!(group_of(&with_meta(&all, Some("Kore, Jpan"), Some("Hans")).unwrap()).0, FontGroup::Japanese, "ties in menu order");
        // A meta table naming no CJK language leaves it to the rest.
        assert_eq!(group_of(&with_meta(&all, Some("Latn"), None).unwrap()).0, FontGroup::Korean);
        for (tag, group) in [
            ("ja", FontGroup::Japanese),
            ("ja-Jpan", FontGroup::Japanese),
            ("zh", FontGroup::ChineseSimplified),
            ("zh-Hans", FontGroup::ChineseSimplified),
            ("zh-TW", FontGroup::ChineseTraditional),
            ("zh-HK", FontGroup::ChineseTraditional),
            ("ko", FontGroup::Korean),
            ("Hang", FontGroup::Korean),
        ] {
            assert_eq!(group_of_tag(tag), Some(group), "{tag}");
        }
        assert_eq!(group_of_tag("en-Latn"), None);
    }

    #[test]
    fn coverage_decides_when_the_font_declares_nothing() {
        for (chars, group) in [
            (&['a', 'あ', '一'][..], FontGroup::Japanese),
            (&['a', '한', '一'][..], FontGroup::Korean),
            (&['a', '一'][..], FontGroup::ChineseSimplified),
            (&['a'][..], FontGroup::Western),
        ] {
            let font = with_code_pages(font_with(FAMILY, chars).unwrap(), 0, 0).unwrap();
            assert_eq!(group_of(&font).0, group, "{chars:?}");
        }
        // Code pages without a CJK one: Western, whatever the font covers.
        assert_eq!(group_of(&with_code_pages(font_with(FAMILY, &['一']).unwrap(), 1, 0).unwrap()).0, FontGroup::Western);
    }

    #[test]
    fn damaged_tables_classify_without_panicking() {
        let base = with_meta(&named(&latin(1 << 18), &[(3, 1, 0x0411, 1, "名前")]), Some("Jpan"), None).unwrap();
        for tag in [b"name", b"OS/2", b"meta", b"cmap"] {
            for len in [0, 3, 7, 13, 21, 80] {
                let font = with_table_len(base.clone(), tag, len).unwrap();
                if let Ok(f) = skrifa::FontRef::new(&font) {
                    let _ = classify(&f, FAMILY);
                }
            }
        }
    }

    #[test]
    fn menus_list_western_fonts_then_each_cjk_language() {
        let info = |family: &str, group, native: Option<&str>| FamilyInfo { family: family.into(), group, native: native.map(Into::into) };
        let mut fams = vec![
            info("Yu Mincho", FontGroup::Japanese, Some("游明朝")),
            info("Apple SD Gothic Neo", FontGroup::Korean, Some("애플 SD 산돌고딕 Neo")),
            info("Songti SC", FontGroup::ChineseSimplified, Some("宋体-简")),
            info("zapfino", FontGroup::Western, None),
            info("Hiragino Mincho ProN", FontGroup::Japanese, Some("ヒラギノ明朝 ProN")),
            info("PMingLiU", FontGroup::ChineseTraditional, Some("新細明體")),
            info("Arial", FontGroup::Western, None),
        ];
        sort_for_menu(&mut fams, true);
        let order: Vec<&str> = fams.iter().map(|f| f.label(true)).collect();
        assert_eq!(order, ["Arial", "zapfino", "Hiragino Mincho ProN", "Yu Mincho", "Songti SC", "PMingLiU", "Apple SD Gothic Neo"]);
        sort_for_menu(&mut fams, false);
        let order: Vec<&str> = fams.iter().map(|f| f.label(false)).collect();
        assert_eq!(order, ["Arial", "zapfino", "ヒラギノ明朝 ProN", "游明朝", "宋体-简", "新細明體", "애플 SD 산돌고딕 Neo"]);
        // Search finds either name.
        let yu = fams.iter().find(|f| f.family == "Yu Mincho").unwrap();
        assert!(yu.matches("yu min") && yu.matches("游") && yu.matches("") && !yu.matches("songti"));
    }
}
