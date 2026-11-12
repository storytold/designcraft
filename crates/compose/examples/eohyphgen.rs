//! Regenerates the bundled Esperanto root list from English Wiktionary's Esperanto entries as
//! extracted by kaikki.org (CC BY-SA 4.0; attribution in `ASSETS.md`).
//!
//! ```text
//! curl -LO https://kaikki.org/dictionary/Esperanto/kaikki.org-dictionary-Esperanto.jsonl
//! sha256sum kaikki.org-dictionary-Esperanto.jsonl   # must be SOURCE_SHA256 (downloaded SOURCE_DATE)
//! cargo run --release -p designcraft-compose --example eohyphgen -- kaikki.org-dictionary-Esperanto.jsonl assets/hyphenation [--validate]
//! ```
//!
//! The generator checks the file's SHA-256 itself and refuses another file, so that `eo.roots`
//! always comes from the source recorded in `ASSETS.md`. To move to a newer download, pass
//! `--new-source`, then record the new checksum and date in `SOURCE_SHA256`, `SOURCE_DATE` and the
//! `ASSETS.md` row, with the `--validate` figures.
//!
//! Writes `eo.roots`. Roots are the stems of words borrowed from another language and of function
//! words (prepositions, conjunctions, pronouns, numerals…), then the stems of the parts of each
//! word's decomposition (`tranĉi` + `-ilo`) and of words without an etymology, unless other roots
//! and affixes already build them (`malsan` = `mal` + `san`). `--validate` scores the segmenter
//! against Wiktionary's morpheme hyphenations (`tranĉ‧ilo`).
//!
//! `cargo test -p designcraft-compose --example eohyphgen` runs its tests; with
//! `DESIGNCRAFT_EO_KAIKKI=/path/to/kaikki.org-dictionary-Esperanto.jsonl` they also check that the
//! bundled `eo.roots` is exactly what the recorded source regenerates.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::process::ExitCode;

use designcraft_compose::hyphen::eo::{PREFIXES, SUFFIXES, breaks_with, ending_len, segment};
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
    c.iter().take(c.len() - ending_len(&c)).collect()
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

/// Where the source comes from, its SHA-256 and when it was downloaded (also in `ASSETS.md`).
const SOURCE_URL: &str = "https://kaikki.org/dictionary/Esperanto/kaikki.org-dictionary-Esperanto.jsonl";
const SOURCE_SHA256: &str = "630451509760ce8e1f62383387f2cb389734e4293d63747b40edf02ce555927e";
const SOURCE_DATE: &str = "2026-10-16";

/// SHA-256 of `bytes`, lowercase hex.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The entries of a Kaikki JSONL file, one per non-blank line. A truncated or malformed download
/// is an error, never a shorter root list.
fn parse_entries(text: &str) -> Result<Vec<Value>, String> {
    let mut entries = Vec::new();
    for (i, l) in text.lines().enumerate() {
        if l.trim().is_empty() {
            continue;
        }
        let v = serde_json::from_str(l).map_err(|e| format!("line {}: not a JSON entry ({e})", i + 1))?;
        entries.push(v);
    }
    Ok(entries)
}

/// The roots built from the entries, and the reference hyphenations (word, `‧`-marked) they give.
fn build_roots(entries: &[Value]) -> (HashSet<String>, Vec<(String, String)>) {
    let mut strong = BTreeSet::new();
    let mut weak = BTreeSet::new();
    let mut refs = Vec::new();
    for e in entries {
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
    (roots, refs)
}

/// The text of `eo.roots`: a header naming the source, then the roots sorted, one per line.
fn roots_file(roots: &HashSet<String>, sha256: &str) -> String {
    let sorted: BTreeSet<&String> = roots.iter().collect();
    let mut body = format!(
        "# Esperanto roots for hyphenation (crates/compose/src/hyphen/eo.rs), one per line.\n\
         # From English Wiktionary via kaikki.org, CC BY-SA 4.0 (attribution in ASSETS.md):\n\
         # {SOURCE_URL}, SHA-256 {sha256}.\n\
         # Generated by crates/compose/examples/eohyphgen.rs: regenerate, don't edit.\n"
    );
    for r in sorted {
        body.push_str(r);
        body.push('\n');
    }
    body
}

/// Agreement of the segmenter with the references.
#[derive(Debug, PartialEq)]
struct Score {
    references: usize,
    exact: usize,
    /// Breaks found that the reference has, found but not in the reference, in the reference but
    /// not found.
    tp: usize,
    fp: usize,
    missed: usize,
}

impl Score {
    fn report(&self) -> String {
        let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
        format!(
            "{} references: {:.1}% exact, break precision {:.1}%, recall {:.1}%",
            self.references,
            pct(self.exact, self.references),
            pct(self.tp, self.tp + self.fp),
            pct(self.tp, self.tp + self.missed)
        )
    }
}

/// Char indices of the `‧` marks in a reference hyphenation (`tranĉ‧ilo` → 5).
fn reference_breaks(r: &str) -> BTreeSet<usize> {
    let mut want = BTreeSet::new();
    let mut at = 0;
    for c in r.chars() {
        if c == '‧' {
            want.insert(at);
        } else {
            at += 1;
        }
    }
    want
}

fn validate(roots: &HashSet<String>, refs: &[(String, String)]) -> Score {
    let mut s = Score { references: refs.len(), exact: 0, tp: 0, fp: 0, missed: 0 };
    for (w, r) in refs {
        let chars: Vec<char> = w.chars().collect();
        let got: BTreeSet<usize> = breaks_with(&chars, &|p| roots.contains(p)).into_iter().collect();
        let want = reference_breaks(r);
        s.tp += got.intersection(&want).count();
        s.fp += got.difference(&want).count();
        s.missed += want.difference(&got).count();
        s.exact += usize::from(got == want);
    }
    s
}

/// Reads the source, checks it, and writes `eo.roots` into `out` only once everything succeeded.
/// Returns what to print.
fn run(src: &Path, out: &Path, validate_too: bool, new_source: bool) -> Result<String, String> {
    let bytes = std::fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let sha = sha256_hex(&bytes);
    if sha != SOURCE_SHA256 && !new_source {
        return Err(format!(
            "{}: SHA-256 {sha}, expected {SOURCE_SHA256} (the file downloaded {SOURCE_DATE}); pass --new-source to use another download",
            src.display()
        ));
    }
    let text = String::from_utf8(bytes).map_err(|e| format!("{}: not UTF-8 ({e})", src.display()))?;
    let entries = parse_entries(&text).map_err(|e| format!("{}: {e}; eo.roots left unchanged", src.display()))?;
    let (roots, refs) = build_roots(&entries);
    if roots.is_empty() {
        return Err(format!("{}: no usable Esperanto roots; eo.roots left unchanged", src.display()));
    }
    let path = out.join("eo.roots");
    std::fs::write(&path, roots_file(&roots, &sha)).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut msg = format!("{} roots → {}", roots.len(), path.display());
    if sha != SOURCE_SHA256 {
        msg.push_str(&format!("\nnew source SHA-256 {sha}: record it in SOURCE_SHA256, SOURCE_DATE and ASSETS.md"));
    }
    if validate_too {
        msg.push('\n');
        msg.push_str(&validate(&roots, &refs).report());
    }
    Ok(msg)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [src, out] = paths[..] else {
        eprintln!("usage: eohyphgen KAIKKI.jsonl OUT_DIR [--validate] [--new-source]");
        return ExitCode::from(2);
    };
    match run(Path::new(src), Path::new(out), flag("--validate"), flag("--new-source")) {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decompositions_are_read_from_af_and_ety() {
        let af = json!({"etymology_templates": [{"name": "af", "args": {"1": "eo", "2": "tranĉi", "3": "-ilo"}}]});
        assert_eq!(af_parts(&af), Some(vec!["tranĉi".to_string(), "-ilo".to_string()]));
        let ety = json!({"etymology_templates": [{"name": "ety", "args": {"1": "eo", "2": ":af", "3": "Mal<t:not>", "4": "sana"}}]});
        assert_eq!(af_parts(&ety), Some(vec!["mal".to_string(), "sana".to_string()]), "lowercased, annotations dropped");
        let other_lang = json!({"etymology_templates": [{"name": "af", "args": {"1": "io", "2": "x"}}]});
        assert_eq!(af_parts(&other_lang), None);
        assert_eq!(af_parts(&json!({})), None);
        assert_eq!(af_parts(&json!({"etymology_templates": "not a list"})), None);
    }

    #[test]
    fn stems_and_letters() {
        assert_eq!(stem("tranĉilo"), "tranĉil");
        assert_eq!(stem("domojn"), "dom");
        assert_eq!(stem("kaj"), "kaj");
        assert_eq!(stem(""), "");
        assert!(is_letters("ŝipo"));
        assert!(!is_letters("acxajx"), "x-system spelling");
        assert!(!is_letters("Ŝipo"), "capitals are names");
        assert!(!is_letters(""));
    }

    #[test]
    fn foreign_origin_and_forms() {
        assert!(foreign_origin(&json!({"etymology_templates": [{"name": "bor", "args": {}}]})));
        assert!(!foreign_origin(&json!({"etymology_templates": [{"name": "af", "args": {}}]})));
        assert!(!foreign_origin(&json!({})));
        assert!(is_form(&json!({"senses": [{"form_of": [{"word": "domo"}]}]})));
        assert!(!is_form(&json!({"senses": [{"glosses": ["house"]}]})));
    }

    fn entry(word: &str, pos: &str, ety: Value) -> Value {
        json!({"word": word, "pos": pos, "etymology_templates": ety, "senses": [{"glosses": ["x"]}]})
    }

    #[test]
    fn roots_come_from_borrowings_function_words_and_decomposition_parts() {
        let entries = vec![
            entry("tranĉi", "verb", json!([{"name": "bor", "args": {}}])),
            entry("sana", "adj", json!([{"name": "der", "args": {}}])),
            entry("tranĉilo", "noun", json!([{"name": "af", "args": {"1": "eo", "2": "tranĉi", "3": "-ilo"}}])),
            entry("malsana", "adj", json!([])),
            entry("kaj", "conj", json!([])),
            entry("-ilo", "suffix", json!([])),
            entry("Parizo", "name", json!([])),
        ];
        let (roots, _) = build_roots(&entries);
        let mut got: Vec<&str> = roots.iter().map(String::as_str).collect();
        got.sort_unstable();
        // `malsan` = `mal` + `san`: derived, not a root; `il` is a suffix; names are skipped.
        assert_eq!(got, ["kaj", "san", "tranĉ"]);
    }

    #[test]
    fn the_roots_file_is_sorted_with_its_source_in_the_header() {
        let roots: HashSet<String> = ["voj", "akv", "bird"].map(String::from).into();
        let text = roots_file(&roots, "abc");
        assert!(text.contains(SOURCE_URL) && text.contains("SHA-256 abc") && text.contains("ASSETS.md"));
        let body: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(body, ["akv", "bird", "voj"]);
    }

    #[test]
    fn validation_counts_exact_words_and_breaks() {
        assert_eq!(reference_breaks("tranĉ‧ilo"), BTreeSet::from([5]));
        assert_eq!(reference_breaks("mal‧san‧ul‧ejo"), BTreeSet::from([3, 6, 8]));
        let roots: HashSet<String> = ["tranĉ", "san"].map(String::from).into();
        let refs = vec![
            ("tranĉilo".to_string(), "tranĉ‧ilo".to_string()),        // exact
            ("malsanulejo".to_string(), "mal‧san‧ulejo".to_string()), // one break too many
            ("turnis".to_string(), "turn‧is".to_string()),            // unknown root: one missed
        ];
        let s = validate(&roots, &refs);
        assert_eq!(s, Score { references: 3, exact: 1, tp: 3, fp: 1, missed: 1 });
        assert_eq!(s.report(), "3 references: 33.3% exact, break precision 75.0%, recall 75.0%");
        assert_eq!(validate(&roots, &[]).report(), "0 references: 0.0% exact, break precision 0.0%, recall 0.0%");
    }

    /// An empty scratch directory in the system temp dir.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eohyphgen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_bad_source_never_overwrites_eo_roots() {
        let dir = scratch("bad");
        let out = dir.join("eo.roots");
        std::fs::write(&out, "kept\n").unwrap();
        let src = dir.join("src.jsonl");
        // Not the recorded download.
        std::fs::write(&src, "{\"word\": \"kaj\", \"pos\": \"conj\"}\n").unwrap();
        assert!(run(&src, &dir, false, false).unwrap_err().contains("SHA-256"));
        // Malformed JSON, even when another source is allowed.
        std::fs::write(&src, "{\"word\": \"kaj\", \"pos\": \"conj\"}\n{not json\n").unwrap();
        assert!(run(&src, &dir, true, true).unwrap_err().contains("line 2"));
        // No usable roots.
        std::fs::write(&src, "\n{\"word\": \"Parizo\", \"pos\": \"name\"}\n").unwrap();
        assert!(run(&src, &dir, false, true).unwrap_err().contains("no usable"));
        // Missing file.
        assert!(run(&dir.join("missing.jsonl"), &dir, false, true).is_err());
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "kept\n");
        // A good one is written, with its checksum in the header and a reminder to record it.
        std::fs::write(&src, "{\"word\": \"kaj\", \"pos\": \"conj\", \"hyphenation\": [\"\", \"kaj\"]}\n").unwrap();
        let msg = run(&src, &dir, true, true).unwrap();
        assert!(msg.contains("1 roots") && msg.contains("new source SHA-256") && msg.contains("1 references"), "{msg}");
        let written = std::fs::read_to_string(&out).unwrap();
        assert!(written.ends_with("\nkaj\n") && written.contains(&sha256_hex(&std::fs::read(&src).unwrap())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With the recorded source at hand (`DESIGNCRAFT_EO_KAIKKI`), the bundled list is exactly what
    /// it regenerates. Skipped without it: the source is 100+ MB and isn't in the repository.
    #[test]
    fn the_bundled_roots_regenerate_from_the_recorded_source() {
        let Some(src) = std::env::var_os("DESIGNCRAFT_EO_KAIKKI") else {
            eprintln!("DESIGNCRAFT_EO_KAIKKI unset: regeneration check skipped");
            return;
        };
        let dir = scratch("regen");
        run(Path::new(&src), &dir, false, false).unwrap();
        let bundled = include_str!("../../../assets/hyphenation/eo.roots");
        assert!(std::fs::read_to_string(dir.join("eo.roots")).unwrap() == bundled, "eo.roots differs from what the recorded source regenerates");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
