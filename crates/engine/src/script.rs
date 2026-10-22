//! Command scripts for agents and shells: a list of `{command, params}` steps where later steps
//! can use earlier results.
//!
//! **References.** A string parameter that is exactly `"$N.path"` (or `"$last.path"`, `"$N"`)
//! becomes the JSON value at `path` in the result of step `N` (0-based): after
//! `frame.create → {"id": 8, "story": 9}`, `{"story": "$0.story"}` passes `9`. Paths are dotted
//! keys and array indices (`$2.ids.0`). Inside longer strings `${N.path}` is replaced by the
//! value's text (`"Frame ${0.id}"`). Any other string starting with `$` is plain text (`"$1-$2"`,
//! `"$5 off"`), and `$$` anywhere writes a literal `$` (`"$$1"` passes `$1`, a GREP group).
//!
//! **Script text** ([`parse`]): a JSON array of steps, JSON lines (`{"command": …, "params": …}`),
//! or compact lines `command.id {json}` / `command.id={json}` / `command.id`; blank lines and
//! lines starting with `#` or `//` are ignored. A leading byte order mark and CRLF line endings are
//! accepted.

use serde_json::{Value, json};

/// One step of a script.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub command: String,
    pub params: Value,
}

/// Parse script text (see the module docs).
pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    // Windows tools save "UTF-8 with BOM"; `trim` keeps U+FEFF, which then became part of the
    // first command id.
    let t = text.trim_start_matches('\u{feff}').trim();
    if t.starts_with('[') {
        let v: Value = serde_json::from_str(t).map_err(|e| format!("script: {e}"))?;
        return v
            .as_array()
            .ok_or("script: expected an array")?
            .iter()
            .enumerate()
            .map(|(i, s)| step_of(s).map_err(|e| format!("step {i}: {e}")))
            .collect();
    }
    let mut out = Vec::new();
    for (n, line) in t.lines().enumerate() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with("//") {
            continue;
        }
        let step = if l.starts_with('{') {
            serde_json::from_str::<Value>(l).map_err(|e| e.to_string()).and_then(|v| step_of(&v))
        } else {
            // `id {json}`, `id={json}` or `id`.
            let split = l.find([' ', '=', '\t']).unwrap_or(l.len());
            let (id, rest) = l.split_at(split);
            let rest = rest.trim_start_matches(['=', ' ', '\t']).trim();
            let params = if rest.is_empty() { Ok(json!({})) } else { serde_json::from_str(rest).map_err(|e| e.to_string()) };
            params.map(|params| Step { command: id.to_string(), params })
        };
        out.push(step.map_err(|e| format!("line {}: {e}", n + 1))?);
    }
    Ok(out)
}

fn step_of(v: &Value) -> Result<Step, String> {
    let command = v.get("command").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("missing `command`")?.to_string();
    let params = v.get("params").cloned().filter(|p| !p.is_null()).unwrap_or(json!({}));
    if !params.is_object() {
        return Err("`params` must be an object".into());
    }
    Ok(Step { command, params })
}

/// The value a reference (`N.path`, `last.path`) points at.
fn lookup(r: &str, results: &[Value]) -> Result<Value, String> {
    let mut parts = r.split('.');
    let head = parts.next().unwrap_or("");
    let idx = if head == "last" {
        results.len().checked_sub(1).ok_or("`$last` before any step")?
    } else {
        head.parse::<usize>().map_err(|_| format!("bad reference `${r}`"))?
    };
    let mut v = results.get(idx).ok_or_else(|| format!("`${r}`: step {idx} hasn't run (only {} have)", results.len()))?.clone();
    for p in parts {
        v = match (&v, p.parse::<usize>()) {
            (Value::Array(a), Ok(i)) => a.get(i).cloned(),
            (Value::Object(o), _) => o.get(p).cloned(),
            _ => None,
        }
        .ok_or_else(|| format!("`${r}`: no `{p}` in {}", short(&v)))?;
    }
    Ok(v)
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 120 { format!("{}…", &s[..s.floor_char_boundary(120)]) } else { s }
}

/// Whether `r` (the text after `$`) is a whole reference: digits or `last`, then zero or more
/// `.segment` of ASCII letters, digits, `_` or `-`.
fn is_reference(r: &str) -> bool {
    let mut parts = r.split('.');
    let head = parts.next().unwrap_or("");
    let head_ok = head == "last" || (!head.is_empty() && head.bytes().all(|b| b.is_ascii_digit()));
    head_ok && parts.all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}

/// Replace references in `params` with values from earlier `results`.
pub fn resolve(params: &Value, results: &[Value]) -> Result<Value, String> {
    Ok(match params {
        Value::String(s) => {
            if let Some(r) = s.strip_prefix('$').filter(|r| is_reference(r)) {
                return lookup(r, results);
            }
            if !s.contains("${") && !s.contains("$$") {
                return Ok(params.clone());
            }
            let mut out = String::new();
            let mut rest = s.as_str();
            while let Some(i) = rest.find('$') {
                out.push_str(&rest[..i]);
                let after = &rest[i + 1..];
                if let Some(tail) = after.strip_prefix('$') {
                    out.push('$');
                    rest = tail;
                } else if let Some(inner) = after.strip_prefix('{') {
                    let end = inner.find('}').ok_or_else(|| format!("unclosed `${{` in {s:?}"))?;
                    match lookup(&inner[..end], results)? {
                        Value::String(t) => out.push_str(&t),
                        other => out.push_str(&other.to_string()),
                    }
                    rest = &inner[end + 1..];
                } else {
                    out.push('$');
                    rest = after;
                }
            }
            out.push_str(rest);
            Value::String(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|v| resolve(v, results)).collect::<Result<_, _>>()?),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| Ok((k.clone(), resolve(v, results)?))).collect::<Result<_, String>>()?),
        v => v.clone(),
    })
}

/// Outcome of running a script.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub results: Vec<Value>,
    /// The failing step (index, command, message); later steps didn't run.
    pub failed: Option<(usize, String, String)>,
}

impl Report {
    pub fn to_json(&self) -> Value {
        let mut v = json!({"completed": self.results.len(), "results": self.results});
        if let Some((i, c, e)) = &self.failed {
            v["failedIndex"] = json!(i);
            v["failedCommand"] = json!(c);
            v["error"] = json!(e);
        }
        v
    }
}

/// Run `steps` through `exec`, resolving references; stops at the first error.
pub fn run(steps: &[Step], mut exec: impl FnMut(&str, Value) -> Result<Value, String>) -> Report {
    let mut results = Vec::with_capacity(steps.len());
    for (i, s) in steps.iter().enumerate() {
        let r = resolve(&s.params, &results).and_then(|p| exec(&s.command, p));
        match r {
            Ok(v) => results.push(v),
            Err(e) => return Report { results, failed: Some((i, s.command.clone(), e)) },
        }
    }
    Report { results, failed: None }
}

impl crate::Session {
    /// Run a script on this session (see [`crate::script`]).
    pub fn run_script(&mut self, steps: &[Step]) -> Report {
        run(steps, |c, p| self.execute(c, &p).map_err(|e| e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_forms() {
        let a = parse(r#"[{"command": "file.new"}, {"command": "frame.create", "params": {"rect": [0, 0, 10, 10]}}]"#).unwrap();
        let b = parse("# comment\nfile.new\n\nframe.create {\"rect\": [0, 0, 10, 10]}\n").unwrap();
        let c = parse("{\"command\": \"file.new\"}\nframe.create={\"rect\": [0, 0, 10, 10]}").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert!(parse("frame.create {bad").unwrap_err().contains("line 1"));
    }

    /// Windows editors and PowerShell 5.1 save "UTF-8 with BOM" with CRLF line endings; the BOM
    /// became part of the first command id (#654).
    #[test]
    fn a_leading_bom_and_crlf_parse_like_plain_text() {
        let plain = "file.new {\"width\": 100}\nframe.create {\"rect\": [0, 0, 10, 10]}\n";
        let windows = format!("\u{feff}{}", plain.replace('\n', "\r\n"));
        assert_eq!(parse(&windows).unwrap(), parse(plain).unwrap());
        let array = r#"[{"command": "file.new"}]"#;
        assert_eq!(parse(&format!("\u{feff}{array}")).unwrap(), parse(array).unwrap());
    }

    #[test]
    fn resolves_references() {
        let results = vec![json!({"id": 8, "story": 9, "ids": [4, 5]}), json!({"name": "A"})];
        let p = json!({"story": "$0.story", "id": "$0.ids.1", "label": "Frame ${0.id} of ${last.name}", "keep": "$", "list": ["$1.name"]});
        let r = resolve(&p, &results).unwrap();
        assert_eq!(r, json!({"story": 9, "id": 5, "label": "Frame 8 of A", "keep": "$", "list": ["A"]}));
        assert!(resolve(&json!({"x": "$5.id"}), &results).unwrap_err().contains("hasn't run"));
        assert!(resolve(&json!({"x": "$0.nope"}), &results).unwrap_err().contains("no `nope`"));
        assert!(resolve(&json!({"x": "Frame ${0.id"}), &results).unwrap_err().contains("unclosed"));
    }

    /// GREP replacements start with `$` too; only a well-formed reference is looked up (#749).
    #[test]
    fn only_well_formed_references_resolve() {
        let results = vec![json!({"id": 8})];
        let p = json!({"a": "$1\u{a0}", "b": "$1-$2", "c": "$5 off", "d": "$$1", "e": "$${0.id}", "f": "$0.id", "g": "${0.id}x", "h": "$1$$", "i": "$1."});
        let r = resolve(&p, &results).unwrap();
        assert_eq!(r, json!({"a": "$1\u{a0}", "b": "$1-$2", "c": "$5 off", "d": "$1", "e": "${0.id}", "f": 8, "g": "8x", "h": "$1$", "i": "$1."}));
    }

    /// Czech typesetting: a nonbreaking space after one-letter prepositions.
    #[test]
    fn a_script_grep_change_keeps_its_group_reference() {
        let mut s = crate::Session::new();
        let steps = vec![
            Step { command: "file.new".into(), params: json!({}) },
            Step { command: "frame.create".into(), params: json!({"rect": [36, 36, 300, 300], "content": "text", "text": "a v Praze"}) },
            Step { command: "find.change".into(), params: json!({"find": r"\b([av]) ", "change": "$1\u{a0}", "grep": true, "caseSensitive": true}) },
            Step { command: "story.get".into(), params: json!({"story": "$1.story"}) },
        ];
        let r = s.run_script(&steps);
        assert!(r.failed.is_none(), "{r:?}");
        assert_eq!(r.results[2]["count"], 2);
        assert_eq!(r.results[3]["text"], "a\u{a0}v\u{a0}Praze");
    }

    #[test]
    fn runs_on_a_session_with_references() {
        let mut s = crate::Session::new();
        let steps = parse(
            "file.new {\"pages\": 2}\n\
             frame.create {\"rect\": [72, 72, 300, 200], \"content\": \"text\", \"text\": \"Hello\"}\n\
             text.select {\"story\": \"$1.story\", \"anchor\": 5, \"focus\": 5}\n\
             text.insert {\"text\": \" world\"}\n\
             story.get {\"story\": \"$1.story\"}",
        )
        .unwrap();
        let r = s.run_script(&steps);
        assert!(r.failed.is_none(), "{r:?}");
        assert_eq!(r.results[4]["text"], "Hello world");
        // Failure reports the step and keeps earlier results.
        let r = s.run_script(&parse("document.inspect\nno.such.command\ndocument.inspect").unwrap());
        assert_eq!(r.results.len(), 1);
        assert_eq!(r.failed.as_ref().map(|f| (f.0, f.1.as_str())), Some((1, "no.such.command")));
        assert_eq!(r.to_json()["failedIndex"], 1);
    }
}
