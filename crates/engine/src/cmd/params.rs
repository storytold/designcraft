//! The parameter keys a command accepts, read from its params description (#776).
//!
//! A description starts with the params object: `{x, width?: pt, name?|title?: …, sides=6, …}`, or
//! with the objects a command takes instead of one another: `{path} or {name, dataBase64}`,
//! `{name} | {names: […]}`. Each entry of an object starts with a key, or with alternatives split
//! by `|`; a `?` marks an optional one, and a `:`, `=` or `(` starts what the entry says about it.
//! An entry that starts with `…` stands for keys listed elsewhere: those of each command it names
//! after `as` or `of` (`…settings as document.new`), or, naming none, any key. Objects inside a
//! value aren't described key by key, so only the top level is checked.
//!
//! Callers outside the app (the control channel, MCP, the CLI) are answered with a parameter error
//! for a key the description doesn't name ([`check_params`]): a misspelt `widht` no longer runs the
//! command with its default. [`Session::execute`](crate::Session::execute) itself takes what it is
//! given, so the journal of an older version still replays.
//!
//! Ported from VectorCraft (storytold/vectorcraft#1442).

use serde_json::Value;

use super::{bad, find_command};
use crate::Result;

/// What a params description says about the keys of the params object.
#[derive(Debug, Default, PartialEq)]
pub struct ParamKeys<'a> {
    /// The keys it names.
    pub keys: Vec<&'a str>,
    /// Its `…` entries, without the `…`.
    pub more: Vec<&'a str>,
}

/// The text between the braces of the object `doc` starts with and what follows it, or why it
/// has none.
fn object_body(doc: &str) -> std::result::Result<(&str, &str), String> {
    // (Named, not written in the `Err`: these are for the tests of the descriptions, not messages
    // the app shows, which the message catalogs would have to translate.)
    let (no_object, not_closed) = ("the description doesn't start with the params object `{…}`", "the params object `{…}` isn't closed");
    if !doc.starts_with('{') {
        return Err(no_object.into());
    }
    let (mut depth, mut quote) = (0usize, None);
    for (i, c) in doc.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '`') => quote = Some(c),
            (None, '{' | '[' | '(') => depth += 1,
            (None, '}' | ']' | ')') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok((doc.get(1..i).unwrap_or_default(), doc.get(i + 1..).unwrap_or_default()));
                }
            }
            _ => {}
        }
    }
    Err(not_closed.into())
}

/// `s` split at each `sep` that is outside brackets, quotes and backticks.
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let (mut out, mut depth, mut quote, mut start) = (Vec::new(), 0usize, None, 0);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '`') => quote = Some(c),
            (None, '{' | '[' | '(') => depth += 1,
            (None, '}' | ']' | ')') => depth = depth.saturating_sub(1),
            (None, _) if c == sep && depth == 0 => {
                out.push(s.get(start..i).unwrap_or_default());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(s.get(start..).unwrap_or_default());
    out
}

fn is_key(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The keys a params description names, or what in it doesn't read as one.
pub fn doc_keys(doc: &str) -> std::result::Result<ParamKeys<'_>, String> {
    let mut out = ParamKeys::default();
    let (mut body, mut rest) = object_body(doc)?;
    let mut entries = split_top(body, ',');
    // `{…} or {…}`, `{…} | {…}`: the objects the command takes instead.
    while let Some(next) = rest.trim_start().strip_prefix("or ").or_else(|| rest.trim_start().strip_prefix("| ")).filter(|r| r.starts_with('{')) {
        (body, rest) = object_body(next)?;
        entries.extend(split_top(body, ','));
    }
    for entry in entries.into_iter().map(str::trim).filter(|e| !e.is_empty()) {
        if let Some(more) = entry.strip_prefix('…') {
            out.more.push(more.trim());
            continue;
        }
        // `a?|b?: note` names two keys, and so does `a?: note | b?: note`; in `a?: x|y` the `y`
        // is a value.
        let mut noted = false;
        for alternative in split_top(entry, '|') {
            let head = alternative.split([':', '=', '(']).next().unwrap_or_default();
            let rest = alternative.get(head.len()..).unwrap_or_default();
            let name = head.trim().trim_end_matches('?').trim_end();
            if !noted {
                if !is_key(name) {
                    let start: String = entry.chars().take(60).collect();
                    let why = format!("`{start}` doesn't start with a parameter name");
                    return Err(why);
                }
                out.keys.push(name);
                noted = !rest.is_empty();
            } else if is_key(name) && (head.trim_end().ends_with('?') || rest.starts_with(':')) {
                out.keys.push(name);
            }
        }
    }
    Ok(out)
}

/// The commands whose keys a `…` entry stands for: those it names after `as` or `of` (`settings as
/// file.new`, `the PDF options of document.exportPdf`), not one it only points to (`see …`).
fn named_commands(more: &str) -> impl Iterator<Item = &'static super::CommandSpec> + '_ {
    let words: Vec<&str> = more.split_whitespace().collect();
    let id = |w: &str| find_command(w.trim_matches(|c: char| !c.is_ascii_alphanumeric()));
    (1..words.len()).filter_map(move |i| id(words.get(i)?).filter(|_| matches!(words.get(i - 1), Some(&"as" | &"of"))))
}

/// Every key command `doc` accepts, the keys of the commands its `…` entries name included.
/// `None`: any key (a `…` entry naming no command, or a description that doesn't parse, which
/// the tests rule out).
fn accepted(doc: &'static str, depth: usize) -> Option<Vec<&'static str>> {
    let parsed = doc_keys(doc).ok()?;
    let mut keys = parsed.keys;
    // A command that acts on `ids` takes one object as `id` too (see `cmd::targets`).
    if keys.contains(&"ids") && !keys.contains(&"id") {
        keys.push("id");
    }
    for more in parsed.more {
        let mut named = named_commands(more).peekable();
        // A reference that leads back here would never end: descriptions refer one or two deep.
        if named.peek().is_none() || depth > 4 {
            return None;
        }
        for spec in named {
            // A command that isn't checked yet doesn't vouch for the ones that take its keys.
            if UNCHECKED.contains(&spec.id) {
                return None;
            }
            keys.extend(accepted(spec.params, depth + 1)?);
        }
    }
    Some(keys)
}

/// The edits (insert, delete, replace, swap of neighbours) that turn `a` into `b`, ignoring case.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().map(|c| c.to_ascii_lowercase()).collect(), b.chars().map(|c| c.to_ascii_lowercase()).collect());
    let (mut before, mut prev): (Vec<usize>, Vec<usize>) = (Vec::new(), (0..=b.len()).collect());
    for (i, ca) in a.iter().enumerate() {
        let mut row = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let at = |r: &[usize], k: usize| r.get(k).copied().unwrap_or(usize::MAX / 2);
            let mut d = (at(&prev, j) + usize::from(ca != cb)).min(at(&prev, j + 1) + 1).min(at(&row, j) + 1);
            if i > 0 && j > 0 && a.get(i - 1) == Some(cb) && b.get(j - 1) == Some(ca) {
                d = d.min(at(&before, j - 1) + 1);
            }
            row.push(d);
        }
        before = std::mem::replace(&mut prev, row);
    }
    prev.last().copied().unwrap_or(0)
}

/// The key of `keys` a misspelt `key` most likely meant: the nearest one, when it is near.
fn nearest<'a>(key: &str, keys: &[&'a str]) -> Option<&'a str> {
    let near = keys.iter().map(|k| (distance(key, k), *k)).min()?;
    (near.0 <= (key.chars().count() / 3).max(1)).then_some(near.1)
}

/// `params` has only keys the description `doc` of command `id` names, or the parameter error
/// naming the first one it doesn't, with the nearest accepted key or the accepted ones.
pub fn check_keys(id: &str, doc: &'static str, params: &Value) -> Result<()> {
    let Value::Object(given) = params else { return Ok(()) };
    if given.is_empty() {
        return Ok(());
    }
    let Some(keys) = accepted(doc, 0) else { return Ok(()) };
    match given.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(key) => Err(unknown(id, key, &keys)),
        None => Ok(()),
    }
}

/// The parameter error of command `id` for `key`, which isn't one of `keys`: with the nearest of
/// them, or all of them.
fn unknown(id: &str, key: &str, keys: &[&str]) -> crate::EngineError {
    bad(
        id,
        match nearest(key, keys) {
            Some(near) => format!("unknown parameter `{key}`; did you mean `{near}`?"),
            None if keys.is_empty() => format!("unknown parameter `{key}`; the command takes no parameters"),
            None => format!("unknown parameter `{key}`; the command takes: {}", keys.join(", ")),
        },
    )
}

/// Commands whose params aren't checked yet (#776): they read keys in ways the audit of the
/// descriptions couldn't follow (a list of keys in a loop, a whole object read as settings), so
/// their descriptions aren't proven to name every key they take. Sorted; take a command out once
/// its description is checked against its code.
pub const UNCHECKED: &[&str] = &[
    // Take the attributes under `attrs` or as the params object itself; the attribute setters
    // refuse a key they don't know.
    "type.char",
    "type.para",
];

/// [`check_keys`] for an engine command. A command the engine doesn't have passes: running it
/// reports that.
pub fn check_params(id: &str, params: &Value) -> Result<()> {
    let Some(spec) = find_command(id) else { return Ok(()) };
    if UNCHECKED.contains(&id) {
        return Ok(());
    }
    check_keys(id, spec.params, params)
}
