//! Regenerates the bundled Esperanto root list from English Wiktionary's Esperanto entries as
//! extracted by kaikki.org (CC BY-SA 4.0).
//!
//! ```text
//! curl -LO https://kaikki.org/dictionary/Esperanto/kaikki.org-dictionary-Esperanto.jsonl
//! cargo run --release -p designcraft-compose --example eohyphgen -- kaikki.org-dictionary-Esperanto.jsonl assets/hyphenation [--validate]
//! ```
//!
//! Writes `eo.roots`. Roots are the stems of words borrowed from another language and of function
//! words (prepositions, conjunctions, pronouns, numerals…), then the stems of the parts of each
//! word's decomposition (`tranĉi` + `-ilo`) and of words without an etymology, unless other roots
//! and affixes already build them (`malsan` = `mal` + `san`). `--validate` scores the segmenter
//! against Wiktionary's morpheme hyphenations (`tranĉ‧ilo`).

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use designcraft_compose::hyphen::eo::{PREFIXES, SUFFIXES, ending_len, segment};
use serde_json::Value;

/// Parts of speech whose words are roots as they stand.
const FUNCTION_POS: &[&str] = &["prep", "conj", "pron", "det", "num", "intj", "particle", "article"];

/// Parts of speech that aren't words to take roots from.
const SKIP_POS: &[&str] = &["prefix", "suffix", "interfix", "character", "name"];

/// Etymology templates that borrow or derive from another language.
const FOREIGN: &[&str] = &["bor", "der", "inh", "bor+", "lbor", "ubor"];

/// Compounds Wiktionary lists without a decomposition, kept out of the roots so that their parts
/// break them (`fer-vojo`). Add a stem here when a frequent compound isn't broken.
const NOT_ROOTS: &[&str] = &["fervoj"];

/// Most a decomposition may cost for its word to count as derived rather than a root.
const DERIVED_MAX_COST: f32 = 2.6;

/// Lowercase letters of the Esperanto alphabet only (no q, w, x, y: `acxajx` is the x-system
/// spelling of `aĉaĵ`, not a root).
fn is_letters(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| "abcdefghijklmnoprstuvzĉĝĥĵŝŭ".contains(c))
}

fn is_affix(w: &str) -> bool {
    PREFIXES.contains(&w) || SUFFIXES.contains(&w)
}

fn stem(w: &str) -> String {
    let c: Vec<char> = w.chars().collect();
    c[..c.len() - ending_len(&c)].iter().collect()
}

fn is_form(e: &Value) -> bool {
    e["senses"].as_array().is_some_and(|s| s.iter().any(|s| s.get("form_of").is_some()))
}

fn templates(e: &Value) -> impl Iterator<Item = &Value> {
    e["etymology_templates"].as_array().into_iter().flatten()
}

/// The parts of a word's `{{af|eo|…}}` / `{{ety|eo|:af|…}}` decomposition.
fn af_parts(e: &Value) -> Option<Vec<String>> {
    for t in templates(e) {
        let a = &t["args"];
        let first = match t["name"].as_str() {
            Some("ety") if a["2"] == ":af" => 3,
            Some("af" | "affix") if a["1"] == "eo" => 2,
            _ => continue,
        };
        let parts = (first..).map_while(|k| a[k.to_string()].as_str()).map(|p| p.split('<').next().unwrap_or(p).trim().to_lowercase()).collect();
        return Some(parts);
    }
    None
}

fn foreign_origin(e: &Value) -> bool {
    templates(e).any(|t| t["name"].as_str().is_some_and(|n| FOREIGN.contains(&n)))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let validate = args.iter().any(|a| a == "--validate");
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [src, out] = paths[..] else {
        eprintln!("usage: eohyphgen KAIKKI.jsonl OUT_DIR [--validate]");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(src).expect("read the Kaikki file");
    let entries: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();

    let mut strong = BTreeSet::new();
    let mut weak = BTreeSet::new();
    let mut refs = Vec::new();
    for e in &entries {
        let Some(w) = e["word"].as_str() else { continue };
        if !is_letters(w) {
            continue;
        }
        if let Some(h) = e["hyphenation"].as_array().and_then(|h| h.get(1)).and_then(Value::as_str) {
            refs.push((w.to_string(), h.replace('·', "‧").to_lowercase()));
        }
        if is_form(e) {
            continue;
        }
        let pos = e["pos"].as_str().unwrap_or("");
        if FUNCTION_POS.contains(&pos) {
            strong.insert(w.to_string());
            continue;
        }
        if SKIP_POS.contains(&pos) {
            continue;
        }
        if let Some(parts) = af_parts(e) {
            for p in parts {
                let p = p.trim_matches('-');
                if is_letters(p) && !is_affix(p) {
                    let s = stem(p);
                    if s.chars().count() >= 2 && !is_affix(&s) {
                        weak.insert(s);
                    }
                }
            }
            continue;
        }
        let s = stem(w);
        if s.chars().count() >= 2 {
            if foreign_origin(e) {
                strong.insert(s)
            } else {
                weak.insert(s)
            };
        }
    }

    let mut roots: HashSet<String> = strong.iter().chain(&weak).cloned().collect();
    let mut by_len: Vec<&String> = weak.difference(&strong).collect();
    by_len.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
    for s in by_len {
        let chars: Vec<char> = s.chars().collect();
        roots.remove(s);
        let derived = segment(&chars, &|p| roots.contains(p)).is_some_and(|(starts, cost)| !starts.is_empty() && cost < DERIVED_MAX_COST);
        if !derived {
            roots.insert(s.clone());
        }
    }

    for s in NOT_ROOTS {
        roots.remove(*s);
    }

    let sorted: BTreeSet<&String> = roots.iter().collect();
    let mut body = String::from(
        "# Esperanto roots for hyphenation (crates/compose/src/hyphen/eo.rs), one per line.\n# From English Wiktionary via kaikki.org, CC BY-SA 4.0; regenerate with examples/eohyphgen.rs.\n",
    );
    for r in sorted {
        body.push_str(r);
        body.push('\n');
    }
    let path = PathBuf::from(out).join("eo.roots");
    std::fs::write(&path, body).expect("write eo.roots");
    println!("{} roots → {}", roots.len(), path.display());

    if validate {
        let (mut tp, mut fp, mut fun) = (0usize, 0usize, 0usize);
        let mut exact = 0;
        for (w, r) in &refs {
            let chars: Vec<char> = w.chars().collect();
            let got: BTreeSet<usize> = designcraft_compose::hyphen::eo::breaks_with(&chars, &|p| roots.contains(p)).into_iter().collect();
            let mut want = BTreeSet::new();
            let mut at = 0;
            for c in r.chars() {
                if c == '‧' {
                    want.insert(at);
                } else {
                    at += 1;
                }
            }
            tp += got.intersection(&want).count();
            fp += got.difference(&want).count();
            fun += want.difference(&got).count();
            exact += usize::from(got == want);
        }
        let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
        println!(
            "{} references: {:.1}% exact, break precision {:.1}%, recall {:.1}%",
            refs.len(),
            pct(exact, refs.len()),
            pct(tp, tp + fp),
            pct(tp, tp + fun)
        );
    }
}
