//! Installed Serbian pattern sets and the two scripts: a Latin set can't see where `nj` is two
//! letters, and a Cyrillic set serves Latin text too. In a test binary of its own, because
//! installed patterns stay for the process; the two sets cover different words.

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

#[test]
fn a_latin_word_uses_installed_cyrillic_patterns_before_the_bundled_ones() {
    // Covers the letters of универзитет, and breaks before each of them.
    install_patterns("sr_RS", "UTF-8\n1у\n1н\n1и\n1в\n1е\n1р\n1з\n1т\n").unwrap();
    let lang = Lang::for_language("sr_RS").unwrap();
    let lim = Limits { min_word: 2, after_first: 1, before_last: 1, capitalized: true };
    let marked = |w: &str| {
        let pts = hyphen_points_in(w, &lim, lang);
        w.chars().enumerate().map(|(i, c)| if pts.contains(&i) { format!("-{c}") } else { c.to_string() }).collect::<String>()
    };
    assert_eq!(marked("универзитет"), "у-н-и-в-е-р-з-и-т-е-т");
    assert_eq!(marked("univerzitet"), "u-n-i-v-e-r-z-i-t-e-t", "the Latin word through its Cyrillic spelling");
    assert_eq!(marked("škola"), "ško-la", "a word outside the installed alphabet keeps the bundled patterns");
}
