#[cfg(not(target_arch = "wasm32"))]
use super::DocState;
#[cfg(not(target_arch = "wasm32"))]
use super::bytes::from_bytes;
use super::bytes::{to_bytes_with_preview, with_document_extension};
#[cfg(not(target_arch = "wasm32"))]
use super::open::load_document_fonts;
#[cfg(test)]
use super::place::image_place_tests;
use super::{Arc, EngineError, Result, Session, Value, bad, json, str_param};

pub(super) fn file_save(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc_mut()?;
    // Saving to the document's own file keeps its name, even one opened without the extension.
    let path = str_param(p, "path")
        .map(with_document_extension)
        .or_else(|| st.path.clone())
        .ok_or_else(|| bad("file.save", "missing `path` (document has never been saved)"))?;
    // Before anything about the document changes: a refused save leaves it as it was.
    crate::file_access::check_write(&path).map_err(EngineError::Other)?;
    // Prepare paths for the destination without changing the live document until persistence succeeds.
    let mut saved = (*st.doc).clone();
    crate::cmd::datamerge::refresh_relative_paths(&mut saved, std::path::Path::new(&path));
    let paths_changed = saved.data_merge != st.doc.data_merge;
    let saved = if paths_changed { Arc::new(saved) } else { st.doc.clone() };
    let bytes = to_bytes_with_preview(&saved, &s.cache)?;
    #[cfg(not(target_arch = "wasm32"))]
    crate::io::write_file(&path, &bytes).map_err(EngineError::Other)?;
    let st = s.doc_mut()?;
    if paths_changed {
        st.doc = saved;
        st.bump_revision();
    }
    st.path = Some(path.clone());
    st.saved_revision = st.revision;
    st.saved_doc = st.doc.clone();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let recovery_dir = s.recovery_dir.clone();
        let index = s.active_index().ok_or(EngineError::NoDocument)?;
        crate::recovery::discard_document(s, index, recovery_dir.as_deref());
        s.prefs.note_recent_file(&path);
    }
    Ok(json!({"path": path, "bytes": bytes.len()}))
}

/// Which access a recovery command needs to the folder it is given.
#[derive(Clone, Copy)]
pub(super) enum FolderAccess {
    Read,
    Write,
    ReadWrite,
}

/// The recovery folder: `dir` when the command names one, else the session's own (set by the
/// host application, so not the automation roots' business). A named folder is checked against
/// the roots for `access`.
pub(super) fn recovery_dir(s: &Session, p: &Value, access: FolderAccess) -> Result<std::path::PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p, access);
        Err(EngineError::Other("no recovery folder on the web".into()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(named) = str_param(p, "dir") else {
            return s.recovery_dir.clone().ok_or_else(|| bad("file.recovery", "no recovery folder (give `dir`)"));
        };
        if matches!(access, FolderAccess::Read | FolderAccess::ReadWrite) {
            crate::file_access::check_read(named).map_err(EngineError::Other)?;
        }
        if matches!(access, FolderAccess::Write | FolderAccess::ReadWrite) {
            crate::file_access::check_write(named).map_err(EngineError::Other)?;
        }
        Ok(std::path::PathBuf::from(named))
    }
}

pub(super) fn can_revert(s: &Session) -> std::result::Result<(), String> {
    match s.active() {
        Some(d) if d.path.is_some() && d.is_dirty() => Ok(()),
        Some(d) if d.path.is_none() => Err("the document has never been saved".into()),
        Some(_) => Err("no changes since the last save".into()),
        None => Err("no document open".into()),
    }
}

pub(super) fn file_revert(s: &mut Session, _: &Value) -> Result<Value> {
    let path = s.doc()?.path.clone().ok_or_else(|| bad("file.revert", "the document has never been saved"))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut d = if path.to_ascii_lowercase().ends_with(".idml") {
            let bytes = crate::io::read_file(&path).map_err(EngineError::Other)?;
            crate::cmd::interchange::import_idml_confined(&bytes, Some(std::path::Path::new(&path))).map_err(|e| EngineError::Other(e.to_string()))?
        } else {
            from_bytes(&crate::io::read_file(&path).map_err(EngineError::Other)?)?
        };
        // As opened again: the fonts beside it now (unchanged files are shared, not read again).
        let (fonts, _, _) = load_document_fonts(&mut d, &path);
        let uid = s.doc()?.uid;
        let recovery_dir = s.recovery_dir.clone();
        let index = s.active_index().ok_or(EngineError::NoDocument)?;
        let mut fresh = DocState::new(d, Some(path.clone()));
        fresh.fonts = fonts;
        fresh.uid = uid;
        // A new state has a revision no state had before, so caches don't show the old document.
        crate::recovery::discard_document(s, index, recovery_dir.as_deref());
        *s.doc_mut()? = fresh;
        Ok(json!({"path": path}))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, path);
        Err(EngineError::Other("revert isn't available on the web".into()))
    }
}

#[cfg(test)]
mod save_tests {
    use serde_json::json;

    use crate::Session;
    use crate::cmd::file::FAIL_TO_BYTES;

    /// Saving over an existing document replaces it whole and leaves nothing beside it.
    #[test]
    fn save_replaces_the_existing_file_without_leftovers() {
        let dir = std::env::temp_dir().join(format!("dc-save-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.designcraft");
        std::fs::write(&path, vec![b'x'; 1 << 20]).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Saved whole", "caret": false})).unwrap();
        s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        s.execute("file.saveACopy", &json!({"path": path.to_string_lossy()})).unwrap();
        let saved = super::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
        assert!(saved.stories.values().any(|st| st.text == "Saved whole"));
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["doc.designcraft"], "no temporary file is left");
        // A folder that doesn't exist: an error naming the path.
        let missing = dir.join("missing").join("doc.designcraft");
        let e = s.execute("file.save", &json!({"path": missing.to_string_lossy()})).unwrap_err();
        assert!(e.to_string().contains("missing"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A document that can't be written is an error: the file on disk, the unsaved state and the
    /// recovery checkpoint all stay as they were.
    #[test]
    fn a_save_that_cannot_write_the_document_changes_nothing() {
        let dir = std::env::temp_dir().join(format!("dc-save-fails-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.designcraft");
        let recovery = dir.join("recovery");
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        let before = std::fs::read(&path).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Unsaved", "caret": false})).unwrap();
        s.execute("file.recovery.save", &json!({"dir": recovery.to_string_lossy()})).unwrap();
        let checkpoint = recovery.join(format!("{}.designcraft", s.doc().unwrap().uid));
        assert!(checkpoint.exists());

        FAIL_TO_BYTES.with(|f| f.set(true));
        let save = s.execute("file.save", &json!({}));
        let copy = s.execute("file.saveACopy", &json!({"path": dir.join("copy.designcraft").to_string_lossy()}));
        let save_as = s.execute("file.saveAs", &json!({"path": dir.join("other.designcraft").to_string_lossy()}));
        let serialized = s.execute("file.serialize", &json!({}));
        FAIL_TO_BYTES.with(|f| f.set(false));

        let e = save.unwrap_err();
        assert!(e.to_string().contains("could not write the document"), "{e}");
        assert!(copy.is_err() && serialized.is_err(), "no empty copy and no empty download");
        assert_eq!(std::fs::read(&path).unwrap(), before, "the saved file is untouched");
        assert!(!dir.join("copy.designcraft").exists());
        assert!(save_as.is_err() && !dir.join("other.designcraft").exists());
        assert!(!s.prefs.recent_files.iter().any(|p| p.ends_with("other.designcraft")), "a failed Save As isn't a recent file");
        assert!(s.doc().unwrap().is_dirty(), "the document still has unsaved changes");
        assert!(checkpoint.exists(), "the recovery checkpoint stays");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1050: a placement whose coordinates overflow to infinity is refused, and Save refuses a
    /// document holding a non-finite rect anyway, leaving the file on disk as it was.
    #[test]
    fn non_finite_coordinates_are_refused_and_never_saved() {
        let image = super::image_place_tests::image_file().unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let e = s.execute("file.place", &json!({"path": image.path(), "x": 1e308, "width": 1e308})).unwrap_err();
        assert!(e.to_string().contains("finite"), "{e}");
        assert!(s.doc().unwrap().doc.all_items().is_empty(), "nothing is placed");

        let dir = std::env::temp_dir().join(format!("dc-save-non-finite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.designcraft");
        s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        let before = std::fs::read(&path).unwrap();
        let id = designcraft_doc::ItemId(s.execute("frame.create", &json!({"rect": [36, 36, 300, 200]})).unwrap()["id"].as_u64().unwrap());
        let st = s.doc_mut().unwrap();
        let infinite = designcraft_geom::Rect::new(36.0, 36.0, f64::INFINITY, 200.0);
        std::sync::Arc::make_mut(&mut st.doc).item_mut(id).unwrap().path = designcraft_geom::shapes::rectangle(infinite);
        let e = s.execute("file.save", &json!({})).unwrap_err();
        assert!(e.to_string().contains("not a finite number"), "{e}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "the saved file is untouched");
        assert!(s.doc().unwrap().is_dirty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
