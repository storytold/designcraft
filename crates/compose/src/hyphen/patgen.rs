//! A small patgen-style trainer (ours): learns Liang hyphenation patterns from a hyphenated word
//! list, level by level. Odd levels add hyphenating patterns, even levels add inhibiting ones.
//!
//! At level `L`, for every pattern length, each inter-letter position whose current value is
//! below `L` is classified as *good* (applying value `L` would fix it) or *bad* (it would break a
//! position that is currently right). A candidate (letters + value position) is selected when
//! `good·good_weight − bad·bad_weight ≥ threshold`; selected patterns are applied to every word
//! before the next length is considered. Used offline by `examples/hyphgen.rs`.

use super::patterns::{Alphabet, FxMap, MAX_PATTERN_LEN, Patterns, apply, pack};

/// Bit position of a candidate's dot (5 bits) in a counting key, above the packed letters.
const DOT_SHIFT: u32 = 6 * MAX_PATTERN_LEN as u32;

/// Parameters of one level.
#[derive(Clone, Copy, Debug)]
pub struct Level {
    pub min_len: usize,
    pub max_len: usize,
    pub good_weight: u32,
    pub bad_weight: u32,
    pub threshold: u32,
}

/// Defaults tuned on the Moby list (see `examples/hyphgen.rs`).
pub const DEFAULT_LEVELS: &[Level] = &[
    Level { min_len: 1, max_len: 4, good_weight: 1, bad_weight: 2, threshold: 20 },
    Level { min_len: 2, max_len: 5, good_weight: 2, bad_weight: 1, threshold: 8 },
    Level { min_len: 3, max_len: 6, good_weight: 1, bad_weight: 4, threshold: 7 },
    Level { min_len: 3, max_len: 7, good_weight: 3, bad_weight: 2, threshold: 2 },
    Level { min_len: 4, max_len: 8, good_weight: 1, bad_weight: 20, threshold: 6 },
    Level { min_len: 4, max_len: 8, good_weight: 3, bad_weight: 1, threshold: 2 },
];

struct Word {
    /// Padded codes: boundary, letters…, boundary.
    codes: Vec<u8>,
    /// Correct hyphen before padded char `p`.
    hyph: Vec<bool>,
    vals: Vec<u8>,
}

/// Accuracy of a pattern set over a word list.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Score {
    pub words: usize,
    pub breaks: usize,
    pub found: usize,
    pub wrong: usize,
}

impl Score {
    pub fn missed(&self) -> usize {
        self.breaks - self.found
    }
}

/// Train patterns from `(lowercase word, break char indices)` pairs. `log` receives progress lines.
pub fn train(words: &[(Vec<char>, Vec<usize>)], levels: &[Level], log: &mut dyn FnMut(String)) -> Patterns {
    let mut alpha = Alphabet::default();
    let mut data: Vec<Word> = Vec::with_capacity(words.len());
    'w: for (w, br) in words {
        let mut codes = Vec::with_capacity(w.len() + 2);
        codes.push(1);
        for &c in w {
            let Some(k) = alpha.intern(c) else { continue 'w };
            codes.push(k);
        }
        codes.push(1);
        let mut hyph = vec![false; codes.len() + 1];
        for &b in br {
            if b > 0 && b < w.len() {
                hyph[b + 1] = true;
            }
        }
        let vals = vec![0; codes.len() + 1];
        data.push(Word { codes, hyph, vals });
    }
    let mut pats: FxMap<u128, Box<[u8]>> = FxMap::default();
    let mut max_len = 0usize;
    for (li, lv) in levels.iter().enumerate() {
        let level = (li + 1) as u8;
        let hyphenating = level % 2 == 1;
        for len in lv.min_len..=lv.max_len.min(MAX_PATTERN_LEN) {
            // (packed letters | dot) → (good, bad)
            let mut counts: FxMap<u128, (u32, u32)> = FxMap::default();
            for w in &data {
                let n = w.codes.len();
                // Positions strictly between the word's letters.
                for p in 2..n - 1 {
                    let v = w.vals[p];
                    if v >= level {
                        continue;
                    }
                    let (good, bad) = if hyphenating {
                        (w.hyph[p] && v % 2 == 0, !w.hyph[p] && v % 2 == 0)
                    } else {
                        (!w.hyph[p] && v % 2 == 1, w.hyph[p] && v % 2 == 1)
                    };
                    if !good && !bad {
                        continue;
                    }
                    for dot in 0..=len {
                        if p < dot || p - dot + len > n {
                            continue;
                        }
                        let s = p - dot;
                        let key = pack(&w.codes[s..s + len]) | ((dot as u128) << DOT_SHIFT);
                        let e = counts.entry(key).or_default();
                        if good {
                            e.0 += 1;
                        } else {
                            e.1 += 1;
                        }
                    }
                }
            }
            // Select.
            let mut new: FxMap<u128, Box<[u8]>> = FxMap::default();
            let mut selected = 0usize;
            for (key, (g, b)) in counts {
                if g > 0 && (g * lv.good_weight) as i64 - (b * lv.bad_weight) as i64 >= lv.threshold as i64 {
                    let dot = ((key >> DOT_SHIFT) & 31) as usize;
                    let letters = key & !(31u128 << DOT_SHIFT);
                    let e = new.entry(letters).or_insert_with(|| vec![0u8; len + 1].into_boxed_slice());
                    e[dot] = level;
                    selected += 1;
                }
            }
            if selected == 0 {
                continue;
            }
            for w in &mut data {
                apply(&new, len, &w.codes, &mut w.vals);
            }
            for (k, v) in new {
                let e = pats.entry(k).or_insert_with(|| vec![0u8; len + 1].into_boxed_slice());
                for (a, b) in e.iter_mut().zip(v.iter()) {
                    *a = (*a).max(*b);
                }
            }
            max_len = max_len.max(len);
            let sc = score_vals(&data);
            log(format!("level {level} len {len}: +{selected} patterns ({} total); found {}/{} wrong {}", pats.len(), sc.found, sc.breaks, sc.wrong));
        }
    }
    // Re-express through the public pattern type (TeX strings) so the alphabet matches at runtime.
    let tmp = Patterns::from_parts(alpha, pats, max_len);
    let mut out = Patterns::default();
    out.add_letters(words.iter().flat_map(|w| w.0.iter().copied()));
    for t in tmp.to_tex() {
        out.insert(&t);
    }
    out
}

fn score_vals(data: &[Word]) -> Score {
    let mut s = Score { words: data.len(), ..Default::default() };
    for w in data {
        for p in 2..w.codes.len() - 1 {
            let on = w.vals[p] % 2 == 1;
            if w.hyph[p] {
                s.breaks += 1;
                if on {
                    s.found += 1;
                }
            } else if on {
                s.wrong += 1;
            }
        }
    }
    s
}

/// Score `pats` against a word list.
pub fn score(pats: &Patterns, words: &[(Vec<char>, Vec<usize>)]) -> Score {
    let mut s = Score::default();
    for (w, br) in words {
        let Some(pts) = pats.points(w) else { continue };
        s.words += 1;
        s.breaks += br.len();
        for p in &pts {
            if br.contains(p) {
                s.found += 1;
            } else {
                s.wrong += 1;
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> (Vec<char>, Vec<usize>) {
        let mut chars = Vec::new();
        let mut br = Vec::new();
        for c in s.chars() {
            if c == '-' {
                br.push(chars.len());
            } else {
                chars.push(c);
            }
        }
        (chars, br)
    }

    #[test]
    fn learns_a_tiny_list() {
        let list: Vec<_> = [
            "hap-pen",
            "hap-pens",
            "hap-pened",
            "pen-cil",
            "pen-cils",
            "rab-bit",
            "rab-bits",
            "kit-ten",
            "kit-tens",
            "but-ter",
            "bet-ter",
            "let-ter",
            "lit-ter",
            "sit-ter",
        ]
        .iter()
        .map(|s| w(s))
        .collect();
        let levels = [
            Level { min_len: 1, max_len: 4, good_weight: 1, bad_weight: 1, threshold: 2 },
            Level { min_len: 1, max_len: 5, good_weight: 1, bad_weight: 1, threshold: 1 },
        ];
        let p = train(&list, &levels, &mut |_| {});
        let sc = score(&p, &list);
        assert_eq!(sc.breaks, list.len());
        assert!(sc.found >= sc.breaks - 1, "{sc:?}");
        assert!(sc.wrong <= 1, "{sc:?}");
        // Generalises to an unseen word with the same shape.
        assert_eq!(p.points(&"bitter".chars().collect::<Vec<_>>()).unwrap(), vec![3]);
    }
}
