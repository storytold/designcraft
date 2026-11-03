//! Stop editor work after graphics loss while retaining documents for recovery downloads.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use designcraft_engine::{Session, doc::Document};

#[derive(Clone, Default)]
pub(crate) struct GraphicsState(Arc<AtomicBool>);

impl GraphicsState {
    /// Only the first loss should replace the error panel or move keyboard focus.
    pub(crate) fn fail(&self) -> bool {
        !self.0.swap(true, Ordering::Relaxed)
    }

    pub(crate) fn has_failed(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub(crate) fn run_if_healthy<R>(&self, work: impl FnOnce() -> R) -> Option<R> {
        if self.has_failed() { None } else { Some(work()) }
    }
}

/// An immutable copy that outlives the session and its renderer.
#[derive(Clone)]
pub(crate) struct RecoveryDocument {
    title: String,
    doc: Arc<Document>,
}

impl RecoveryDocument {
    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn save_copy(&self) -> Result<(String, Vec<u8>), String> {
        let bytes = designcraft_format::save(&self.doc).map_err(|error| format!("Couldn't save a copy of {}: {error}", self.title))?;
        Ok((format!("{}.designcraft", self.doc.title), bytes))
    }
}

/// Retain all documents without activating one, changing dirty state, or committing a preview.
pub(crate) fn recovery_documents(session: &Session) -> Vec<RecoveryDocument> {
    session
        .documents()
        .iter()
        .map(|state| RecoveryDocument { title: state.title(), doc: state.preview_stash.as_ref().unwrap_or(&state.doc).clone() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn recovery_document(session: &Session, index: usize) -> Result<(String, Vec<u8>), String> {
        recovery_documents(session).get(index).ok_or_else(|| "This document is no longer open.".to_string())?.save_copy()
    }

    // Synthetic 1x1 red RGBA PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";

    #[test]
    fn graphics_loss_blocks_editor_work_and_cannot_be_cleared_by_another_callback() {
        let graphics = GraphicsState::default();
        let device_callback = graphics.clone();
        let mut session = Session::new();
        session.execute("file.new", &json!({"pages": 1})).unwrap();
        graphics.run_if_healthy(|| session.execute("file.place", &json!({"name": "red.png", "base64": PNG}))).unwrap().unwrap();
        graphics.run_if_healthy(|| session.execute("edit.undo", &json!({}))).unwrap().unwrap();
        let (_, undone) = recovery_document(&session, 0).unwrap();
        assert!(designcraft_engine::cmd::from_bytes(&undone).unwrap().all_items().is_empty());
        graphics.run_if_healthy(|| session.execute("edit.redo", &json!({}))).unwrap().unwrap();
        let (_, before) = recovery_document(&session, 0).unwrap();
        assert_eq!(designcraft_engine::cmd::from_bytes(&before).unwrap().all_items().len(), 1);

        assert!(device_callback.fail());
        assert!(!graphics.fail(), "duplicate context/device notifications must not replace recovery controls");
        assert!(graphics.has_failed());
        assert!(graphics.run_if_healthy(|| session.execute("edit.undo", &json!({}))).is_none());
        assert_eq!(recovery_document(&session, 0).unwrap().1, before, "a hidden editor must not apply more edits");
    }

    #[test]
    fn recovery_copies_all_open_documents_without_changing_session_state() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"title": "First", "pages": 1})).unwrap();
        session.execute("file.place", &json!({"name": "red.png", "base64": PNG})).unwrap();
        let first = session.doc().unwrap().doc.clone();
        session.execute("file.new", &json!({"title": "Second", "pages": 1})).unwrap();
        session.execute("frame.create", &json!({"rect": [10, 10, 110, 110]})).unwrap();
        let second = session.doc().unwrap().doc.clone();
        let active = session.active_index();
        let journal_len = session.journal.len();

        for (index, expected) in [first.clone(), second.clone()].iter().enumerate() {
            let (name, bytes) = recovery_document(&session, index).unwrap();
            assert_eq!(name, format!("{}.designcraft", expected.title));
            let reopened = designcraft_engine::cmd::from_bytes(&bytes).unwrap();
            assert_eq!(reopened, **expected, "the copy must retain document content and embedded PNG bytes");
        }
        assert_eq!(session.active_index(), active);
        assert_eq!(session.journal.len(), journal_len);
        assert!(Arc::ptr_eq(&session.documents()[0].doc, &first));
        assert!(Arc::ptr_eq(&session.documents()[1].doc, &second));
        assert!(session.documents().iter().all(|state| state.is_dirty()));
        assert!(recovery_document(&session, 2).is_err());
    }

    #[test]
    fn recovery_saves_the_template_during_data_merge_preview() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"title": "Template", "pages": 1})).unwrap();
        let template = session.doc().unwrap().doc.clone();
        session.execute("frame.create", &json!({"rect": [0, 0, 100, 100]})).unwrap();
        session.doc_mut().unwrap().preview_stash = Some(template.clone());
        let (_, bytes) = recovery_document(&session, 0).unwrap();
        let reopened = designcraft_engine::cmd::from_bytes(&bytes).unwrap();
        assert_eq!(reopened, *template);
        assert!(Arc::ptr_eq(session.doc().unwrap().preview_stash.as_ref().unwrap(), &template));
    }

    #[test]
    fn recovery_snapshots_keep_all_documents_and_assets_after_session_drop() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"title": "First", "pages": 1})).unwrap();
        session.execute("file.place", &json!({"name": "red.png", "base64": PNG})).unwrap();
        let first = session.doc().unwrap().doc.clone();
        session.execute("file.new", &json!({"title": "Template", "pages": 1})).unwrap();
        session.execute("file.place", &json!({"name": "template.png", "base64": PNG})).unwrap();
        let template = session.doc().unwrap().doc.clone();
        session.execute("frame.create", &json!({"rect": [0, 0, 100, 100]})).unwrap();
        session.doc_mut().unwrap().preview_stash = Some(template.clone());

        let documents = recovery_documents(&session);
        assert_eq!(documents.len(), 2);
        assert_eq!(documents[0].title(), "First");
        assert_eq!(documents[1].title(), "Template");
        assert!(Arc::ptr_eq(&documents[0].doc, &first));
        assert!(Arc::ptr_eq(&documents[1].doc, &template));
        session.stop_data_preview();
        session.execute("frame.create", &json!({"rect": [10, 10, 110, 110]})).unwrap();
        drop(session);

        for (snapshot, expected) in documents.iter().zip([first, template]) {
            let (name, bytes) = snapshot.save_copy().unwrap();
            assert_eq!(name, format!("{}.designcraft", expected.title));
            assert!(bytes.starts_with(b"PK"));
            let reopened = designcraft_engine::cmd::from_bytes(&bytes).unwrap();
            assert_eq!(reopened, *expected, "snapshots must retain every document and its embedded asset bytes");
            assert!(!reopened.assets.is_empty());
            assert!(reopened.assets.values().all(|asset| !asset.data.is_empty()));
        }
    }
}
