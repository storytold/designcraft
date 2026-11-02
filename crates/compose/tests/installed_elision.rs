//! Installed French patterns and elisions: when the installed set doesn't cover a word with an
//! elision (`l'imprimerie`), the bundled patterns hyphenate it, and the paragraph limits still
//! count only the word after the apostrophe. In a test binary of its own, because installed
//! patterns stay for the process.

use designcraft_compose::hyphen::{Lang, Limits, hyphen_points_in, install_patterns};

#[test]
fn elision_limits_apply_to_installed_french_patterns() {
    // An alphabet of one letter, so the installed set covers no word below.
    install_patterns("fr_FR", "UTF-8\n1q\n").unwrap();
    let fr = Lang::for_language("fr_FR").unwrap();
    assert!(matches!(fr, Lang::Installed(_)));
    let lim = Limits { min_word: 5, after_first: 3, before_last: 2, capitalized: true };
    assert_eq!(hyphen_points_in("l'imprimerie", &lim, fr), vec![7, 9]);
    let lim = Limits { min_word: 11, ..Limits::default() };
    assert!(hyphen_points_in("l'imprimerie", &lim, fr).is_empty(), "imprimerie has ten letters");
}
