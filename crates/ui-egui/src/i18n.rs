//! Interface language (Preferences › General › Interface Language): translations of menu titles,
//! the most used menu items and panel names. Untranslated strings stay English. The translations
//! are our own.

use std::{collections::HashMap, sync::OnceLock};

mod table;

use table::TABLE;

mod ar;
mod cs;
mod de;
mod fr;
mod it;
mod ja;
mod ka;
mod pl;
mod pt_br;
mod ru;
mod uk;

/// Supported interface languages: (code, name in that language).
pub const LANGUAGES: &[(&str, &str)] = &[
    ("", "English"),
    ("de", "Deutsch"),
    ("fr", "Français"),
    ("es", "Español"),
    ("ja", "日本語"),
    ("zh", "简体中文"),
    ("ar", "العربية"),
    ("pt-br", "Português (Brasil)"),
    ("it", "Italiano"),
    ("uk", "Українська"),
    ("ru", "Русский"),
    ("cs", "Čeština"),
    ("pl", "Polski"),
    ("ka", "ქართული"),
];

/// The interface languages in the order the Preferences picker lists them: English first (the
/// source language), then the rest by their own name with accents folded, so Čeština sits under
/// C. Names in other scripts follow the Latin ones in code point order: Cyrillic, Arabic,
/// Georgian, then the CJK languages.
pub fn picker_languages() -> Vec<(&'static str, &'static str)> {
    let mut rest: Vec<(&str, &str)> = LANGUAGES.iter().copied().filter(|(code, _)| !code.is_empty()).collect();
    rest.sort_by_cached_key(|(_, name)| name_sort_key(name));
    std::iter::once(LANGUAGES[0]).chain(rest).collect()
}

/// Interface direction; independent of document binding and paragraph direction.
pub fn is_rtl(lang: &str) -> bool {
    lang == "ar"
}

/// A Japanese, Chinese or Korean interface language (`zh-tw` included): CJK features are shown by
/// default (see [`crate::cjk_features`]).
pub fn is_cjk(lang: &str) -> bool {
    matches!(lang.split('-').next(), Some("ja" | "zh" | "ko"))
}

fn column(lang: &str) -> Option<usize> {
    match lang {
        "de" => Some(0),
        "fr" => Some(1),
        "es" => Some(2),
        "ja" => Some(3),
        "zh" => Some(4),
        _ => None,
    }
}

/// `s` in `lang` (English, or the string itself, when there's no translation).
pub fn tr<'a>(lang: &str, s: &'a str) -> &'a str {
    let text = lookup(lang, s);
    // An empty row is a translation nobody has written yet.
    if text.trim().is_empty() { s } else { text }
}

fn lookup<'a>(lang: &str, s: &'a str) -> &'a str {
    static GERMAN: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    static JAPANESE: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    static TRANSLATIONS: OnceLock<HashMap<&str, [&str; 5]>> = OnceLock::new();
    static ARABIC: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    static UKRAINIAN: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    static PORTUGUESE_BR: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    static ITALIAN: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    static POLISH: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    static RUSSIAN: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    static CZECH: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    static FRENCH: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    static GEORGIAN: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    if lang == "ar" {
        return ARABIC.get_or_init(|| ar::TABLE.iter().copied().collect()).get(s).copied().unwrap_or(s);
    }
    // The German and Japanese tables cover the whole interface; the shared table's German and Japanese
    // columns are the fallback for keys added there after them.
    if lang == "de"
        && let Some(text) = GERMAN.get_or_init(|| de::TABLE.iter().copied().collect()).get(s).copied().filter(|text| !text.trim().is_empty())
    {
        return text;
    }
    if lang == "ja"
        && let Some(text) = JAPANESE.get_or_init(|| ja::TABLE.iter().copied().collect()).get(s).copied().filter(|text| !text.trim().is_empty())
    {
        return text;
    }
    // `fr::TABLE` holds the strings the shared table has no row for.
    if lang == "fr"
        && let Some(text) = FRENCH.get_or_init(|| fr::TABLE.iter().copied().collect()).get(s).copied().filter(|text| !text.trim().is_empty())
    {
        return text;
    }
    if lang == "pt-br" {
        return PORTUGUESE_BR.get_or_init(|| pt_br::TABLE.iter().copied().collect()).get(s).copied().unwrap_or(s);
    }
    if lang == "it" {
        return ITALIAN.get_or_init(|| it::TABLE.iter().copied().collect()).get(s).copied().unwrap_or(s);
    }
    if lang == "pl" {
        return POLISH.get_or_init(|| pl::TABLE.iter().copied().collect()).get(s).copied().unwrap_or(s);
    }
    if lang == "ka" {
        return GEORGIAN.get_or_init(|| ka::TABLE.iter().copied().collect()).get(s).copied().unwrap_or(s);
    }
    if lang == "uk" {
        return UKRAINIAN.get_or_init(|| uk::TABLE.iter().copied().collect()).get(s).copied().unwrap_or(s);
    }
    if lang == "ru" {
        return RUSSIAN.get_or_init(|| ru::TABLE.iter().copied().collect()).get(s).copied().unwrap_or(s);
    }
    if lang == "cs" {
        return CZECH.get_or_init(|| cs::TABLE.iter().copied().collect()).get(s).copied().unwrap_or(s);
    }
    let Some(c) = column(lang) else { return s };
    TRANSLATIONS.get_or_init(|| TABLE.iter().copied().collect()).get(s).and_then(|row| row.get(c)).copied().unwrap_or(s)
}

/// Context-sensitive captions in the shared table's languages (columns as in [`TABLE`]).
const CONTEXT: &[(&str, &str, [&str; 5])] = &[
    // A count of columns or rows, unlike the numbering field's "Number:".
    ("Number:", "count", ["Anzahl:", "Nombre :", "Número:", "数:", "数量："]),
];

/// Context-sensitive UI captions; unsupported contexts keep the shared translation.
pub fn tr_context<'a>(lang: &str, s: &'a str, context: &str) -> &'a str {
    let contexts = match lang {
        "uk" => uk::CONTEXT,
        "ru" => ru::CONTEXT,
        "cs" => cs::CONTEXT,
        "pl" => pl::CONTEXT,
        "ja" => ja::CONTEXT,
        "ar" => ar::CONTEXT,
        _ => &[],
    };
    let written = |text: &&str| !text.trim().is_empty();
    if let Some(text) = contexts.iter().find(|(key, scope, _)| *key == s && *scope == context).map(|(_, _, text)| *text).filter(written) {
        return text;
    }
    if let Some(c) = column(lang)
        && let Some(text) =
            CONTEXT.iter().find(|(key, scope, _)| *key == s && *scope == context).and_then(|(_, _, row)| row.get(c)).copied().filter(written)
    {
        return text;
    }
    tr(lang, s)
}

#[cfg(test)]
pub(crate) fn ukrainian_catalog() -> Vec<&'static str> {
    uk::TABLE.iter().map(|(_, text)| *text).chain(uk::CONTEXT.iter().map(|(_, _, text)| *text)).collect()
}

#[cfg(test)]
pub(crate) fn russian_catalog() -> Vec<&'static str> {
    ru::TABLE.iter().map(|(_, text)| *text).chain(ru::CONTEXT.iter().map(|(_, _, text)| *text)).collect()
}

/// Ukrainian, Russian, Polish and Georgian put the category before the value in count captions
/// ("сторінок: 5", "страниц: 5", "Strony: 5", "გვერდი: 5"): every number stays grammatical in the
/// languages with several plural forms, and Georgian nouns stay singular after numerals. Other
/// languages keep the existing value-first caption.
pub fn category_first(lang: &str) -> bool {
    matches!(lang, "uk" | "ru" | "pl" | "ka")
}

/// Caption for a counted category, e.g. `count_label("ru", "pages", 5)` is "страниц: 5". Czech
/// picks the plural form for the count (1, 2–4, other).
pub fn count_label(lang: &str, noun: &str, count: usize) -> String {
    if category_first(lang) {
        return format!("{}: {count}", tr(lang, noun));
    }
    if lang == "cs"
        && let Some((_, [one, few, other])) = cs::PLURALS.iter().find(|(key, _)| *key == noun)
    {
        let form = match count {
            1 => one,
            2..=4 => few,
            _ => other,
        };
        if !form.trim().is_empty() {
            return format!("{count} {form}");
        }
    }
    format!("{count} {}", tr(lang, noun))
}

/// Localize reserved built-in style names only; user-defined names are document data.
pub fn style_name<'a>(lang: &str, name: &'a str) -> &'a str {
    match name {
        designcraft_doc::NO_PARA_STYLE | designcraft_doc::BASIC_PARAGRAPH | designcraft_doc::NO_CHAR_STYLE => tr(lang, name),
        _ => name,
    }
}

/// Built-in workspace captions; custom workspace names remain unchanged.
pub fn workspace_name<'a>(lang: &str, name: &'a str) -> &'a str {
    match name {
        "Essentials"
        | "Essentials Classic"
        | "Advanced"
        | "Book"
        | "Digital Publishing"
        | "Interactive for PDF"
        | "Printing and Proofing"
        | "Typography" => tr(lang, name),
        _ => name,
    }
}

#[cfg(test)]
mod tests;

/// `name` lowercased with the Latin accents that occur in language names folded away, so a
/// byte-wise sort reads alphabetically. Other scripts are left as they are.
fn name_sort_key(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è'..='ë' | 'ē' | 'ě' | 'ę' | 'ế' | 'ệ' => 'e',
            'ì'..='ï' | 'ī' => 'i',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò'..='ö' | 'ø' | 'ő' => 'o',
            'ś' | 'š' => 's',
            'ù'..='ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod picker_order_tests {
    use super::*;

    #[test]
    fn picker_lists_english_first_then_names_alphabetically() {
        let order = picker_languages();
        assert_eq!(order.first(), Some(&("", "English")));
        assert_eq!(order.len(), LANGUAGES.len());
        let keys: Vec<String> = order[1..].iter().map(|(_, name)| name_sort_key(name)).collect();
        assert!(keys.windows(2).all(|w| w[0] <= w[1]), "{keys:?}");
        let pos = |code: &str| order.iter().position(|(c, _)| *c == code).unwrap_or(usize::MAX);
        assert!(pos("cs") < pos("de"), "Čeština sorts under C");
        assert!(pos("pl") < pos("ru"), "Latin names come before Cyrillic");
        assert!(pos("ru") < pos("ja"), "Cyrillic names come before CJK");
    }
}
