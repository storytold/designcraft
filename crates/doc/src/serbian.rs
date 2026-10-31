//! Serbian spelling that hyphenation and sorting share.

/// Roots of Serbian Latin words where `nj` or `dž` is two letters, not one: the Latin spelling
/// doesn't show it, the Cyrillic does (конјугација, not коњугација; надживети, not наџивети). `|`
/// marks the pair. These only start a word: `an|jon` is not in `kanjon`, nor `od|žal` in `odžak`.
/// Checked against the ~190,000 stems of the Serbian Hunspell dictionaries: they touch no word with
/// one letter but the variants коњугација and коњунктив, which break as the words they vary. A list,
/// not a dictionary lookup, so that a text breaks the same on every machine. The headwords of the
/// Rečnik srpskoga jezika (Matica srpska, 2011) raise no other case. A root stays long enough to
/// leave foreign names alone (Поџи).
const AT_START: &[&str] = &[
    "an|jon",
    "and|žej",
    "feld|žandarm",
    "man|jogan",
    "nad|žet",
    "nad|živ",
    "nad|žnj",
    "nad|žup",
    "od|žali",
    "od|žalj",
    "od|žar",
    "od|živ",
    "pan|jug",
    "pod|žanr",
    "pod|žar",
    "pod|ždrel",
    "pod|žel",
    "pod|žup",
    "pred|žalb",
    "pred|žel",
    "pred|žet",
    "pred|živ",
    "šen|jan",
    "šen|jeng",
    "tan|jug",
    "van|jed",
    "van|jez",
    "van|jugo",
];

/// The same, anywhere in a word (`mikroin|jekcija`, `hiperkon|jugacija`).
const ANYWHERE: &[&str] = &["in|jek", "in|junk", "kon|jekt", "kon|jug", "kon|juk", "kon|junk", "kon|junt"];

/// The index of the second letter of each such pair in a lowercase Serbian Latin word.
pub fn two_letters(word: &[char]) -> Vec<usize> {
    let is_at = |root: &str, at: usize| root.chars().filter(|&c| c != '|').enumerate().all(|(i, c)| word.get(at + i) == Some(&c));
    let mark = |root: &str| root.chars().position(|c| c == '|').unwrap_or(0);
    let mut out: Vec<usize> = AT_START.iter().filter(|r| is_at(r, 0)).map(|r| mark(r)).collect();
    for at in 0..word.len() {
        out.extend(ANYWHERE.iter().filter(|r| is_at(r, at)).map(|r| at + mark(r)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_of_two_letters() {
        let at = |w: &str| two_letters(&w.chars().collect::<Vec<_>>());
        assert_eq!(at("konjugacija"), [3]);
        assert_eq!(at("nadživeti"), [3]);
        assert_eq!(at("mikroinjekcija"), [7]);
        assert!(at("konj").is_empty());
        assert!(at("odžak").is_empty());
    }
}
