//! MPL-2.0 Magyar Ispell 1.9 data; see assets/dictionaries/hu_HU/.
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

static DICTIONARY: OnceLock<std::result::Result<spellbook::Dictionary, String>> = OnceLock::new();

// Thread-local test instrumentation observes requests even after OnceLock is warm.
#[cfg(test)]
std::thread_local! {
    static INITIALIZATION_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn initialization_calls() -> usize {
    INITIALIZATION_CALLS.with(std::cell::Cell::get)
}

pub(super) fn initialize() -> std::result::Result<&'static spellbook::Dictionary, &'static str> {
    #[cfg(test)]
    INITIALIZATION_CALLS.with(|calls| calls.set(calls.get() + 1));
    DICTIONARY
        .get_or_init(|| {
            spellbook::Dictionary::new(
                include_str!("../../../../../assets/dictionaries/hu_HU/hu_HU.aff"),
                include_str!("../../../../../assets/dictionaries/hu_HU/hu_HU.dic"),
            )
            .map_err(|e| format!("Hungarian dictionary: {e:?}"))
        })
        .as_ref()
        .map_err(String::as_str)
}

pub(super) fn known(word: &str) -> bool {
    // The engine preserves case: proper names and KEEPCASE entries matter in Hungarian.
    initialize().is_ok_and(|d| d.check(&word.nfc().collect::<String>()))
}

pub(super) fn suggest(word: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(dict) = initialize() {
        dict.suggest(&word.nfc().collect::<String>(), &mut out);
    }
    out.truncate(8);
    out
}
