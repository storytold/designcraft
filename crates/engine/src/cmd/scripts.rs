//! Window › Utilities › Scripts: run a command script (see [`crate::script`]) as one undo step.

use std::sync::Arc;

use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, str_param};
use crate::Result;

fn run(s: &mut crate::Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").ok_or_else(|| bad("script.run", "`text` required"))?;
    let steps = crate::script::parse(text).map_err(|e| bad("script.run", e))?;
    // The steps' own undo entries collapse into this command's single one. A step that fails
    // rolls the successful prefix back (document, selection and history), so a half-run script
    // never strands edits that can't be undone (#709).
    let before = s.active().map(|d| (d.uid, d.doc.clone(), d.selection.clone(), d.history.clone()));
    let report = s.run_script(&steps);
    if let Some(st) = s.active_mut()
        && let Some((uid, doc, selection, history)) = &before
        && st.uid == *uid
    {
        if report.failed.is_some() {
            st.doc = Arc::clone(doc);
            st.selection = selection.clone();
            st.history = history.clone();
            st.revision = st.revision.saturating_add(1);
        } else if st.history.undo.len() > history.undo.len() {
            st.history.undo.truncate(history.undo.len());
        }
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

    /// A script that fails after a successful step leaves the document and its history exactly as
    /// they were: the half-run prefix is not stranded without an undo entry (#709).
    #[test]
    fn a_failed_script_rolls_the_successful_prefix_back() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let count = |s: &Session| s.doc().unwrap().doc.spreads[0].items.len();
        let err = s.execute("script.run", &json!({"text": "frame.create {\"rect\": [72, 72, 200, 200]}\nnope.command"})).unwrap_err();
        assert!(err.to_string().contains("step 1") && err.to_string().contains("nope.command"), "{err}");
        assert_eq!(count(&s), 0, "the successful prefix is rolled back");
        assert!(s.execute("edit.undo", &json!({})).is_err(), "nothing was left to undo");
        // The session keeps working and its undo history is as it was.
        s.execute("frame.create", &json!({"rect": [72, 72, 200, 200]})).unwrap();
        assert_eq!(count(&s), 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(count(&s), 0);
    }
}
