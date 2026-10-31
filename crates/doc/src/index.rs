//! Index (Window › Type & Tables › Index): page references marked in stories and the generated
//! index story.
//!
//! A page reference is a zero-width [`INDEX_MARK`] in a story; [`Story::index_refs`] holds one
//! [`IndexRef`] per mark in text order (like footnotes and cross-references). Generating the
//! index composes the document to find each mark's page and writes a story with "Index Title",
//! "Index Section Head" and "Index Level n" paragraph styles.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::StoryId;

/// A page reference (index marker).
pub const INDEX_MARK: char = '\u{E00D}';

/// The page range a reference covers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "value")]
pub enum IndexRange {
    #[default]
    CurrentPage,
    /// From the marker's page to the page where the story ends.
    ToEndOfStory,
    /// From the marker's page through the next `n` paragraphs.
    NextParagraphs(u32),
    /// The topic is listed without page numbers.
    SuppressPageRange,
    /// "See …" instead of page numbers.
    See(String),
    /// Page numbers followed by "See also …".
    SeeAlso(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRef {
    /// Topic levels, 1 to 4 (e.g. `["Typography", "kerning"]`).
    pub topics: Vec<String>,
    /// Sort keys per level (empty = the topic itself).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<String>,
    #[serde(default)]
    pub range: IndexRange,
}

/// Generate Index options (kept for regeneration).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexSpec {
    pub story: StoryId,
    pub title: String,
    /// Include Index Section Headings (A, B, C…).
    pub section_headings: bool,
    /// Run-in: subtopics follow their topic in one paragraph (else nested paragraphs).
    pub run_in: bool,
    /// Entry separators.
    pub following_topic: String,
    pub between_pages: String,
    pub page_range: String,
    pub between_entries: String,
    pub before_cross_ref: String,
}

impl Default for IndexSpec {
    fn default() -> Self {
        IndexSpec {
            story: StoryId(0),
            title: "Index".into(),
            section_headings: true,
            run_in: false,
            following_topic: "  ".into(),
            between_pages: ", ".into(),
            page_range: "\u{2013}".into(),
            between_entries: "; ".into(),
            before_cross_ref: ". ".into(),
        }
    }
}

/// One reference with its resolved page span (absolute pages; None for page-less references).
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub r: IndexRef,
    pub pages: Option<(usize, usize)>,
    /// The language of the text at the marker, which says whether `lj`, `nj` and `dž` are letters.
    pub language: String,
}

#[derive(Default, Debug)]
struct Node {
    name: String,
    pages: Vec<(usize, usize)>,
    see: Vec<String>,
    see_also: Vec<String>,
    children: BTreeMap<String, Node>,
}

/// Does `language` treat `lj`, `nj` and `dž` as single letters (Serbian, Croatian, Bosnian)?
fn has_digraphs(language: &str) -> bool {
    crate::language_tag(language).is_some_and(|t| matches!(crate::language_subtag(t), "sr" | "sh" | "hr" | "bs"))
}

/// `s` lowercase and without leading punctuation, as a list of chars.
fn sortable(s: &str) -> Vec<char> {
    let k: String = s.trim().trim_start_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    if k.is_empty() { s.to_lowercase().chars().collect() } else { k.chars().collect() }
}

/// The indices of the first letter of each `lj`, `nj` and `dž` of `word` that is one letter.
fn digraphs_of(word: &[char]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < word.len() {
        let end = word[i..].iter().position(|c| !c.is_alphabetic()).map_or(word.len(), |n| i + n);
        let two = crate::serbian::two_letters(&word[i..end]);
        for a in i..end.saturating_sub(1) {
            if matches!((word[a], word[a + 1]), ('l' | 'n', 'j') | ('d', 'ž')) && !two.contains(&(a + 1 - i)) {
                out.push(a);
            }
        }
        i = end + 1;
    }
    out
}

/// The sort key of `s`: code-point order, with the letters that order misplaces right after the
/// one they follow (each is its base letter and a mark above every other char), so that Serbian
/// follows its azbuka and abeceda (Pravopis srpskoga jezika, т. 4). None of these letters changes
/// another alphabet's order. `digraphs`: `lj`, `nj` and `dž` are letters too, which in Latin only
/// the language can tell.
fn sort_key(s: &str, digraphs: bool) -> String {
    const FIRST: char = '\u{10FFFE}';
    const SECOND: char = '\u{10FFFF}';
    let word = sortable(s);
    let pairs = if digraphs { digraphs_of(&word) } else { vec![] };
    let mut out = String::with_capacity(s.len() * 2);
    let mut i = 0;
    while let Some(&c) = word.get(i) {
        if pairs.contains(&i) {
            out.push(c);
            out.push(if c == 'd' { FIRST } else { SECOND });
            i += 2;
            continue;
        }
        let (base, mark) = match c {
            'č' => ('c', Some(FIRST)),
            'ć' => ('c', Some(SECOND)),
            'đ' => ('d', Some(SECOND)),
            'š' => ('s', Some(SECOND)),
            'ž' => ('z', Some(SECOND)),
            'ђ' => ('д', Some(SECOND)),
            'ј' => ('и', Some(SECOND)),
            'љ' => ('л', Some(SECOND)),
            'њ' => ('н', Some(SECOND)),
            'ћ' => ('т', Some(SECOND)),
            'џ' => ('ч', Some(SECOND)),
            c => (c, None),
        };
        out.push(base);
        out.extend(mark);
        i += 1;
    }
    out
}

/// The section head of `s`, the alphabet's first letter of it (`Č`, `Lj`), or `#` for a symbol.
fn section_head(s: &str, digraphs: bool) -> Option<String> {
    let word = sortable(s);
    let c = *word.first()?;
    if !c.is_alphabetic() {
        return Some("#".into());
    }
    // Georgian headings stay Mkhedruli (no capitals; Mtavruli is only for all-caps text).
    let mut head: String = if crate::capitalizes(c) { c.to_uppercase().collect() } else { c.to_string() };
    if digraphs && digraphs_of(&word).first() == Some(&0) {
        head.extend(word.get(1));
    }
    Some(head)
}

/// Record one reference's see-target or page on its topic node.
fn record_ref(n: &mut Node, res: &Resolved) {
    match &res.r.range {
        IndexRange::See(t) => n.see.push(t.clone()),
        IndexRange::SeeAlso(t) => {
            n.see_also.push(t.clone());
            if let Some(p) = res.pages {
                n.pages.push(p);
            }
        }
        IndexRange::SuppressPageRange => {}
        _ => {
            if let Some(p) = res.pages {
                n.pages.push(p);
            }
        }
    }
}

/// The index as paragraphs `(style, text)`; `page_name` formats an absolute page.
pub fn build(spec: &IndexSpec, refs: &[Resolved], page_name: &dyn Fn(usize) -> String) -> Vec<(String, String)> {
    let mut root: BTreeMap<String, Node> = BTreeMap::new();
    let mut heads: BTreeMap<String, Option<String>> = BTreeMap::new();
    for res in refs {
        let topics: Vec<&String> = res.r.topics.iter().filter(|t| !t.trim().is_empty()).take(4).collect();
        if topics.is_empty() {
            continue;
        }
        let digraphs = has_digraphs(&res.language);
        let source = |i: usize, t: &'_ String| res.r.sort.get(i).filter(|s| !s.is_empty()).cloned().unwrap_or_else(|| t.to_string());
        let keys: Vec<(String, String)> = topics
            .iter()
            .enumerate()
            .map(|(i, t)| (format!("{}\u{0}{}", sort_key(&source(i, t), digraphs), t.trim()), t.trim().to_string()))
            .collect();
        if let (Some((key, _)), Some(t)) = (keys.first(), topics.first()) {
            heads.entry(key.clone()).or_insert_with(|| section_head(&source(0, t), digraphs));
        }
        let n = descend(&mut root, &keys);
        record_ref(n, res);
    }
    let mut out = Vec::new();
    if !spec.title.is_empty() {
        out.push(("Index Title".to_string(), spec.title.clone()));
    }
    let mut heading: Option<String> = None;
    for (key, n) in &root {
        if spec.section_headings {
            let head = heads.get(key).cloned().flatten();
            if head != heading {
                heading = head.clone();
                if let Some(head) = head {
                    out.push(("Index Section Head".into(), if head == "#" { "Symbols".into() } else { head }));
                }
            }
        }
        emit(spec, n, 1, page_name, &mut out);
    }
    out
}

/// The node for a topic path (created as needed).
fn descend<'a>(level: &'a mut BTreeMap<String, Node>, keys: &[(String, String)]) -> &'a mut Node {
    let (k, name) = &keys[0];
    let n = level.entry(k.clone()).or_insert_with(|| Node { name: name.clone(), ..Default::default() });
    if keys.len() == 1 { n } else { descend(&mut n.children, &keys[1..]) }
}

fn entry_text(spec: &IndexSpec, n: &Node, page_name: &dyn Fn(usize) -> String) -> String {
    let mut pages = n.pages.clone();
    pages.sort_unstable();
    pages.dedup();
    // Drop single pages already inside a range.
    let ranges: Vec<(usize, usize)> = pages.iter().copied().filter(|&(a, b)| a != b).collect();
    pages.retain(|&(a, b)| a != b || !ranges.iter().any(|&(x, y)| x <= a && a <= y));
    let list: Vec<String> =
        pages.iter().map(|&(a, b)| if a == b { page_name(a) } else { format!("{}{}{}", page_name(a), spec.page_range, page_name(b)) }).collect();
    let mut t = n.name.clone();
    if !list.is_empty() {
        t.push_str(&spec.following_topic);
        t.push_str(&list.join(&spec.between_pages));
    }
    if !n.see.is_empty() {
        t.push_str(if list.is_empty() { &spec.following_topic } else { &spec.before_cross_ref });
        t.push_str("See ");
        t.push_str(&n.see.join(&spec.between_entries));
    }
    if !n.see_also.is_empty() {
        t.push_str(&spec.before_cross_ref);
        t.push_str("See also ");
        t.push_str(&n.see_also.join(&spec.between_entries));
    }
    t
}

// `build` limits topic paths to four levels, bounding this traversal.
fn run_in_subtopics(spec: &IndexSpec, n: &Node, page_name: &dyn Fn(usize) -> String) -> Vec<String> {
    n.children.values().flat_map(|c| [entry_text(spec, c, page_name)].into_iter().chain(run_in_subtopics(spec, c, page_name))).collect()
}

fn emit(spec: &IndexSpec, n: &Node, level: usize, page_name: &dyn Fn(usize) -> String, out: &mut Vec<(String, String)>) {
    if spec.run_in && level == 1 {
        let mut t = entry_text(spec, n, page_name);
        let subs = run_in_subtopics(spec, n, page_name);
        if !subs.is_empty() {
            t.push_str(": ");
            t.push_str(&subs.join(&spec.between_entries));
        }
        out.push(("Index Level 1".into(), t));
        return;
    }
    out.push((format!("Index Level {level}"), entry_text(spec, n, page_name)));
    for c in n.children.values() {
        emit(spec, c, (level + 1).min(4), page_name, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(topics: &[&str], range: IndexRange, pages: Option<(usize, usize)>) -> Resolved {
        Resolved { r: IndexRef { topics: topics.iter().map(|s| s.to_string()).collect(), sort: vec![], range }, pages, language: String::new() }
    }

    #[test]
    fn run_in_keeps_all_topic_levels_in_sort_order() {
        let refs = vec![
            r(&["Typography", "kerning", "pairs", "optical"], IndexRange::CurrentPage, Some((4, 4))),
            r(&["Typography", "kerning", "pairs"], IndexRange::CurrentPage, Some((2, 2))),
            r(&["Typography", "leading"], IndexRange::CurrentPage, Some((6, 6))),
            r(&["Typography", "kerning", "adjustment"], IndexRange::See("Spacing".into()), None),
        ];
        let spec = IndexSpec { run_in: true, title: String::new(), section_headings: false, between_entries: " / ".into(), ..IndexSpec::default() };
        let out = build(&spec, &refs, &|p| (p + 1).to_string());
        assert_eq!(out, [("Index Level 1".into(), "Typography: kerning / adjustment  See Spacing / pairs  3 / optical  5 / leading  7".into())]);
    }

    #[test]
    fn builds_nested_index() {
        let refs = vec![
            r(&["Typography", "kerning"], IndexRange::CurrentPage, Some((4, 4))),
            r(&["Typography"], IndexRange::ToEndOfStory, Some((1, 3))),
            r(&["Typography"], IndexRange::CurrentPage, Some((2, 2))),
            r(&["typography", "Leading"], IndexRange::CurrentPage, Some((6, 6))),
            r(&["Baseline grid"], IndexRange::CurrentPage, Some((0, 0))),
            r(&["Grid"], IndexRange::See("Baseline grid".into()), None),
            r(&["Ampersand"], IndexRange::SeeAlso("Glyphs".into()), Some((9, 9))),
        ];
        let spec = IndexSpec::default();
        let out = build(&spec, &refs, &|p| (p + 1).to_string());
        let lines: Vec<String> = out.iter().map(|(s, t)| format!("{s}|{t}")).collect();
        assert_eq!(
            lines,
            [
                "Index Title|Index",
                "Index Section Head|A",
                "Index Level 1|Ampersand  10. See also Glyphs",
                "Index Section Head|B",
                "Index Level 1|Baseline grid  1",
                "Index Section Head|G",
                "Index Level 1|Grid  See Baseline grid",
                "Index Section Head|T",
                "Index Level 1|Typography  2\u{2013}4",
                "Index Level 2|kerning  5",
                "Index Level 1|typography",
                "Index Level 2|Leading  7",
            ]
        );
        let run_in = build(&IndexSpec { run_in: true, section_headings: false, title: String::new(), ..spec }, &refs[..2], &|p| (p + 1).to_string());
        assert_eq!(run_in, [("Index Level 1".to_string(), "Typography  2\u{2013}4: kerning  5".to_string())]);
    }
    /// The entries and section heads of `topics` in `language`.
    fn sorted(topics: &[&str], language: &str) -> (Vec<String>, Vec<String>) {
        let refs: Vec<Resolved> =
            topics.iter().map(|t| Resolved { language: language.into(), ..r(&[t], IndexRange::CurrentPage, Some((0, 0))) }).collect();
        let spec = IndexSpec { title: String::new(), following_topic: "|".into(), ..Default::default() };
        let out = build(&spec, &refs, &|p| (p + 1).to_string());
        let of = |style: bool| {
            out.iter().filter(|(s, _)| (s == "Index Section Head") == style).map(|(_, t)| t.split('|').next().unwrap_or(t).to_string()).collect()
        };
        (of(false), of(true))
    }

    #[test]
    fn serbian_cyrillic_follows_the_azbuka_in_any_language() {
        let want = [
            "ангел",
            "дан",
            "ђак",
            "евро",
            "игра",
            "јелен",
            "кап",
            "лав",
            "љубав",
            "мир",
            "нос",
            "њива",
            "окно",
            "тон",
            "ћилим",
            "ум",
            "чај",
            "џеп",
            "шума",
        ];
        let mut shuffled = want;
        shuffled.reverse();
        for language in ["Serbian: Cyrillic", "English: USA", ""] {
            let (entries, heads) = sorted(&shuffled, language);
            assert_eq!(entries, want);
            assert_eq!(heads[2], "Ђ");
            assert_eq!(heads[8], "Љ");
        }
    }

    #[test]
    fn serbian_latin_follows_the_abeceda() {
        let want = [
            "ako", "cvet", "čaj", "ćup", "dan", "džep", "đak", "ime", "lav", "ljubav", "mir", "nos", "njiva", "oko", "sat", "šuma", "tužan", "zima",
            "žaba",
        ];
        let mut shuffled = want;
        shuffled.reverse();
        let (entries, heads) = sorted(&shuffled, "Serbian: Latin");
        assert_eq!(entries, want);
        assert_eq!(heads, ["A", "C", "Č", "Ć", "D", "Dž", "Đ", "I", "L", "Lj", "M", "N", "Nj", "O", "S", "Š", "T", "Z", "Ž"]);
    }

    /// Where `nj` is two letters, the word sorts by them: `konjugacija` before `konj`.
    #[test]
    fn serbian_latin_knows_pairs_of_two_letters() {
        let (entries, _) = sorted(&["konj", "konjugacija", "konopac", "kos"], "sr_RS");
        assert_eq!(entries, ["konjugacija", "konopac", "konj", "kos"]);
        let (entries, heads) = sorted(&["nadživeti", "nadžak", "Injekcija", "Njegoš"], "Serbian: Latin");
        assert_eq!(entries, ["Injekcija", "nadživeti", "nadžak", "Njegoš"]);
        assert_eq!(heads, ["I", "N", "Nj"]);
    }

    #[test]
    fn croatian_shares_the_digraphs_and_english_does_not() {
        assert_eq!(sorted(&["lopta", "ljubav"], "hr-HR").0, ["lopta", "ljubav"]);
        assert_eq!(sorted(&["lopta", "Ljubljana"], "English: USA").0, ["Ljubljana", "lopta"]);
        // The letters with marks follow their base letter in any language.
        assert_eq!(sorted(&["dan", "cvet", "čaj"], "English: USA").0, ["cvet", "čaj", "dan"]);
    }

    #[test]
    fn georgian_section_heads_stay_mkhedruli() {
        let refs = vec![r(&["ასო"], IndexRange::CurrentPage, Some((0, 0))), r(&["ბადე"], IndexRange::CurrentPage, Some((1, 1)))];
        let out = build(&IndexSpec::default(), &refs, &|p| (p + 1).to_string());
        let heads: Vec<&str> = out.iter().filter(|(s, _)| s == "Index Section Head").map(|(_, t)| t.as_str()).collect();
        assert_eq!(heads, ["ა", "ბ"]);
        assert!(crate::capitalizes('a') && !crate::capitalizes('ა') && !crate::capitalizes('Ა'));
    }
}
