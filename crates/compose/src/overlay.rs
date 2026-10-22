//! Character styles that paragraphs apply by themselves: nested styles (from the paragraph start
//! through or up to a delimiter) and GREP styles (every match of a pattern).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use designcraft_doc::{GrepStyle, NestedStyle, NestedUntil};

use crate::grep::{Grep, GrepOptions};

thread_local! {
    /// Compiled GREP styles by pattern; None for one that doesn't compile or gave up on a search.
    static PATTERNS: RefCell<HashMap<String, Option<Grep>>> = RefCell::new(HashMap::new());
}

fn is_event(u: &NestedUntil, c: char, prev: Option<char>, next: Option<char>) -> bool {
    match u {
        NestedUntil::Sentences => matches!(c, '.' | '!' | '?') && next.is_none_or(char::is_whitespace),
        NestedUntil::Words => c.is_whitespace() && prev.is_some_and(|p| !p.is_whitespace()),
        NestedUntil::Characters => !matches!(c, '\n'),
        NestedUntil::Letters => c.is_alphabetic(),
        NestedUntil::Digits => c.is_numeric(),
        NestedUntil::Tab => c == '\t',
        NestedUntil::ForcedLineBreak => c == designcraft_doc::FORCED_LINE_BREAK,
        NestedUntil::EmSpace => c == '\u{2003}',
        NestedUntil::EnSpace => c == '\u{2002}',
        // Counted by position, not by character (see `overlays`).
        NestedUntil::Dropcap => false,
        NestedUntil::Chars(s) => s.contains(c),
    }
}

/// Character-style ranges for the paragraph `text[range]`, in application order (later wins).
/// The paragraph's drop cap ends at `drop_end` (`range.start` without one).
pub(crate) fn overlays(text: &str, range: Range<usize>, nested: &[NestedStyle], grep: &[GrepStyle], drop_end: usize) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    let para = &text[range.clone()];
    // The paragraph's own end mark isn't styled.
    let end = range.start + para.trim_end_matches('\n').len();
    let mut pos = range.start;
    for ns in nested {
        if pos >= end {
            break;
        }
        let mut stop = end;
        if ns.until == NestedUntil::Dropcap {
            // Through the drop cap runs to its end; up to it (it starts the paragraph) is nothing.
            stop = if ns.through { drop_end.max(pos).min(end) } else { pos };
        } else {
            let mut count = 0;
            let chars: Vec<(usize, char)> = text[pos..end].char_indices().map(|(i, c)| (pos + i, c)).collect();
            for (k, &(i, c)) in chars.iter().enumerate() {
                let prev = if k > 0 { Some(chars[k - 1].1) } else { None };
                let next = chars.get(k + 1).map(|x| x.1);
                if is_event(&ns.until, c, prev, next) {
                    count += 1;
                    if count >= ns.count.max(1) {
                        stop = if ns.through { i + c.len_utf8() } else { i };
                        break;
                    }
                }
            }
        }
        if stop > pos && !ns.style.is_empty() {
            out.push((pos..stop, ns.style.clone()));
        }
        pos = stop;
    }
    for g in grep {
        if g.style.is_empty() || g.pattern.is_empty() {
            continue;
        }
        PATTERNS.with(|p| {
            let mut p = p.borrow_mut();
            let slot = p.entry(g.pattern.clone()).or_insert_with(|| {
                Grep::new(&g.pattern, GrepOptions::default()).map_err(|e| log::warn!("GREP style {:?} skipped: {e}", g.pattern)).ok()
            });
            let Some(re) = slot else { return };
            match re.find_all(text.get(range.start..end).unwrap_or(""), usize::MAX) {
                Ok(ms) => out.extend(ms.into_iter().filter(|m| !m.is_empty()).map(|m| (range.start + m.start..range.start + m.end, g.style.clone()))),
                Err(e) => {
                    log::warn!("GREP style {:?} disabled: {e}", g.pattern);
                    *slot = None;
                }
            }
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_and_grep_ranges() {
        let text = "Q: what is 42? A drop of 7 words.\n".to_string();
        let n = |style: &str, through: bool, count: u32, until: NestedUntil| NestedStyle { style: style.into(), through, count, until };
        // Through the first ':' then up to the second word end.
        let o =
            overlays(&text, 0..text.len(), &[n("Lead", true, 1, NestedUntil::Chars(":".into())), n("Next", false, 2, NestedUntil::Words)], &[], 0);
        assert_eq!(&text[o[0].0.clone()], "Q:");
        assert_eq!(&text[o[1].0.clone()], " what is", "up to the end of the second word");
        // Digits by GREP.
        let o = overlays(&text, 0..text.len(), &[], &[GrepStyle { style: "Num".into(), pattern: r"\d+".into() }], 0);
        let got: Vec<&str> = o.iter().map(|(r, _)| &text[r.clone()]).collect();
        assert_eq!(got, ["42", "7"]);
        // Through the drop cap (its first 2 bytes here); up to it is nothing; without one, nothing.
        let d = |through: bool| n("Cap", through, 1, NestedUntil::Dropcap);
        let o = overlays(&text, 0..text.len(), &[d(true), n("Next", true, 1, NestedUntil::Words)], &[], 2);
        assert_eq!((&text[o[0].0.clone()], &text[o[1].0.clone()]), ("Q:", " what "));
        assert_eq!(&text[overlays(&text, 0..text.len(), &[d(false), n("Next", true, 1, NestedUntil::Words)], &[], 2)[0].0.clone()], "Q: ");
        assert!(overlays(&text, 0..text.len(), &[d(true)], &[], 0).is_empty());
        // A bad pattern is ignored.
        assert!(overlays(&text, 0..text.len(), &[], &[GrepStyle { style: "X".into(), pattern: "(".into() }], 0).is_empty());
    }

    #[test]
    fn grep_style_in_the_documents_dialect() {
        // Short Russian words kept with the next one: `\l` lowercase letter, `\h` horizontal space.
        let text = "Мы и он в доме.\n".to_string();
        let o = overlays(&text, 0..text.len(), &[], &[GrepStyle { style: "nobreak".into(), pattern: r"\b\l{1,2}\h".into() }], 0);
        let got: Vec<&str> = o.iter().map(|(r, _)| &text[r.clone()]).collect();
        assert_eq!(got, ["и ", "он ", "в "]);
    }

    #[test]
    fn grep_style_with_lookbehind() {
        // Polish one-letter words kept with the next one; the space before is not styled.
        let text = "Ala i kot w domu.\n".to_string();
        let o = overlays(&text, 0..text.len(), &[], &[GrepStyle { style: "nobreak".into(), pattern: r"(?<=\s)[aiouwzAIOUWZ]\s".into() }], 0);
        let got: Vec<&str> = o.iter().map(|(r, _)| &text[r.clone()]).collect();
        assert_eq!(got, ["i ", "w "]);
    }
}
