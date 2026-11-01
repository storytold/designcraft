/// Does `c` belong to a script that capitalizes the first letter of sentences, words and headings?
/// Georgian doesn't: Unicode maps Mkhedruli to Mtavruli capitals, but those are only for all-caps
/// text, never for a sentence's, a title's or an index heading's first letter.
pub fn capitalizes(c: char) -> bool {
    !matches!(c, '\u{10A0}'..='\u{10FF}' | '\u{1C90}'..='\u{1CBF}' | '\u{2D00}'..='\u{2D2F}')
}

/// The BCP 47 tag of a language name as InDesign and IDML write it ("English: USA", "Chinese:
/// Simplified", or a locale code such as "de_DE_2006"), for the shaper's localized forms (`locl`)
/// and everything that depends on the language (quotes, hyphenation and spelling, line breaking,
/// fallback fonts). A variant we don't know falls back to its language ("English: Australian" →
/// `en`); an unknown language (or "[No Language]") has none. Case, punctuation and IDML's `$ID/`
/// don't matter. Locale codes (`ll`, `ll_CC`, `ll-CC`, with a reform year after them as in
/// `de_DE_2006`) are their tag (`de-DE`), and Chinese also goes by its other common spellings
/// ("Simplified Chinese", "Chinese (Traditional)", "zh_CN", "zh-Hant").
pub fn language_tag(language: &str) -> Option<&'static str> {
    let language = language.trim();
    let language = language.strip_prefix("$ID/").unwrap_or(language);
    let l = language.to_lowercase();
    let words: Vec<&str> = l.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let first = words.first().copied().unwrap_or("");
    let has = |any: &[&str]| words.iter().any(|w| any.contains(w));
    if first == "zh" || has(&["chinese"]) {
        // The script decides; then the name; then the region.
        return Some(if has(&["hans"]) {
            "zh-Hans"
        } else if has(&["hant"]) {
            "zh-Hant"
        } else if has(&["simplified", "cn", "sg"]) {
            "zh-Hans"
        } else if has(&["traditional", "tw", "hk", "mo"]) {
            "zh-Hant"
        } else {
            "zh"
        });
    }
    if let Some(tag) = locale_code(&words) {
        return intern_tag(tag);
    }
    let variant = match words.join(" ").as_str() {
        "english usa" => Some("en-US"),
        "english uk" => Some("en-GB"),
        "english canadian" => Some("en-CA"),
        "german swiss 2006 reform" | "german swiss" => Some("de-CH"),
        "french swiss" => Some("fr-CH"),
        "italian swiss" => Some("it-CH"),
        "spanish castilian" => Some("es-ES"),
        "french canadian" => Some("fr-CA"),
        "portuguese brazilian" => Some("pt-BR"),
        "norwegian bokmål" | "norwegian bokmal" => Some("nb"),
        "norwegian nynorsk" => Some("nn"),
        _ => None,
    };
    if variant.is_some() {
        return variant;
    }
    Some(match first {
        "english" => "en",
        "german" => "de",
        "french" => "fr",
        "spanish" => "es",
        "catalan" => "ca",
        "italian" => "it",
        "portuguese" => "pt",
        "dutch" => "nl",
        "danish" => "da",
        "swedish" => "sv",
        "norwegian" => "no",
        "finnish" => "fi",
        "polish" => "pl",
        "czech" => "cs",
        "slovak" => "sk",
        "hungarian" => "hu",
        "romanian" => "ro",
        "croatian" => "hr",
        "lithuanian" => "lt",
        "greek" => "el",
        "russian" => "ru",
        "ukrainian" => "uk",
        "bulgarian" => "bg",
        "serbian" => "sr",
        "macedonian" => "mk",
        "turkish" => "tr",
        "japanese" => "ja",
        "korean" => "ko",
        "arabic" => "ar",
        "persian" | "farsi" => "fa",
        "urdu" => "ur",
        "hebrew" => "he",
        "georgian" => "ka",
        _ => return None,
    })
}

/// The tag of a locale code split into lowercase `words`: a language (2 or 3 letters), then
/// optionally a region (2 letters or 3 digits) and a reform year (4 digits) → `ll` or `ll-CC`.
fn locale_code(words: &[&str]) -> Option<String> {
    let letters = |w: &str, n: std::ops::RangeInclusive<usize>| n.contains(&w.len()) && w.bytes().all(|b| b.is_ascii_lowercase());
    let digits = |w: &str, n: usize| w.len() == n && w.bytes().all(|b| b.is_ascii_digit());
    match *words {
        [lang] if letters(lang, 2..=3) => Some(lang.to_string()),
        [lang, region] | [lang, region, _] if letters(lang, 2..=3) && (letters(region, 2..=2) || digits(region, 3)) => {
            if let [_, _, year] = *words
                && !digits(year, 4)
            {
                return None;
            }
            Some(format!("{lang}-{}", region.to_ascii_uppercase()))
        }
        _ => None,
    }
}

/// `tag` as a `&'static str`: each distinct tag is kept once for the process. Language names come
/// from documents, so at most [`MAX_TAGS`] are kept; past that a new tag is unknown (`None`).
fn intern_tag(tag: String) -> Option<&'static str> {
    const MAX_TAGS: usize = 512;
    static TAGS: std::sync::Mutex<Vec<&str>> = std::sync::Mutex::new(Vec::new());
    let mut tags = TAGS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(t) = tags.iter().find(|t| **t == tag) {
        return Some(t);
    }
    if tags.len() >= MAX_TAGS {
        return None;
    }
    let t: &'static str = Box::leak(tag.into_boxed_str());
    tags.push(t);
    Some(t)
}

/// The language subtag of a BCP 47 tag (`de-DE` → `de`, `zh-Hant` → `zh`).
pub fn language_subtag(tag: &str) -> &str {
    tag.split('-').next().unwrap_or(tag)
}
