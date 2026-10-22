//! The GREP dialect of page-layout documents (Perl-style regular expressions with letter-class
//! escapes and `~` metacharacters), compiled for our story encoding.
//!
//! [`translate`] rewrites the dialect into the syntax `fancy_regex` reads; [`Grep::new`] compiles
//! it. GREP styles, Find/Change and the style dialogs all go through here.
//!
//! - `\l` `\u` (and `\L` `\U`) are Unicode lowercase and uppercase letters, `\h` `\H` horizontal
//!   whitespace, `\v` `\V` vertical whitespace, `\R` any line break, `\N` anything but a
//!   paragraph end.
//! - `\r` is the paragraph end (`\n` in a story), `\n` the forced line break (U+2028).
//! - `~` codes name special characters (`~e` ellipsis, `~=` en dash, `~S` nonbreaking space…).
//! - POSIX classes (`[[:alpha:]]`) are Unicode-aware.
//! - Lookahead, lookbehind, atomic groups, possessive quantifiers, backreferences and `\K` are
//!   supported. Backtracking is capped ([`BACKTRACK_LIMIT`]), so a pathological pattern stops
//!   with an error instead of hanging.

use std::fmt;
use std::ops::Range;

use designcraft_doc::story;

/// Longest pattern accepted, in bytes.
pub const MAX_PATTERN: usize = 4096;
/// Backtracking steps one search may take before it gives up.
pub const BACKTRACK_LIMIT: usize = 200_000;
/// Compiled program size cap (bytes) for the underlying regex engine.
const SIZE_LIMIT: usize = 4 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepError(pub String);

impl fmt::Display for GrepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GrepError {}

impl From<GrepError> for String {
    fn from(e: GrepError) -> String {
        e.0
    }
}

fn err(msg: impl Into<String>) -> GrepError {
    GrepError(msg.into())
}

/// Characters a `~` code stands for. Several characters means "any of these".
fn tilde(c: char) -> Option<&'static [char]> {
    Some(match c {
        'b' => &['\n'],
        '#' => &[story::PAGE_NUMBER, story::NEXT_PAGE_NUMBER, story::PREV_PAGE_NUMBER],
        'N' => &[story::PAGE_NUMBER],
        'X' => &[story::NEXT_PAGE_NUMBER],
        'V' => &[story::PREV_PAGE_NUMBER],
        'x' => &[story::SECTION_MARKER],
        'M' => &[story::COLUMN_BREAK],
        'R' => &[story::FRAME_BREAK],
        'P' => &[story::PAGE_BREAK],
        'L' => &[story::ODD_PAGE_BREAK],
        'E' => &[story::EVEN_PAGE_BREAK],
        'i' => &[story::INDENT_HERE],
        'y' => &[story::RIGHT_INDENT_TAB],
        'n' => &[story::FORCED_LINE_BREAK],
        'k' => &['\u{200B}'],
        'j' => &['\u{200C}'],
        '=' => &['\u{2013}'],
        '_' => &['\u{2014}'],
        '-' => &['\u{AD}'],
        '~' => &['\u{2011}'],
        'S' => &['\u{A0}'],
        's' => &['\u{202F}'],
        'f' => &['\u{2001}'],
        'm' => &['\u{2003}'],
        '>' => &['\u{2002}'],
        '3' => &['\u{2004}'],
        '4' => &['\u{2005}'],
        '%' => &['\u{2006}'],
        '/' => &['\u{2007}'],
        '.' => &['\u{2008}'],
        '<' => &['\u{2009}'],
        '|' => &['\u{200A}'],
        'e' => &['\u{2026}'],
        '8' => &['\u{2022}'],
        '2' => &['\u{A9}'],
        'r' => &['\u{AE}'],
        'd' => &['\u{2122}'],
        '6' => &['\u{A7}'],
        '7' => &['\u{B6}'],
        '{' => &['\u{201C}'],
        '}' => &['\u{201D}'],
        '[' => &['\u{2018}'],
        ']' => &['\u{2019}'],
        '"' => &['"', '\u{201C}', '\u{201D}'],
        '\'' => &['\'', '\u{2018}', '\u{2019}'],
        _ => return None,
    })
}

const H_SPACE: &str = r"\t\p{Zs}";
const V_SPACE: &str = r"\n\x0B\f\r\x{85}\x{2028}\x{2029}";

/// Class members as they appear inside `[...]`, or a whole class / group outside one.
enum Piece {
    /// Valid both inside and outside a class as is.
    Atom(&'static str),
    /// Class members: wrapped in `[...]` outside a class, spliced in inside one.
    Members(&'static str),
    /// Negated members: `[^...]` (nested when inside a class).
    NotMembers(&'static str),
    /// Only valid outside a class.
    Outside(&'static str),
}

/// The translation of `\c`, or None to keep the escape as is.
fn escape(c: char) -> Option<Piece> {
    Some(match c {
        'l' => Piece::Atom(r"\p{Ll}"),
        'u' => Piece::Atom(r"\p{Lu}"),
        'L' => Piece::Atom(r"\P{Ll}"),
        'U' => Piece::Atom(r"\P{Lu}"),
        'h' => Piece::Members(H_SPACE),
        'H' => Piece::NotMembers(H_SPACE),
        'v' => Piece::Members(V_SPACE),
        'V' => Piece::NotMembers(V_SPACE),
        'r' => Piece::Atom(r"\n"),
        'n' => Piece::Atom(r"\x{2028}"),
        'N' => Piece::Outside(r"[^\n]"),
        'R' => Piece::Outside(r"(?:\r\n|[\n\x0B\f\r\x{85}\x{2028}\x{2029}])"),
        _ => return None,
    })
}

/// Members of a POSIX class `[:name:]` (Unicode-aware, unlike the regex crate's ASCII ones).
fn posix(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => r"\p{Alphabetic}",
        "lower" => r"\p{Ll}",
        "upper" => r"\p{Lu}",
        "digit" => r"\p{Nd}",
        "alnum" => r"\p{Alphabetic}\p{Nd}",
        "word" => r"\w",
        "space" => r"\s",
        "blank" => H_SPACE,
        "punct" => r"\p{P}",
        "cntrl" => r"\p{Cc}",
        "xdigit" => r"0-9A-Fa-f",
        "graph" => r"^\s\p{Cc}\p{Cn}",
        "print" => r"^\p{Cc}\p{Cn}",
        _ => return None,
    })
}

fn push_char(out: &mut String, c: char) {
    use std::fmt::Write as _;
    let _ = write!(out, r"\x{{{:X}}}", c as u32);
}

/// Rewrite a GREP-dialect pattern into `fancy_regex` syntax for our story encoding.
pub fn translate(pattern: &str) -> Result<String, GrepError> {
    if pattern.len() > MAX_PATTERN {
        return Err(err(format!("GREP pattern longer than {MAX_PATTERN} bytes")));
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut i = 0;
    let mut in_class = false;
    // The previous class member was a plain `-`: a second one would read as set difference.
    let mut dash = false;
    while let Some(&c) = chars.get(i) {
        i += 1;
        let was_dash = std::mem::take(&mut dash);
        match c {
            '\\' => {
                let Some(&n) = chars.get(i) else { return Err(err("GREP pattern ends with a backslash")) };
                i += 1;
                match escape(n) {
                    None => {
                        out.push('\\');
                        out.push(n);
                    }
                    Some(Piece::Atom(s)) => out.push_str(s),
                    Some(Piece::Members(s)) if in_class => out.push_str(s),
                    Some(Piece::Members(s)) => {
                        out.push('[');
                        out.push_str(s);
                        out.push(']');
                    }
                    Some(Piece::NotMembers(s)) => {
                        out.push_str("[^");
                        out.push_str(s);
                        out.push(']');
                    }
                    Some(Piece::Outside(_)) if in_class => {
                        return Err(err(format!(r"GREP: \{n} can't be used inside [...]")));
                    }
                    Some(Piece::Outside(s)) => out.push_str(s),
                }
            }
            '~' => match chars.get(i).and_then(|&n| tilde(n)) {
                Some(set) => {
                    i += 1;
                    let wrap = set.len() > 1 && !in_class;
                    if wrap {
                        out.push('[');
                    }
                    for &ch in set {
                        push_char(&mut out, ch);
                    }
                    if wrap {
                        out.push(']');
                    }
                }
                None => out.push_str(r"\~"),
            },
            '[' if !in_class => {
                in_class = true;
                out.push('[');
                if chars.get(i) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                // A `]` first in a class is a literal.
                if chars.get(i) == Some(&']') {
                    out.push_str(r"\]");
                    i += 1;
                }
            }
            '[' => {
                if chars.get(i) == Some(&':') {
                    let rest = chars.get(i + 1..).unwrap_or(&[]);
                    let close = rest.windows(2).position(|w| w == [':', ']']).ok_or_else(|| err("GREP: unclosed [:class:]"))?;
                    let raw: String = rest.get(..close).unwrap_or(&[]).iter().collect();
                    let (neg, name) = match raw.strip_prefix('^') {
                        Some(n) => (true, n),
                        None => (false, raw.as_str()),
                    };
                    let members = posix(name).ok_or_else(|| err(format!("GREP: unknown class [:{raw}:]")))?;
                    // Nested so negation and the `^` in graph/print stay local to this class.
                    out.push('[');
                    if neg {
                        out.push('^');
                        out.push('[');
                        out.push_str(members);
                        out.push(']');
                    } else {
                        out.push_str(members);
                    }
                    out.push(']');
                    i += 1 + close + 2;
                } else {
                    // A bare `[` inside a class is a literal in this dialect, a nested class in ours.
                    out.push_str(r"\[");
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '&' if in_class => out.push_str(r"\&"),
            '-' if in_class => {
                if was_dash {
                    out.push_str(r"\-");
                } else {
                    out.push('-');
                    dash = true;
                }
            }
            _ => out.push(c),
        }
    }
    Ok(out)
}

/// Rewrite a GREP change-to string into an expansion template: `$0`…`$9` are groups; `\r` `\n`
/// `\t`, `~` codes, any other `$` and `\` + any other character are literal text.
pub fn translate_replacement(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    let lit = |out: &mut String, c: char| {
        if c == '$' {
            out.push_str("$$");
        } else {
            out.push(c);
        }
    };
    while let Some(c) = it.next() {
        match c {
            '\\' => match it.next() {
                Some('r') => out.push('\n'),
                Some('n') => out.push(story::FORCED_LINE_BREAK),
                Some('t') => out.push('\t'),
                Some(n) => lit(&mut out, n),
                None => out.push('\\'),
            },
            '~' => match it.peek().and_then(|&n| tilde(n)).and_then(|set| set.first()) {
                Some(&ch) => {
                    it.next();
                    lit(&mut out, ch);
                }
                None => out.push('~'),
            },
            // `$1abc` is group 1 then "abc"; a `$` before anything but a digit is literal.
            '$' => match it.peek().copied().filter(char::is_ascii_digit) {
                Some(d) => {
                    it.next();
                    out.push_str("${");
                    out.push(d);
                    out.push('}');
                }
                None => out.push_str("$$"),
            },
            _ => out.push(c),
        }
    }
    out
}

/// Matching options.
#[derive(Debug, Clone, Copy, Default)]
pub struct GrepOptions {
    pub case_insensitive: bool,
    /// `^` and `$` match at paragraph starts and ends, not only at the ends of the text.
    pub multi_line: bool,
    pub whole_word: bool,
}

/// A compiled GREP pattern.
#[derive(Debug, Clone)]
pub struct Grep {
    re: fancy_regex::Regex,
}

impl Grep {
    pub fn new(pattern: &str, opts: GrepOptions) -> Result<Grep, GrepError> {
        Grep::build(translate(pattern)?, opts)
    }

    /// A pattern matching `text` literally.
    pub fn literal(text: &str, opts: GrepOptions) -> Result<Grep, GrepError> {
        if text.len() > MAX_PATTERN {
            return Err(err(format!("search text longer than {MAX_PATTERN} bytes")));
        }
        Grep::build(regex::escape(text), opts)
    }

    fn build(mut pat: String, opts: GrepOptions) -> Result<Grep, GrepError> {
        if opts.whole_word {
            pat = format!(r"\b(?:{pat})\b");
        }
        let flags = match (opts.case_insensitive, opts.multi_line) {
            (true, true) => "(?im)",
            (true, false) => "(?i)",
            (false, true) => "(?m)",
            (false, false) => "",
        };
        let re = fancy_regex::RegexBuilder::new(&format!("{flags}{pat}"))
            .backtrack_limit(BACKTRACK_LIMIT)
            .delegate_size_limit(SIZE_LIMIT)
            .delegate_dfa_size_limit(SIZE_LIMIT)
            .build()
            .map_err(|e| err(format!("bad GREP: {e}")))?;
        Ok(Grep { re })
    }

    /// The first match starting at or after byte `start`.
    pub fn find_at(&self, text: &str, start: usize) -> Result<Option<Range<usize>>, GrepError> {
        if start > text.len() || !text.is_char_boundary(start) {
            return Ok(None);
        }
        self.re.find_from_pos(text, start).map(|m| m.map(|m| m.range())).map_err(run_err)
    }

    /// The first `limit` non-overlapping matches in `text`.
    pub fn find_all(&self, text: &str, limit: usize) -> Result<Vec<Range<usize>>, GrepError> {
        self.re.find_iter(text).take(limit).map(|m| m.map(|m| m.range()).map_err(run_err)).collect()
    }

    /// Matches in `text` and their replacements, `template` expanded per match (`$1`…; see
    /// [`translate_replacement`]). Stops after the first match when `first_only`.
    pub fn replacements(&self, text: &str, template: &str, first_only: bool) -> Result<Vec<(Range<usize>, String)>, GrepError> {
        let exp = fancy_regex::Expander::default();
        let mut out = Vec::new();
        for caps in self.re.captures_iter(text) {
            let caps = caps.map_err(run_err)?;
            let Some(m) = caps.get(0) else { continue };
            out.push((m.range(), exp.expansion(template, &caps)));
            if first_only {
                break;
            }
        }
        Ok(out)
    }
}

fn run_err(e: fancy_regex::Error) -> GrepError {
    err(format!("GREP search stopped: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grep(p: &str) -> Grep {
        Grep::new(p, GrepOptions::default()).unwrap()
    }

    fn found<'a>(p: &str, text: &'a str) -> Vec<&'a str> {
        grep(p).find_all(text, usize::MAX).unwrap().into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn translates_dialect_tokens() {
        let t = |p: &str| translate(p).unwrap();
        assert_eq!(t(r"\l\u\L\U"), r"\p{Ll}\p{Lu}\P{Ll}\P{Lu}");
        assert_eq!(t(r"\h"), r"[\t\p{Zs}]");
        assert_eq!(t(r"[\h,]"), r"[\t\p{Zs},]");
        assert_eq!(t(r"\H"), r"[^\t\p{Zs}]");
        assert_eq!(t(r"\v"), r"[\n\x0B\f\r\x{85}\x{2028}\x{2029}]");
        assert_eq!(t(r"\r\n"), r"\n\x{2028}");
        assert_eq!(t(r"~e~=~_~S~<~>~m~%~/"), r"\x{2026}\x{2013}\x{2014}\x{A0}\x{2009}\x{2002}\x{2003}\x{2006}\x{2007}");
        assert_eq!(t(r#"~""#), r"[\x{22}\x{201C}\x{201D}]");
        assert_eq!(t("a~q"), r"a\~q", "an unknown ~ code is a literal tilde");
        // Escaped backslashes and other escapes stay put.
        assert_eq!(t(r"\\l\\h"), r"\\l\\h");
        assert_eq!(t(r"\d\w\s\b\<\>\x{41}\p{Lu}"), r"\d\w\s\b\<\>\x{41}\p{Lu}");
        // Character classes: leading `]`, bare `[`, `&&`, `--`, POSIX classes.
        assert_eq!(t(r"[]a]"), r"[\]a]");
        assert_eq!(t(r"[^]a]"), r"[^\]a]");
        assert_eq!(t(r"[a[b]"), r"[a\[b]");
        assert_eq!(t(r"[a&&b]"), r"[a\&\&b]");
        assert_eq!(t(r"[+--]"), r"[+-\-]");
        assert_eq!(t(r"[[:alpha:]]"), r"[[\p{Alphabetic}]]");
        assert_eq!(t(r"[[:^digit:]x]"), r"[[^[\p{Nd}]]x]");
        assert!(translate(r"[[:bogus:]]").is_err());
        assert!(translate(r"[\R]").is_err());
        assert!(translate("a\\").is_err());
        assert!(translate(&"a".repeat(MAX_PATTERN + 1)).is_err());
    }

    #[test]
    fn matches_like_the_dialect() {
        // Russian one- and two-letter words followed by a space (a no-break GREP style).
        assert_eq!(found(r"\b\l{1,2}\h", "Я и он в доме"), ["и ", "он ", "в "]);
        assert_eq!(found(r"\u\l+", "Hello World ok"), ["Hello", "World"]);
        assert_eq!(found(r"[[:upper:]]+", "abc ÉTÉ"), ["ÉTÉ"]);
        assert_eq!(found(r"[]x]+", "a]x]b"), ["]x]"]);
        assert_eq!(found(r"\d~=\d", "1\u{2013}2 3-4"), ["1\u{2013}2"]);
        assert_eq!(found(r"\<\w", "ab cd"), ["a", "c"]);
        assert_eq!(found(r"\\l", r"a\lb"), [r"\l"]);
        // Lookaround, atomic groups, possessive quantifiers, \K.
        assert_eq!(found(r"(?<=\d)\h(?=\l)", "5 kg 5 K"), [" "]);
        assert_eq!(found(r"(?>a+)b", "aab"), ["aab"]);
        assert_eq!(found(r"a++b", "aab"), ["aab"]);
        assert_eq!(found(r"\d+\K%", "50% off"), ["%"]);
    }

    #[test]
    fn options_and_replacements() {
        let g = Grep::new(r"\u\l+", GrepOptions { case_insensitive: false, multi_line: true, whole_word: true }).unwrap();
        assert_eq!(g.find_at("Ab Cd", 1).unwrap(), Some(3..5));
        assert_eq!(g.find_at("Ab Cd", 99).unwrap(), None);
        let g = Grep::literal("a.b", GrepOptions { case_insensitive: true, ..Default::default() }).unwrap();
        assert_eq!(g.find_all("axb A.B a.b", 1).unwrap().first(), Some(&(4..7)));
        let g = grep(r"(\d+)\h(\l+)");
        let tpl = translate_replacement(r"$2~S$1x\r\$$");
        assert_eq!(g.replacements("5 kg", &tpl, false).unwrap(), [(0..4, "kg\u{A0}5x\n$$".to_string())]);
    }

    #[test]
    fn catastrophic_backtracking_stops() {
        // Exponential in the backtracking engine (the backreference forces it): without the limit
        // this runs for hours.
        let g = grep(r"(a|aa)+\1x");
        let text = "a".repeat(64);
        let t = std::time::Instant::now();
        assert!(g.find_all(&text, usize::MAX).is_err());
        assert!(t.elapsed().as_secs() < 5);
    }
}
