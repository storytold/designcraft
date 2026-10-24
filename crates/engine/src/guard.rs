//! Last-resort guard (craftrules `standards/never-crash.md`): a panic that escapes a command —
//! any edit, import or export, from the UI, the CLI, MCP or the control channel — becomes an
//! error, and the document is kept as it was before the command (and saved for crash recovery
//! when a recovery directory is set). It is a safety net: commands still must not panic.

use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::Value;

use crate::{EngineError, Result, Session};

/// Text of a panic payload (`panic!` with a literal or a formatted message).
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(ToString::to_string)
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

/// Log every panic (message and location) before it unwinds, then run the previous hook.
/// Installed once per process, when the first [`Session`] is made.
pub fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
            log::error!("panic{at}: {}", panic_message(info.payload()));
            previous(info);
        }));
    });
}

impl Session {
    /// Run command `id` (`f`) inside the guard.
    pub(crate) fn guarded(&mut self, id: &str, f: impl FnOnce(&mut Session) -> Result<Value>) -> Result<Value> {
        // The whole command transaction: every open document with its undo/redo history and
        // selection, which one is active, and the journal (nested commands journal too).
        let docs = self.docs.clone();
        let active = self.active;
        let journal = self.journal.len();
        let in_undo_step = self.in_undo_step;
        match catch_unwind(AssertUnwindSafe(|| f(self))) {
            Ok(r) => r,
            Err(payload) => {
                let msg = panic_message(payload.as_ref());
                // Keep the documents as they were before the command: a nested edit that
                // succeeded before the panic is rolled back with its history entry, a document
                // opened or switched by the command is gone, and redo it cleared is back.
                self.docs = docs;
                self.active = active;
                self.journal.truncate(journal);
                if let Some(st) = self.active_mut() {
                    st.interaction = None;
                    st.bump_revision();
                }
                if let Some(dir) = self.recovery_dir.clone()
                    && let Err(e) = crate::recovery::save(self, &dir)
                {
                    log::error!("crash recovery save after `{id}` failed: {e}");
                }
                // The command never got to say that it is done; a command that ran it goes on.
                self.in_undo_step = in_undo_step;
                log::error!("command `{id}` panicked: {msg}");
                Err(EngineError::Internal(id.to_string(), msg))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn a_panicking_command_is_an_error_and_keeps_the_document() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 36, 200, 200], "content": "text", "text": "keep me"})).unwrap();
        let before = s.active().unwrap().doc.clone();
        // A command that edits the document and then panics half way.
        let err = s
            .guarded("test.panic", |s| {
                s.execute("frame.create", &json!({"rect": [36, 236, 200, 400]}))?;
                panic!("boom");
            })
            .unwrap_err();
        assert!(err.to_string().contains("test.panic") && err.to_string().contains("boom"), "{err}");
        let st = s.active().unwrap();
        assert!(std::sync::Arc::ptr_eq(&st.doc, &before), "the document is the one from before the command");
        // The session keeps working.
        s.execute("frame.create", &json!({"rect": [36, 236, 200, 400]})).unwrap();
    }

    /// An undoable command that panics is no longer running: the next command records its own
    /// undo step.
    #[test]
    fn a_panicking_command_does_not_swallow_later_undo_steps() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let panicked = s.guarded("test.panic", |s| {
            s.in_undo_step = true;
            panic!("boom");
        });
        assert!(panicked.is_err());
        s.execute("frame.create", &json!({"rect": [36, 36, 200, 200]})).unwrap();
        assert_eq!(s.active().unwrap().history.undo.len(), 1);
    }

    /// A command that panics inside an undoable one leaves that one running: what it runs next
    /// is still part of its undo step.
    #[test]
    fn a_panic_in_a_command_run_by_another_leaves_the_outer_undo_step() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // An undoable command that runs one that panics, carries on and runs another.
        s.guarded("test.outer", |s| {
            s.in_undo_step = true;
            assert!(s.guarded("test.panic", |_| panic!("boom")).is_err());
            let made = s.execute("frame.create", &json!({"rect": [36, 36, 200, 200]}));
            s.in_undo_step = false;
            made
        })
        .unwrap();
        let st = s.active().unwrap();
        assert_eq!(st.doc.spreads[0].items.len(), 1);
        assert_eq!(st.history.undo.len(), 0, "the frame is left to the outer command's step");
    }

    /// A nested edit that succeeded before the panic is rolled back with its history entry, and
    /// the redo it cleared is back (#742).
    #[test]
    fn a_panicking_command_restores_history_with_the_document() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 36, 200, 200]})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        let (undo, redo) = {
            let h = &s.active().unwrap().history;
            (h.undo.len(), h.redo.len())
        };
        assert_eq!(redo, 1, "there is redo to lose");
        let err = s
            .guarded("test.panic", |s| {
                s.execute("frame.create", &json!({"rect": [36, 236, 200, 400]}))?;
                panic!("boom");
            })
            .unwrap_err();
        assert!(err.to_string().contains("boom"), "{err}");
        let st = s.active().unwrap();
        assert_eq!(st.history.undo.len(), undo, "the nested edit's undo entry is gone with the edit");
        assert_eq!(st.history.redo.len(), redo, "the redo the nested edit cleared is back");
        assert_eq!(st.doc.spreads[0].items.len(), 0, "the document is the one from before the command");
    }

    /// A command that opens another document and then panics leaves none of it behind (#742).
    #[test]
    fn a_panicking_command_restores_the_document_set() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let err = s
            .guarded("test.panic", |s| {
                s.execute("file.new", &json!({}))?;
                assert_eq!(s.docs.len(), 2);
                panic!("boom");
            })
            .unwrap_err();
        assert!(err.to_string().contains("boom"), "{err}");
        assert_eq!(s.docs.len(), 1, "the document the command opened is gone");
        assert_eq!(s.active, Some(0), "the document from before the command is active");
    }
}
