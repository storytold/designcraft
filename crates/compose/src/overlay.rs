//! Character styles that paragraphs apply by themselves: nested styles (from the paragraph start
//! through or up to a delimiter) and GREP styles (every match of a pattern).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use designcraft_doc::{GrepStyle, NestedStyle, NestedUntil};

use crate::grep::{Grep, GrepOptions};

/// Patterns kept compiled at once. A document's GREP styles are few; the cache grew without
/// bound across every pattern a session ever composed (#1598).
const MAX_CACHED_PATTERNS: usize = 64;

thread_local! {
    /// Compiled GREP styles by pattern, with the tick they were last used at; `None` for one that
    /// doesn't compile or gave up on a search. Bounded to [`MAX_CACHED_PATTERNS`], least recently
    /// used evicted first.
    static PATTERNS: RefCell<HashMap<String, (Option<Grep>, u64)>> = RefCell::new(HashMap::new());
    /// Counts cache uses, so the least recently used entry can be found.
    static TICK: RefCell<u64> = const { RefCell::new(0) };
}

/// The compiled pattern for `pattern`, compiling it on first use and evicting the least recently
/// used entry once the cache is full. `f` is given `None` when the pattern does not compile.
fn with_pattern<R>(pattern: &str, f: impl FnOnce(&mut Option<Grep>) -> R) -> R {
    let now = TICK.with(|t| {
        let mut t = t.borrow_mut();
        *t += 1;
        *t
    });
    PATTERNS.with(|p| {
        let mut p = p.borrow_mut();
        if !p.contains_key(pattern)
            && p.len() >= MAX_CACHED_PATTERNS
            && let Some(oldest) = p.iter().min_by_key(|(_, (_, used))| *used).map(|(k, _)| k.clone())
        {
            p.remove(&oldest);
        }
        let slot = p.entry(pattern.to_string()).or_insert_with(|| {
            let re = Grep::new(pattern, GrepOptions::default()).map_err(|e| log::warn!("GREP style {pattern:?} skipped: {e}")).ok();
            (re, now)
        });
        slot.1 = now;
        f(&mut slot.0)
    })
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
        with_pattern(&g.pattern, |slot| {
            let Some(re) = slot else { return };
            match re.find_nonempty(text.get(range.start..end).unwrap_or(""), crate::grep::MAX_MATCHES_PER_PARAGRAPH) {
                Ok(ms) => {
                    if ms.len() == crate::grep::MAX_MATCHES_PER_PARAGRAPH {
                        log::warn!("GREP style {:?} matched more than {} times in one paragraph; the rest are not styled", g.pattern, ms.len());
                    }
                    out.extend(ms.into_iter().filter(|m| !m.is_empty()).map(|m| (range.start + m.start..range.start + m.end, g.style.clone())));
                }
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

    /// A pattern that can match nothing still styles a paragraph longer than the cap: empty
    /// matches don't count toward it.
    #[test]
    fn empty_matches_do_not_use_up_the_grep_cap() {
        let text = format!("{}a", "b".repeat(crate::grep::MAX_MATCHES_PER_PARAGRAPH + 100));
        let o = overlays(&text, 0..text.len(), &[], &[GrepStyle { style: "A".into(), pattern: "a*".into() }], 0);
        assert_eq!(o.len(), 1, "the one non-empty match at the end is styled");
    }

    /// A GREP style's scan is bounded two ways (#1598): the matches one paragraph can take, and
    /// the patterns kept compiled at once.
    #[test]
    fn a_grep_scan_and_the_pattern_cache_are_bounded() {
        // More matches in one paragraph than the cap: the scan stops with what it has instead of
        // paying the backtrack limit again for each one.
        let text = "a ".repeat(crate::grep::MAX_MATCHES_PER_PARAGRAPH + 500);
        let o = overlays(&text, 0..text.len(), &[], &[GrepStyle { style: "A".into(), pattern: "a".into() }], 0);
        assert_eq!(o.len(), crate::grep::MAX_MATCHES_PER_PARAGRAPH, "the scan stops at the cap");

        // The cache holds at most MAX_CACHED_PATTERNS, and the one just used is not the one
        // dropped: a document's own styles stay compiled however many patterns a session saw.
        PATTERNS.with(|p| p.borrow_mut().clear());
        for i in 0..MAX_CACHED_PATTERNS + 20 {
            let pattern = format!("x{i}y");
            overlays("x1y", 0..3, &[], &[GrepStyle { style: "S".into(), pattern }], 0);
        }
        let (len, has_last) = PATTERNS.with(|p| {
            let p = p.borrow();
            (p.len(), p.contains_key(&format!("x{}y", MAX_CACHED_PATTERNS + 19)))
        });
        assert_eq!(len, MAX_CACHED_PATTERNS, "the cache is bounded");
        assert!(has_last, "the most recently used pattern is still cached");
    }

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
