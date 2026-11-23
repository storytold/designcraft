//! The craft UI framework's `Host` for DesignCraft, and the words its components read in the app's language.
//!
//! The Keyboard Shortcuts dialog (`craft_menu::shortcuts`) asks the scope for its words and the `Host` for command labels.
//! DesignCraft's translations are `i18n::tr` tables keyed by English text, so this module builds both from them: a `Host`
//! whose `Commands` translate a command's label, and a `Words` whose lookup translates every word of a component by its
//! English text. Both are built once per language, because a new `Host` or `Words` is a new generation to the framework,
//! which then builds every view that reads them again.

use std::sync::{Arc, Mutex, PoisonError};

use craft_host::{AppInfo, Commands, Host};
use craft_ui_widgets::runtime::env::Words;

/// The labels of the app's commands in one language; a label with no translation stays English.
struct DcCommands {
    lang: String,
}

impl Commands for DcCommands {
    fn label(&self, _id: &str, english: &str) -> String {
        crate::i18n::tr(&self.lang, english).to_owned()
    }
}

/// The host for the interface language `lang` (`UiState::language`): the same `Arc` every frame, so the framework does not
/// build its views again.
pub(crate) fn host(lang: &str) -> Arc<Host> {
    static HOSTS: Mutex<Vec<(String, Arc<Host>)>> = Mutex::new(Vec::new());
    let mut hosts = HOSTS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, host)) = hosts.iter().find(|(code, _)| code == lang) {
        return host.clone();
    }
    let info = AppInfo { id: "designcraft", name: "DesignCraft", version: env!("CARGO_PKG_VERSION") };
    let host = Arc::new(Host::builder(info).commands(Arc::new(DcCommands { lang: lang.to_owned() })).build());
    hosts.push((lang.to_owned(), host.clone()));
    host
}

/// The words of a framework component in `lang`: each word reads as the app's translation of its English text, and stays
/// English where the tables have none. A count reads the singular or the plural form of its English texts, with `{n}` filled.
pub(crate) fn words(lang: &str) -> Arc<Words> {
    static WORDS: Mutex<Vec<(String, Arc<Words>)>> = Mutex::new(Vec::new());
    let mut cache = WORDS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, words)) = cache.iter().find(|(code, _)| code == lang) {
        return words.clone();
    }
    let (single, plural) = (lang.to_owned(), lang.to_owned());
    let words = Words::new(move |word| {
        let text = crate::i18n::tr(&single, word.english);
        (text != word.english).then(|| text.to_owned())
    })
    .with_plural(move |_id, n, one, other| {
        let form = if n == 1 { one } else { other };
        let text = crate::i18n::tr(&plural, form);
        (text != form).then(|| text.replace("{n}", &n.to_string()))
    });
    let words = Arc::new(words);
    cache.push((lang.to_owned(), words.clone()));
    words
}
