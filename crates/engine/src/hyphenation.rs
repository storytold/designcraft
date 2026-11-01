//! Hyphenation pattern files installed by the user: LibreOffice/Hyphen `hyph_<locale>.dic` files
//! (`hyph_sr_RS.dic`, `hyph_sh_RS.dic`, `hyph_de_DE.dic`) in a folder or its subfolders (one per
//! locale, as InDesign's Hunspell folder keeps them), read once at start-up. Their patterns come
//! before the bundled ones for text in that language
//! (see [`designcraft_compose::hyphen::install_patterns`]).

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use designcraft_compose::hyphen;

/// Pattern files larger than this are skipped (the largest LibreOffice ones are under 1 MB).
const MAX_FILE_BYTES: u64 = 8 << 20;
/// At most this many files are read from the folder.
const MAX_FILES: usize = 64;

/// The locale of a pattern file name: `hyph_sr_RS.dic` → `sr_RS`.
fn locale_of(name: &str) -> Option<&str> {
    let locale = name.strip_prefix("hyph_")?.strip_suffix(".dic")?;
    (!locale.is_empty()).then_some(locale)
}

/// The files in `dir` and in its subfolders (one level), by name then path, and one line for each
/// folder or entry that couldn't be read. A missing folder has no files and no such line. Names
/// that aren't UTF-8 are left out.
pub(crate) fn dictionary_files(dir: &Path) -> (Vec<(String, PathBuf)>, Vec<String>) {
    let mut problems = Vec::new();
    let mut list = |d: &Path| -> Vec<(String, PathBuf, bool)> {
        let entries = match crate::io::read_dir(d) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Vec::new(),
            Err(e) => {
                problems.push(format!("{}: {e}", d.display()));
                return Vec::new();
            }
        };
        entries
            .filter_map(|e| e.map_err(|e| problems.push(format!("{}: {e}", d.display()))).ok())
            .filter_map(|e| {
                let is_dir = crate::io::is_dir(e.path());
                Some((e.file_name().into_string().ok()?, e.path(), is_dir))
            })
            .collect()
    };
    let mut files = Vec::new();
    for (name, path, is_dir) in list(dir) {
        if is_dir {
            files.extend(list(&path).into_iter().filter(|f| !f.2).map(|(n, p, _)| (n, p)));
        } else {
            files.push((name, path));
        }
    }
    files.sort();
    (files, problems)
}

/// Install the `hyph_*.dic` files in `dir` and its subfolders, in name order. A missing folder
/// installs nothing. Returns one line per file for the log: what was installed, or why not.
pub fn install_dir(dir: &Path) -> Vec<String> {
    let (files, problems) = dictionary_files(dir);
    let mut files: Vec<(String, PathBuf)> = files.into_iter().filter(|(name, _)| locale_of(name).is_some()).collect();
    let mut report: Vec<String> = problems.into_iter().map(|p| format!("hyphenation: {p}")).collect();
    if files.len() > MAX_FILES {
        report.push(format!("hyphenation: only the first {MAX_FILES} of {} pattern files are read", files.len()));
        files.truncate(MAX_FILES);
    }
    for (name, path) in files {
        let Some(locale) = locale_of(&name) else { continue };
        // A folder or a device with a pattern file's name isn't one; what can't be read is reported.
        if crate::io::exists(&path) && !crate::io::is_file(&path) {
            continue;
        }
        let line = match crate::io::read_capped(&path, MAX_FILE_BYTES) {
            Err(e) => format!("hyphenation: {name}: {e}"),
            Ok(bytes) if bytes.len() as u64 > MAX_FILE_BYTES => format!("hyphenation: {name} skipped: larger than {} MB", MAX_FILE_BYTES >> 20),
            Ok(bytes) => match hyphen::install_patterns(locale, &bytes) {
                Ok((n, 0)) => format!("hyphenation: {name}: {n} patterns for {locale}"),
                Ok((n, skipped)) => format!("hyphenation: {name}: {n} patterns for {locale} ({skipped} not supported)"),
                Err(e) => format!("hyphenation: {name} skipped: {e}"),
            },
        };
        report.push(line);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_file_names() {
        assert_eq!(locale_of("hyph_sr_RS.dic"), Some("sr_RS"));
        assert_eq!(locale_of("hyph_sh_RS.dic"), Some("sh_RS"));
        assert_eq!(locale_of("hyph_.dic"), None);
        assert_eq!(locale_of("sr_RS.dic"), None, "a spelling dictionary");
        assert_eq!(locale_of("hyph_sr_RS.dic.bak"), None);
    }

    #[test]
    fn installs_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        // A language no other test installs or hyphenates: installing is process-wide.
        // A locale folder, as InDesign's Hunspell folder has them.
        std::fs::create_dir(dir.path().join("tpi")).unwrap();
        std::fs::write(dir.path().join("tpi/hyph_tpi.dic"), "UTF-8\nLEFTHYPHENMIN 2\n1pa\n1ta\n").unwrap();
        std::fs::write(dir.path().join("hyph_tpi_PG.dic"), "ISO8859-1\n1pa\n").unwrap();
        std::fs::write(dir.path().join("readme.txt"), "not a pattern file").unwrap();
        let report = install_dir(dir.path());
        assert_eq!(report.len(), 2, "{report:?}");
        assert!(report[0].contains("2 patterns for tpi"), "{report:?}");
        assert!(report[1].contains("skipped"), "{report:?}");
        let lang = hyphen::Lang::for_language("tpi").unwrap();
        assert_eq!(hyphen::hyphen_points_in("papatapa", &hyphen::Limits::default(), lang), vec![2, 4, 6]);
        assert!(install_dir(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn a_file_in_iso_8859_is_read_and_an_unknown_charset_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hyph_tpi.dic"), b"ISO8859-5\n\xD01\xDD\n").unwrap();
        std::fs::write(dir.path().join("hyph_tog.dic"), b"KOI8-R\n1a\n").unwrap();
        let report = install_dir(dir.path());
        assert!(report[0].contains("skipped") && report[0].contains("hyph_tog.dic"), "{report:?}");
        assert!(report[1].contains("hyph_tpi.dic: 1 patterns"), "{report:?}");
    }

    #[test]
    fn a_file_past_the_cap_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hyph_tpi_TO.dic"), vec![b' '; MAX_FILE_BYTES as usize + 1]).unwrap();
        let report = install_dir(dir.path());
        assert_eq!(report.len(), 1, "{report:?}");
        assert!(report[0].contains("larger than 8 MB"), "{report:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_read_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-folder");
        std::fs::write(&file, "").unwrap();
        let report = install_dir(&file);
        assert_eq!(report.len(), 1, "{report:?}");
        assert!(report[0].starts_with("hyphenation: "), "{report:?}");
    }
}
