//! Window › Utilities › Scripts: run a command script (see [`crate::script`]) as one undo step.

use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, str_param};
use crate::Result;

fn run(s: &mut crate::Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").ok_or_else(|| bad("script.run", "`text` required"))?;
    let steps = crate::script::parse(text).map_err(|e| bad("script.run", e))?;
    // The steps record undo entries of their own while the script runs (one of them may be
    // Undo); they collapse into this command's single one.
    s.in_undo_step = false;
    let uid = s.active().map(|d| d.uid);
    let before = s.active().map_or(0, |d| d.history.undo.len());
    let report = s.run_script(&steps);
    if let Some(st) = s.active_mut()
        && Some(st.uid) == uid
        && st.history.undo.len() > before
    {
        st.history.undo.truncate(before);
    }
    let out = json!({"results": report.results, "steps": steps.len()});
    match report.failed {
        Some((i, c, e)) => Err(bad("script.run", format!("step {i} ({c}): {e}"))),
        None => Ok(out),
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "script.run",
        "Run Script",
        [],
        None,
        "{text} — command script lines (`command.id {json}`, JSON lines or a JSON array; `$N.path` refers to earlier results) → {results, steps}",
        always,
        run
    )]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn scripts_run_as_one_undo_step() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "frame.create {\"rect\": [72, 72, 200, 200]}\nobject.fill {\"swatch\": \"[Black]\", \"ids\": [\"$0.id\"]}\nframe.create {\"rect\": [300, 72, 400, 200]}";
        let r = s.execute("script.run", &json!({"text": text})).unwrap();
        assert_eq!(r["steps"], 3);
        let count = |s: &Session| s.doc().unwrap().doc.spreads[0].items.len();
        assert_eq!(count(&s), 2);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(count(&s), 0, "one undo removes the whole script's work");
        assert!(s.execute("script.run", &json!({"text": "nope.command"})).is_err());
    }
}
