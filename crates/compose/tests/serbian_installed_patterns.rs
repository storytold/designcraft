//! An installed Serbian Latin pattern set can't see where `nj` is two letters. In a test binary of
//! its own, because installed patterns stay for the process.

use designcraft_compose::hyphen::{Lang, Limits, hyphen_points_in, install_patterns};

#[test]
fn a_word_with_two_letters_goes_past_installed_latin_patterns() {
    // Covers the letters of both words, and breaks between every pair it names.
    install_patterns("sh_RS", "UTF-8\n1ka\n1ko\nk1a\nk1o\no1n\na1n\nn1j\nj1u\nj1o\nu1g\ng1a\na1c\nc1i\ni1j\nj1a\n").unwrap();
    let lang = Lang::for_language("sh_RS").unwrap();
    let lim = Limits { min_word: 2, after_first: 1, before_last: 1, capitalized: true };
    let marked = |w: &str| {
        let pts = hyphen_points_in(w, &lim, lang);
        w.chars().enumerate().map(|(i, c)| if pts.contains(&i) { format!("-{c}") } else { c.to_string() }).collect::<String>()
    };
    assert_eq!(marked("kanjon"), "k-a-nj-o-n", "other words use the installed patterns, and `nj` stays whole");
    assert_eq!(marked("konjugacija"), "kon-ju-ga-ci-ja", "the Cyrillic spelling decides this one");
}
