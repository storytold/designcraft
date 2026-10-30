//! Adobe PDF settings (`.joboptions`), read as data.
//!
//! The format is PostScript-like: `<< /Key value … >> setdistillerparams`, with names (`/Name`),
//! numbers, `true`/`false`, strings in brackets (`(text)`, with `\050` octal escapes and nested
//! brackets), arrays (`[ … ]`) and nested dicts. Adobe files hold the real settings in the first
//! `<< … >>` block; blocks after it (page device settings) are ignored. [`parse`] never panics:
//! bad input is an error.
//!
//! [`to_preset`] turns the parsed data into Export PDF fields.

use std::collections::BTreeMap;

mod preset;
pub use preset::{ImportedPreset, to_preset};

/// Files larger than this are refused: a real settings file is under 10 KB.
pub const MAX_BYTES: usize = 1_000_000;
/// Deepest nesting of dicts and arrays that is read.
const MAX_DEPTH: usize = 32;

/// One value in a settings block.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `/Name`, without the slash. Bare words that are not numbers or booleans also land here.
    Name(String),
    Num(f64),
    Bool(bool),
    /// `(text)` with escapes resolved.
    Str(String),
    Array(Vec<Value>),
    Dict(BTreeMap<String, Value>),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum JobOptionsError {
    #[error("the file is larger than 1 MB, so it is not a PDF settings file")]
    TooLarge,
    #[error("no settings block (<< … >>) was found, so this is not a .joboptions file")]
    NoDict,
    #[error("the settings are cut off or malformed near byte {0}")]
    Malformed(usize),
}

/// The settings block of a `.joboptions` file.
#[derive(Clone, Debug, PartialEq)]
pub struct JobOptions {
    root: BTreeMap<String, Value>,
}

impl JobOptions {
    /// The value of `key`: at the top level if it is there, else the first match inside a nested
    /// dict or array (InDesign keeps bleed, marks and tagging in a dict inside `/OtherNamespaces`).
    pub fn get(&self, key: &str) -> Option<&Value> {
        find(&self.root, key)
    }

    pub fn flag(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        match self.get(key)? {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// A `/Name` value, without the slash.
    pub fn name(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Name(n) => Some(n),
            _ => None,
        }
    }

    /// A `(string)` value.
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The names in an array value (`/CheckCompliance [ /PDFX3:2002 ]`); empty if `key` is not one.
    pub fn names(&self, key: &str) -> Vec<&str> {
        match self.get(key) {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|v| match v {
                    Value::Name(n) => Some(n.as_str()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

fn find<'a>(map: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a Value> {
    map.get(key).or_else(|| map.values().find_map(|v| find_in(v, key)))
}

fn find_in<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Dict(map) => find(map, key),
        Value::Array(items) => items.iter().find_map(|v| find_in(v, key)),
        _ => None,
    }
}

/// Read the first `<< … >>` block of `text`.
pub fn parse(text: &str) -> Result<JobOptions, JobOptionsError> {
    if text.len() > MAX_BYTES {
        return Err(JobOptionsError::TooLarge);
    }
    let mut parser = Parser { lex: Lexer { bytes: text.as_bytes(), at: 0 } };
    loop {
        match parser.lex.token()? {
            None => return Err(JobOptionsError::NoDict),
            Some((at, Tok::Open)) => return Ok(JobOptions { root: parser.dict(1, at)? }),
            Some(_) => {}
        }
    }
}

/// Read the bytes of a settings file. A file with a UTF-16 byte order mark (`FF FE` or `FE FF`)
/// is decoded as UTF-16; anything else is read as UTF-8, with bad bytes replaced.
pub fn parse_bytes(bytes: &[u8]) -> Result<JobOptions, JobOptionsError> {
    if bytes.len() > MAX_BYTES {
        return Err(JobOptionsError::TooLarge);
    }
    let utf16 = |rest: &[u8], unit: fn([u8; 2]) -> u16| {
        let units = rest.as_chunks::<2>().0.iter().map(|pair| unit(*pair));
        char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect::<String>()
    };
    let text = match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    };
    parse(&text)
}

#[derive(Debug)]
enum Tok {
    Open,
    Close,
    ArrOpen,
    ArrClose,
    Name(String),
    Str(String),
    Word(String),
}

fn is_delim(c: u8) -> bool {
    c.is_ascii_whitespace() || matches!(c, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'/' | b'%')
}

struct Lexer<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Lexer<'_> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.at + ahead).copied()
    }

    /// The next token and the byte it starts at; `None` at the end.
    fn token(&mut self) -> Result<Option<(usize, Tok)>, JobOptionsError> {
        let c = loop {
            match self.peek(0) {
                None => return Ok(None),
                Some(c) if c.is_ascii_whitespace() => self.at += 1,
                Some(b'%') => {
                    while self.peek(0).is_some_and(|c| c != b'\n' && c != b'\r') {
                        self.at += 1;
                    }
                }
                Some(c) => break c,
            }
        };
        let start = self.at;
        let tok = match c {
            b'<' if self.peek(1) == Some(b'<') => {
                self.at += 2;
                Tok::Open
            }
            b'>' if self.peek(1) == Some(b'>') => {
                self.at += 2;
                Tok::Close
            }
            b'<' => Tok::Str(self.hex_string(start)?),
            b'[' => {
                self.at += 1;
                Tok::ArrOpen
            }
            b']' => {
                self.at += 1;
                Tok::ArrClose
            }
            b'(' => Tok::Str(self.string(start)?),
            b'/' => {
                self.at += 1;
                Tok::Name(self.run(self.at))
            }
            _ => {
                // Always takes the first byte, so a stray delimiter cannot stall the lexer.
                self.at += 1;
                Tok::Word(self.run(start))
            }
        };
        Ok(Some((start, tok)))
    }

    /// Consume non-delimiter bytes and return the text from `from`.
    fn run(&mut self, from: usize) -> String {
        while self.peek(0).is_some_and(|c| !is_delim(c)) {
            self.at += 1;
        }
        String::from_utf8_lossy(self.bytes.get(from..self.at).unwrap_or_default()).into_owned()
    }

    /// A `<hex>` string. Adobe writes its localised descriptions as UTF-16BE (`<FEFF…>`); other
    /// hex is read as UTF-8. Characters that are not hex digits are skipped.
    fn hex_string(&mut self, start: usize) -> Result<String, JobOptionsError> {
        self.at += 1;
        let mut bytes: Vec<u8> = Vec::new();
        let mut high: Option<u8> = None;
        loop {
            let Some(c) = self.peek(0) else { return Err(JobOptionsError::Malformed(start)) };
            self.at += 1;
            if c == b'>' {
                break;
            }
            let Some(digit) = char::from(c).to_digit(16).and_then(|d| u8::try_from(d).ok()) else { continue };
            match high.take() {
                Some(h) => bytes.push(h << 4 | digit),
                None => high = Some(digit),
            }
        }
        if let Some(h) = high {
            bytes.push(h << 4);
        }
        Ok(match bytes.strip_prefix(&[0xFE, 0xFF]) {
            Some(utf16) => {
                let units = utf16.chunks(2).map(|pair| match pair {
                    [a, b] => u16::from_be_bytes([*a, *b]),
                    [a] => u16::from_be_bytes([*a, 0]),
                    _ => 0,
                });
                char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
            }
            None => String::from_utf8_lossy(&bytes).into_owned(),
        })
    }

    /// A `(string)`: nested brackets balance, `\ddd` is an octal byte, `\x` is `x`.
    fn string(&mut self, start: usize) -> Result<String, JobOptionsError> {
        self.at += 1;
        let mut depth = 1usize;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek(0) else { return Err(JobOptionsError::Malformed(start)) };
            self.at += 1;
            match c {
                b'\\' => {
                    let Some(e) = self.peek(0) else { return Err(JobOptionsError::Malformed(start)) };
                    self.at += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'0'..=b'7' => {
                            let mut v = u32::from(e - b'0');
                            for _ in 0..2 {
                                match self.peek(0) {
                                    Some(d @ b'0'..=b'7') => {
                                        v = v * 8 + u32::from(d - b'0');
                                        self.at += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(u8::try_from(v & 0xff).unwrap_or(b'?'));
                        }
                        other => out.push(other),
                    }
                }
                b'(' => {
                    depth += 1;
                    out.push(c);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

struct Parser<'a> {
    lex: Lexer<'a>,
}

impl Parser<'_> {
    /// The `key value` pairs up to the closing `>>`; `open_at` is where the `<<` was.
    fn dict(&mut self, depth: usize, open_at: usize) -> Result<BTreeMap<String, Value>, JobOptionsError> {
        let mut map = BTreeMap::new();
        loop {
            match self.lex.token()? {
                None => return Err(JobOptionsError::Malformed(open_at)),
                Some((_, Tok::Close)) => return Ok(map),
                Some((_, Tok::Name(key))) => {
                    let value = self.value(depth)?;
                    map.insert(key, value);
                }
                Some((at, _)) => return Err(JobOptionsError::Malformed(at)),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, JobOptionsError> {
        match self.lex.token()? {
            None => Err(JobOptionsError::Malformed(self.lex.at)),
            Some((at, tok)) => self.value_from(depth, at, tok),
        }
    }

    fn value_from(&mut self, depth: usize, at: usize, tok: Tok) -> Result<Value, JobOptionsError> {
        if depth > MAX_DEPTH {
            return Err(JobOptionsError::Malformed(at));
        }
        match tok {
            Tok::Open => Ok(Value::Dict(self.dict(depth + 1, at)?)),
            Tok::ArrOpen => {
                let mut items = Vec::new();
                loop {
                    match self.lex.token()? {
                        None => return Err(JobOptionsError::Malformed(at)),
                        Some((_, Tok::ArrClose)) => return Ok(Value::Array(items)),
                        Some((inner, tok)) => items.push(self.value_from(depth + 1, inner, tok)?),
                    }
                }
            }
            Tok::Name(n) => Ok(Value::Name(n)),
            Tok::Str(s) => Ok(Value::Str(s)),
            Tok::Word(w) => Ok(match w.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ if w.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.')) => {
                    w.parse::<f64>().map_or(Value::Name(w), Value::Num)
                }
                _ => Value::Name(w),
            }),
            Tok::Close | Tok::ArrClose => Err(JobOptionsError::Malformed(at)),
        }
    }
}

#[cfg(test)]
mod tests;
