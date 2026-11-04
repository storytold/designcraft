//! Hungarian proofing is owned by craft-proof; dictionaries are supplied by the hu pack.
use std::sync::OnceLock;
#[cfg(test)]
std::thread_local! { static INITIALIZATION_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(super) fn initialization_calls() -> usize {
    INITIALIZATION_CALLS.with(std::cell::Cell::get)
}
fn lang() -> craft_proof::ProofLang {
    craft_proof::ProofLang::from_tag(Some("hu"))
}
pub(super) fn initialize() -> Result<(), &'static str> {
    #[cfg(test)]
    INITIALIZATION_CALLS.with(|c| c.set(c.get() + 1));
    if craft_proof::has_dictionary(Some("hu")) {
        return Ok(());
    }
    static ERROR: OnceLock<String> = OnceLock::new();
    Err(ERROR
        .get_or_init(|| craft_proof::dictionary_load_error(lang()).unwrap_or_else(|| "The Hungarian (hu) Language pack is not installed.".into()))
        .as_str())
}
pub(super) fn known(word: &str) -> bool {
    let opts = craft_proof::Options { ignore_uppercase: false, ignore_numbers: false, ignore_internet: false };
    craft_proof::is_correct_in_with(word, lang(), &opts)
}
pub(super) fn suggest(word: &str) -> Vec<String> {
    craft_proof::suggest_in(word, 8, lang())
}
