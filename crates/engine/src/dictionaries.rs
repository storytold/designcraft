//! Hunspell spelling dictionaries installed by the user: `<locale>.aff` + `<locale>.dic` pairs
//! (`sr_RS`, `sh_RS`, `de_DE`, as LibreOffice and InDesign's Hunspell folder name them) in a
//! folder or its subfolders, registered at start-up and read on first use (a large dictionary
//! takes a few hundred milliseconds). Text in a language with an installed dictionary is
//! spell-checked with it, through [`spellbook`] (see `cmd::spelling`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, PoisonError, RwLock};

/// Dictionary files larger than this are not read (the largest LibreOffice ones are ~20 MB).
const MAX_FILE_BYTES: u64 = 64 << 20;
/// At most this many dictionaries are registered.
const MAX_DICTIONARIES: usize = 32;
/// Letters looked at to tell a dictionary's script.
const SCRIPT_SAMPLE: usize = 4096;

/// One installed dictionary.
pub struct Installed {
    pub locale: String,
    language: &'static str,
    aff: PathBuf,
    dic: PathBuf,
    loaded: OnceLock<Option<Loaded>>,
}

struct Loaded {
    dict: spellbook::Dictionary,
    cyrillic: bool,
}

impl Installed {
    /// The dictionary, read on first use; None when it can't be read (logged once).
    fn loaded(&self) -> Option<&Loaded> {
        self.loaded
            .get_or_init(|| {
                let read = |p: &Path| -> Result<String, String> {
                    let len = std::fs::metadata(p).map_err(|e| e.to_string())?.len();
                    if len > MAX_FILE_BYTES {
                        return Err(format!("larger than {} MB", MAX_FILE_BYTES >> 20));
                    }
                    String::from_utf8(std::fs::read(p).map_err(|e| e.to_string())?).map_err(|_| "not UTF-8".to_string())
                };
                let loaded = read(&self.aff).and_then(|aff| {
                    let dic = read(&self.dic)?;
                    let dict = spellbook::Dictionary::new(&aff, &dic).map_err(|e| e.to_string())?;
                    Ok(Loaded { dict, cyrillic: mostly_cyrillic(&dic) })
                });
                match loaded {
                    Ok(l) => {
                        log::info!("spelling: {} dictionary read", self.locale);
                        Some(l)
                    }
                    Err(e) => {
                        log::warn!("spelling: {} dictionary skipped: {e}", self.locale);
                        None
                    }
                }
            })
            .as_ref()
    }
}

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{052F}')
}

/// Whether most of the first letters of a word list are Cyrillic.
fn mostly_cyrillic(dic: &str) -> bool {
    let (mut cyr, mut all) = (0usize, 0usize);
    for c in dic.chars().filter(|c| c.is_alphabetic()).take(SCRIPT_SAMPLE) {
        all += 1;
        cyr += usize::from(is_cyrillic(c));
    }
    cyr * 2 > all
}

static INSTALLED: RwLock<Vec<Arc<Installed>>> = RwLock::new(Vec::new());

/// The language dictionaries are kept under: `sh` (Serbo-Croatian, the code Hunspell's Serbian
/// Latin dictionaries use) is Serbian, as for hyphenation.
fn dictionary_language(language: &str) -> Option<&'static str> {
    let sub = designcraft_doc::language_subtag(designcraft_doc::language_tag(language)?);
    Some(if sub == "sh" { "sr" } else { sub })
}

/// Register the `<locale>.aff` + `<locale>.dic` pairs in `dir` and its subfolders (in name
/// order; `hyph_*.dic` hyphenation files are not dictionaries). A missing folder registers
/// nothing. Returns one line per dictionary for the log.
pub fn install_dir(dir: &Path) -> Vec<String> {
    let files = crate::hyphenation::dictionary_files(dir);
    let mut report = Vec::new();
    for (name, aff) in &files {
        let Some(locale) = name.strip_suffix(".aff").filter(|l| !l.is_empty()) else { continue };
        let dic = aff.with_extension("dic");
        if !dic.is_file() {
            report.push(format!("spelling: {name} skipped: no {locale}.dic beside it"));
            continue;
        }
        report.push(match install(locale, aff.clone(), dic) {
            Ok(()) => format!("spelling: {locale} dictionary installed"),
            Err(e) => format!("spelling: {name} skipped: {e}"),
        });
    }
    report
}

/// Register the dictionary `aff` + `dic` for `locale` (read on first use).
pub fn install(locale: &str, aff: PathBuf, dic: PathBuf) -> Result<(), String> {
    let language = dictionary_language(locale).ok_or_else(|| format!("{locale}: not a language"))?;
    let mut installed = INSTALLED.write().unwrap_or_else(PoisonError::into_inner);
    if installed.len() >= MAX_DICTIONARIES {
        return Err(format!("{locale}: too many dictionaries installed"));
    }
    installed.push(Arc::new(Installed { locale: locale.to_string(), language, aff, dic, loaded: OnceLock::new() }));
    Ok(())
}

/// The dictionaries installed for a language name or locale code; empty when there are none.
pub fn for_language(language: &str) -> Vec<Arc<Installed>> {
    let Some(language) = dictionary_language(language) else { return Vec::new() };
    INSTALLED.read().unwrap_or_else(PoisonError::into_inner).iter().filter(|d| d.language == language).cloned().collect()
}

/// The readable dictionaries of `dicts` in the script of `word` (all of them when none is).
fn for_word<'a>(dicts: &'a [Arc<Installed>], word: &str) -> Vec<&'a Loaded> {
    let loaded: Vec<&Loaded> = dicts.iter().filter_map(|d| d.loaded()).collect();
    let cyrillic = word.chars().find(|c| c.is_alphabetic()).is_some_and(is_cyrillic);
    let same: Vec<&Loaded> = loaded.iter().copied().filter(|l| l.cyrillic == cyrillic).collect();
    if same.is_empty() { loaded } else { same }
}

/// Is `word` (as written: dictionaries know which words are capitalized) spelled correctly by
/// one of `dicts`? None when none of them can be read.
pub fn check(dicts: &[Arc<Installed>], word: &str) -> Option<bool> {
    let loaded = for_word(dicts, word);
    (!loaded.is_empty()).then(|| loaded.iter().any(|l| l.dict.check(word)))
}

/// Up to 6 suggestions for `word` from the dictionaries in its script.
pub fn suggest(dicts: &[Arc<Installed>], word: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in for_word(dicts, word) {
        let mut s = Vec::new();
        l.dict.suggest(word, &mut s);
        for w in s {
            if !out.contains(&w) {
                out.push(w);
            }
        }
    }
    out.truncate(6);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Small dictionaries in a language no other test installs: installing is process-wide.
    const AFF: &str = "SET UTF-8\nTRY abcdefghijklmnopqrstuvwxyz\nSFX A Y 1\nSFX A 0 s .\n";

    #[test]
    fn installs_and_checks_by_script() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("kw_GB")).unwrap();
        std::fs::write(dir.path().join("kw_GB/kw_GB.aff"), AFF).unwrap();
        std::fs::write(dir.path().join("kw_GB/kw_GB.dic"), "3\nkernow/A\nlyver/A\nPenzans\n").unwrap();
        std::fs::write(dir.path().join("kw_GB/hyph_kw_GB.dic"), "UTF-8\n1a\n").unwrap();
        // The same language in Cyrillic, as Serbian has.
        std::fs::write(dir.path().join("kw.aff"), "SET UTF-8\n").unwrap();
        std::fs::write(dir.path().join("kw.dic"), "1\nкерноу\n").unwrap();
        std::fs::write(dir.path().join("kw_XX.aff"), AFF).unwrap();
        let report = install_dir(dir.path());
        assert_eq!(report.len(), 3, "{report:?}");
        assert!(report.iter().any(|l| l.contains("no kw_XX.dic")), "{report:?}");

        let kw = for_language("kw");
        assert_eq!(kw.len(), 2);
        assert_eq!(check(&kw, "kernow"), Some(true));
        assert_eq!(check(&kw, "kernows"), Some(true), "affixes apply");
        assert_eq!(check(&kw, "Penzans"), Some(true));
        assert_eq!(check(&kw, "penzans"), Some(false), "a capitalized word stays capitalized");
        assert_eq!(check(&kw, "керноу"), Some(true), "the Cyrillic dictionary");
        assert_eq!(check(&kw, "kernoq"), Some(false));
        assert!(suggest(&kw, "kernoq").contains(&"kernow".to_string()));
        assert!(suggest(&kw, "kernoq").iter().all(|s| !s.chars().any(is_cyrillic)), "suggestions in the word's script");
        assert!(for_language("Serbian").iter().all(|d| d.language == "sr"));
        assert!(for_language("[No Language]").is_empty());
    }

    #[test]
    fn unreadable_dictionaries_check_nothing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("gv.aff"), AFF).unwrap();
        std::fs::write(dir.path().join("gv.dic"), [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(install_dir(dir.path()).len(), 1);
        assert_eq!(check(&for_language("gv"), "word"), None, "not UTF-8: read once, then skipped");
    }

    #[test]
    fn serbian_codes() {
        assert_eq!(dictionary_language("sr_RS"), Some("sr"));
        assert_eq!(dictionary_language("sh_RS"), Some("sr"));
        assert_eq!(dictionary_language("Serbian: Latin"), Some("sr"));
        assert!(mostly_cyrillic("2\nкерноу\nlyver\n"));
        assert!(!mostly_cyrillic("2\nkernow\nкер\n"));
    }
}
