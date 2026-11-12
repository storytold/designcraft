use super::*;

/// [`super::discard`] as the session that wrote or recovered the entry, if one did.
fn discard(dir: &Path, uid: u64) {
    let key = directory_key(dir).ok().map(|key| (key, uid));
    let owner = key.and_then(|key| WRITTEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key).copied());
    super::discard(dir, uid, owner.unwrap_or(0));
}

fn dirty_session() -> Session {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("frame.create", &json!({"rect": [10, 10, 100, 100]})).unwrap();
    session
}

#[test]
fn recovers_unsaved_documents() {
    let dir = std::env::temp_dir().join(format!("dc-recovery-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.new", &json!({})).unwrap();
    // Only the edited document is written.
    s.execute("frame.create", &json!({"rect": [10, 10, 100, 100]})).unwrap();
    assert_eq!(save(&s, &dir).unwrap(), 1);
    assert_eq!(list(&dir).len(), 1);
    // "Crash": a new session reopens it, unsaved, with the item.
    let mut s2 = Session::new();
    let opened = open(&mut s2, &dir).unwrap();
    assert_eq!(opened.len(), 1);
    let st = s2.doc().unwrap();
    assert!(st.is_dirty());
    assert_eq!(st.doc.spreads[0].items.len(), 1);
    // The entry moved to the reopened document: a session that ends before its first
    // recovery save (a restart, another crash) still leaves it to recover.
    let entries = list(&dir);
    assert_eq!(entries.iter().map(|e| e.0).collect::<Vec<_>>(), [st.uid]);
    let mut s3 = Session::new();
    assert_eq!(open(&mut s3, &dir).unwrap().len(), 1);
    assert_eq!(s3.doc().unwrap().doc.spreads[0].items.len(), 1);
    assert!(s3.doc().unwrap().is_dirty());
    assert_eq!(list(&dir).len(), 1, "recovering twice never duplicates an entry");
    discard(&dir, s3.doc().unwrap().uid);
    // Saving a document drops its entry.
    s.recovery_dir = Some(dir.clone());
    assert_eq!(save(&s, &dir).unwrap(), 1);
    let path = dir.join("saved.designcraft");
    s.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
    assert!(list(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_recovered_document_survives_a_second_crash_before_autosave() {
    let dir = tempfile::tempdir().unwrap();
    let original = unsaved("edits preserved after a second crash");
    assert_eq!(save(&original, dir.path()).unwrap(), 1);
    let mut reopened = Session::new();
    assert_eq!(open(&mut reopened, dir.path()).unwrap().len(), 1);
    drop(reopened);
    let mut recovered_again = Session::new();
    assert_eq!(open(&mut recovered_again, dir.path()).unwrap().len(), 1);
    assert_eq!(texts(&recovered_again), ["edits preserved after a second crash"]);
    assert!(recovered_again.documents().iter().all(DocState::is_dirty));
    assert_eq!(list(dir.path()).len(), 1);
}

#[test]
fn opening_recovery_again_does_not_duplicate_live_documents() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(save(&unsaved("one recovered document"), dir.path()).unwrap(), 1);
    let mut reopened = Session::new();
    reopened.recovery_dir = Some(dir.path().to_path_buf());
    assert_eq!(open(&mut reopened, dir.path()).unwrap().len(), 1);
    assert!(open(&mut reopened, dir.path()).unwrap().is_empty());
    assert_eq!(texts(&reopened), ["one recovered document"]);
    assert_eq!(list(dir.path()).len(), 1);
}

/// Directory collisions fail even for privileged runners. Creating the blocker in the
/// publisher occurs after UID reservation, which would otherwise skip an occupied path.
fn open_with_failed_republication(s: &mut Session, dir: &Path) -> Result<Vec<usize>> {
    let mut attempts = 0;
    let opened = open_with_publisher(s, dir, |path, bytes| {
        assert_eq!(path.extension().and_then(|extension| extension.to_str()), Some("designcraft"));
        std::fs::create_dir(path).unwrap();
        let result = publish_recovery_file(path, bytes, false);
        std::fs::remove_dir(path).unwrap();
        assert!(result.is_err(), "a directory must obstruct actual recovery publication");
        attempts += 1;
        result
    });
    assert_eq!(attempts, 1, "the fixture must exercise the actual failed publication");
    opened
}

#[test]
fn a_failed_recovery_rewrite_preserves_the_original() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(save(&unsaved("keep the original"), dir.path()).unwrap(), 1);
    let entries = list(dir.path());
    let original = files(dir.path(), entries[0].0).0;
    let bytes = std::fs::read(&original).unwrap();
    let mut reopened = Session::new();
    let result = open_with_failed_republication(&mut reopened, dir.path());
    assert_eq!(result.unwrap().len(), 1);
    assert_eq!(texts(&reopened), ["keep the original"]);
    assert_eq!(std::fs::read(original).unwrap(), bytes);
    assert_eq!(list(dir.path()).len(), 1);
    // Its source uid differs from the reopened uid, but explicit Close still discards it.
    assert!(open(&mut reopened, dir.path()).unwrap().is_empty());
    reopened.close_document(0);
    assert!(list(dir.path()).is_empty());
}

#[cfg(unix)]
#[test]
fn recovery_data_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("recovery");
    let s = dirty_session();
    assert_eq!(save(&s, &dir).unwrap(), 1);

    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert_eq!(entries.len(), 2);
    for entry in entries {
        let entry = entry.unwrap();
        assert_eq!(entry.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
}

/// The recovery writer is one thread for the whole process, and its results go to whichever
/// session polls it: tests that submit to it take turns, so one test's write or error is never
/// drained by another.
static BACKGROUND: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn background() -> std::sync::MutexGuard<'static, ()> {
    BACKGROUND.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Wait for the background writer to have nothing left, then read `file.recovery.progress` as the
/// app does at the end of a frame.
fn finished_writes(s: &mut Session) -> usize {
    testing::wait_until_idle();
    let v = s.execute("file.recovery.progress", &json!({})).unwrap();
    assert_eq!(v["errors"], json!([]), "{v}");
    assert_eq!(v["busy"], false, "{v}");
    v["written"].as_u64().unwrap_or(0) as usize
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_background_save_returns_while_its_write_is_held() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.path().to_path_buf());
    let uid = s.docs[0].uid;
    let (file, _) = files(dir.path(), uid);
    let held = testing::hold_next_write(dir.path());
    let queued = s.execute("file.recovery.save", &json!({"background": true})).unwrap();
    assert!(!held.ran_here(), "the write ran on the calling thread");
    assert_eq!(queued["queued"], 1, "{queued}");
    // The writer thread is stopped at the start of the write, and the session goes on.
    held.wait_until_held();
    let v = s.execute("file.recovery.progress", &json!({})).unwrap();
    assert_eq!(v["busy"], true, "{v}");
    s.execute("frame.create", &json!({"rect": [20, 20, 60, 60]})).unwrap();
    assert!(!file.exists(), "nothing is published while the write is held");
    held.release();
    assert_eq!(finished_writes(&mut s), 1);
    assert!(file.exists());
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_document_saved_while_its_write_is_held_leaves_no_snapshot() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.path().to_path_buf());
    let uid = s.docs[0].uid;
    let held = testing::hold_next_write(dir.path());
    s.execute("file.recovery.save", &json!({"background": true})).unwrap();
    held.wait_until_held();
    let saved = dir.path().join("saved.designcraft");
    s.execute("file.saveAs", &json!({"path": saved.to_string_lossy()})).unwrap();
    held.release();
    // Before the session takes the result: a crash now must not find the stale snapshot.
    testing::wait_until_idle();
    assert!(!files(dir.path(), uid).0.exists(), "the stale snapshot was not published");
    assert!(!files(dir.path(), uid).1.exists(), "nor its metadata");
    assert_eq!(finished_writes(&mut s), 0);
    assert!(list(dir.path()).is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_document_saved_while_its_write_to_a_named_folder_is_held_leaves_no_snapshot() {
    // A folder given to `file.recovery.save` is not the session's, and is not in the document's
    // entries until the write's result is taken.
    let _turn = background();
    let named = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    let uid = s.docs[0].uid;
    let held = testing::hold_next_write(named.path());
    let queued = s.execute("file.recovery.save", &json!({"background": true, "dir": named.path().to_string_lossy()})).unwrap();
    assert_eq!(queued["queued"], 1, "{queued}");
    held.wait_until_held();
    let saved = named.path().join("saved.designcraft");
    s.execute("file.saveAs", &json!({"path": saved.to_string_lossy()})).unwrap();
    held.release();
    // Before the session takes the result: a crash now must not find the stale snapshot.
    testing::wait_until_idle();
    assert!(!files(named.path(), uid).0.exists(), "the stale snapshot was not published");
    assert_eq!(finished_writes(&mut s), 0);
    assert!(list(named.path()).is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn saving_removes_a_finished_write_to_a_named_folder_whose_result_was_not_taken() {
    let _turn = background();
    let named = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    let uid = s.docs[0].uid;
    s.execute("file.recovery.save", &json!({"background": true, "dir": named.path().to_string_lossy()})).unwrap();
    testing::wait_until_idle();
    assert!(files(named.path(), uid).0.exists(), "the write finished");
    let saved = named.path().join("saved.designcraft");
    s.execute("file.saveAs", &json!({"path": saved.to_string_lossy()})).unwrap();
    assert!(!files(named.path(), uid).0.exists(), "Save removed the snapshot before its result was taken");
    // Edited again before the result is taken: the removed snapshot is not its checkpoint.
    s.execute("frame.create", &json!({"rect": [20, 20, 60, 60]})).unwrap();
    assert_eq!(finished_writes(&mut s), 0);
    assert!(!s.docs[0].recovery_entries.iter().any(|(d, _)| same_directory(d, named.path())), "no checkpoint for a removed snapshot");
    assert!(list(named.path()).is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn opening_recovery_leaves_a_finished_background_write_whose_result_was_not_taken() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.path().to_path_buf());
    let uid = s.docs[0].uid;
    s.execute("file.recovery.save", &json!({"background": true})).unwrap();
    testing::wait_until_idle();
    assert!(files(dir.path(), uid).0.exists(), "the write finished");
    assert!(open(&mut s, dir.path()).unwrap().is_empty(), "the live document's own snapshot is not recovered");
    assert_eq!(s.docs.len(), 1);
    assert_eq!(finished_writes(&mut s), 1);
    assert_eq!(list(dir.path()).len(), 1);
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_edit_after_a_save_is_written_after_the_cancelled_write_stops() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.path().to_path_buf());
    let uid = s.docs[0].uid;
    let held = testing::hold_next_write(dir.path());
    s.execute("file.recovery.save", &json!({"background": true})).unwrap();
    held.wait_until_held();
    let saved = tempfile::tempdir().unwrap();
    s.execute("file.saveAs", &json!({"path": saved.path().join("saved.designcraft").to_string_lossy()})).unwrap();
    s.execute("frame.create", &json!({"rect": [20, 20, 60, 60]})).unwrap();
    // The held write was cancelled: the new edit is queued behind it instead of waiting for the
    // next timer tick.
    let queued = s.execute("file.recovery.save", &json!({"background": true})).unwrap();
    assert_eq!(queued["queued"], 1, "{queued}");
    held.release();
    assert_eq!(finished_writes(&mut s), 1);
    let mut reopened = Session::new();
    assert_eq!(open(&mut reopened, dir.path()).unwrap().len(), 1);
    assert_eq!(reopened.doc().unwrap().doc.spreads[0].items.len(), 2, "the snapshot after the edit was written");
    discard(dir.path(), reopened.doc().unwrap().uid);
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_background_result_goes_to_the_session_that_queued_it() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut queued_by = dirty_session();
    queued_by.recovery_dir = Some(dir.path().to_path_buf());
    let uid = queued_by.docs[0].uid;
    let mut other = dirty_session();
    queued_by.execute("file.recovery.save", &json!({"background": true})).unwrap();
    testing::wait_until_idle();
    // Another session has none of these documents: it must neither take the result nor remove
    // the snapshot as if its document had been closed.
    let v = other.execute("file.recovery.progress", &json!({})).unwrap();
    assert_eq!(v["written"], 0, "{v}");
    assert!(files(dir.path(), uid).0.exists(), "the snapshot is still there");
    assert_eq!(finished_writes(&mut queued_by), 1);
    assert!(queued_by.docs[0].recovery_entries.iter().any(|(d, u)| *u == uid && same_directory(d, dir.path())), "the checkpoint is recorded");
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn saving_in_one_session_leaves_another_sessions_write_alone() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut queued_by = dirty_session();
    queued_by.recovery_dir = Some(dir.path().to_path_buf());
    let uid = queued_by.docs[0].uid;
    // The same uid in another session (recovered documents keep the uids of their entries).
    let mut other = dirty_session();
    other.docs[0].uid = uid;
    let held = testing::hold_next_write(dir.path());
    queued_by.execute("file.recovery.save", &json!({"background": true})).unwrap();
    held.wait_until_held();
    let elsewhere = tempfile::tempdir().unwrap();
    other.execute("file.saveAs", &json!({"path": elsewhere.path().join("other.designcraft").to_string_lossy()})).unwrap();
    held.release();
    assert_eq!(finished_writes(&mut queued_by), 1);
    assert!(files(dir.path(), uid).0.exists(), "the other session's save did not cancel this write");
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn saving_in_one_session_leaves_another_sessions_write_to_the_same_folder_alone() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut queued_by = dirty_session();
    queued_by.recovery_dir = Some(dir.path().to_path_buf());
    let uid = queued_by.docs[0].uid;
    // The same uid and recovery folder in another session.
    let mut other = dirty_session();
    other.recovery_dir = Some(dir.path().to_path_buf());
    other.docs[0].uid = uid;
    let held = testing::hold_next_write(dir.path());
    queued_by.execute("file.recovery.save", &json!({"background": true})).unwrap();
    held.wait_until_held();
    let elsewhere = tempfile::tempdir().unwrap();
    other.execute("file.saveAs", &json!({"path": elsewhere.path().join("other.designcraft").to_string_lossy()})).unwrap();
    assert!(!worker::cancelled(dir.path(), uid), "the other session's save cancelled this write");
    held.release();
    assert_eq!(finished_writes(&mut queued_by), 1);
    assert!(files(dir.path(), uid).0.exists(), "this session's snapshot was kept");
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_write_now_in_one_session_leaves_another_sessions_write_of_the_same_uid_alone() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut queued_by = dirty_session();
    queued_by.recovery_dir = Some(dir.path().to_path_buf());
    let uid = queued_by.docs[0].uid;
    // The same uid in another session, with two objects instead of one.
    let mut other = dirty_session();
    other.docs[0].uid = uid;
    other.execute("frame.create", &json!({"rect": [20, 20, 60, 60]})).unwrap();
    let held = testing::hold_next_write(dir.path());
    queued_by.execute("file.recovery.save", &json!({"background": true})).unwrap();
    held.wait_until_held();
    let err = save(&other, dir.path()).unwrap_err();
    assert!(err.to_string().contains("still being written"), "{err}");
    let err = other.execute("file.recovery.save", &json!({"dir": dir.path().to_string_lossy()})).unwrap_err();
    assert!(err.to_string().contains("still being written"), "{err}");
    assert!(!worker::cancelled(dir.path(), uid), "the other session's write now cancelled this write");
    held.release();
    assert_eq!(finished_writes(&mut queued_by), 1);
    let mut reopened = Session::new();
    assert_eq!(open(&mut reopened, dir.path()).unwrap().len(), 1);
    assert_eq!(reopened.doc().unwrap().doc.spreads[0].items.len(), 1, "the queued session's snapshot was kept");
    discard(dir.path(), reopened.doc().unwrap().uid);
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn no_other_sessions_write_of_the_uid_is_queued_during_a_write_now() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let writing = dirty_session();
    let uid = writing.docs[0].uid;
    let mut other = dirty_session();
    other.recovery_dir = Some(dir.path().to_path_buf());
    other.docs[0].uid = uid;
    let now = worker::write_now(dir.path(), uid, writing.recovery_owner.0).expect("nothing else is in flight");
    let queued = other.execute("file.recovery.save", &json!({"background": true})).unwrap();
    assert_eq!(queued["queued"], 0, "another session's write was queued during the write now: {queued}");
    assert!(worker::write_now(dir.path(), uid, other.recovery_owner.0).is_none(), "a second write now of the uid started");
    drop(now);
    let queued = other.execute("file.recovery.save", &json!({"background": true})).unwrap();
    assert_eq!(queued["queued"], 1, "{queued}");
    assert_eq!(finished_writes(&mut other), 1);
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_write_now_cancels_a_background_write_to_the_same_folder_spelled_another_way() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let spelled = dir.path().join("sub").join("..");
    let mut s = dirty_session();
    s.recovery_dir = Some(spelled.clone());
    let uid = s.docs[0].uid;
    let held = testing::hold_next_write(&spelled);
    s.execute("file.recovery.save", &json!({"background": true})).unwrap();
    held.wait_until_held();
    // The newer state, written now under the folder's plain name.
    s.execute("frame.create", &json!({"rect": [20, 20, 60, 60]})).unwrap();
    s.execute("file.recovery.save", &json!({"dir": dir.path().to_string_lossy()})).unwrap();
    assert!(worker::cancelled(&spelled, uid), "the older write is cancelled");
    held.release();
    testing::wait_until_idle();
    let _ = s.execute("file.recovery.progress", &json!({})).unwrap();
    let mut reopened = Session::new();
    assert_eq!(open(&mut reopened, dir.path()).unwrap().len(), 1);
    assert_eq!(reopened.doc().unwrap().doc.spreads[0].items.len(), 2, "the newer snapshot was kept");
    discard(dir.path(), reopened.doc().unwrap().uid);
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn queued_background_writes_count_toward_the_checkpoint_limit() {
    let _turn = background();
    let mut s = dirty_session();
    let folders: Vec<tempfile::TempDir> = (0..=MAX_RECOVERY_ENTRIES).map(|_| tempfile::tempdir().unwrap()).collect();
    // The first write is held, so every later one is still queued when the next is asked for.
    let held = testing::hold_next_write(folders[0].path());
    for folder in &folders[..MAX_RECOVERY_ENTRIES] {
        let queued = s.execute("file.recovery.save", &json!({"background": true, "dir": folder.path().to_string_lossy()})).unwrap();
        assert_eq!(queued["queued"], 1, "{queued}");
    }
    let over = folders[MAX_RECOVERY_ENTRIES].path();
    let err = s.execute("file.recovery.save", &json!({"background": true, "dir": over.to_string_lossy()})).unwrap_err();
    assert!(err.to_string().contains("too many recovery checkpoints"), "{err}");
    // Saving cancels the queued writes: they no longer count once the document changes again.
    let saved = folders[0].path().join("saved.designcraft");
    s.execute("file.saveAs", &json!({"path": saved.to_string_lossy()})).unwrap();
    s.execute("frame.create", &json!({"rect": [20, 20, 60, 60]})).unwrap();
    let queued = s.execute("file.recovery.save", &json!({"background": true, "dir": over.to_string_lossy()})).unwrap();
    assert_eq!(queued["queued"], 1, "{queued}");
    held.release();
    assert_eq!(finished_writes(&mut s), 1, "only the write queued after the save is a checkpoint");
    assert_eq!(list(over).len(), 1);
    let uid = s.docs[0].uid;
    for folder in &folders {
        discard(folder.path(), uid);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn results_for_a_session_that_is_gone_are_not_kept() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut gone = dirty_session();
    gone.recovery_dir = Some(dir.path().to_path_buf());
    let owner = gone.recovery_owner.0;
    gone.execute("file.recovery.save", &json!({"background": true})).unwrap();
    testing::wait_until_idle();
    drop(gone);
    let mut other = dirty_session();
    other.execute("file.recovery.progress", &json!({})).unwrap();
    let writer = worker::started().unwrap();
    let kept = writer.undelivered.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(!kept.iter().any(|d| d.owner == owner), "a result nobody can take was kept");
}

#[test]
fn a_background_save_rejects_a_background_that_is_not_true_or_false() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.path().to_path_buf());
    let err = s.execute("file.recovery.save", &json!({"background": "yes"})).unwrap_err();
    assert!(err.to_string().contains("`background` must be true or false"), "{err}");
    assert!(list(dir.path()).is_empty(), "nothing was written");
}

#[test]
fn a_background_save_writes_recovery_data_the_app_can_recover() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.path().to_path_buf());
    // The timer's call: the interface thread does not wait for the deflate or the file writes.
    let queued = s.execute("file.recovery.save", &json!({"background": true})).unwrap();
    assert_eq!(queued["queued"], 1, "{queued}");
    assert_eq!(finished_writes(&mut s), 1);
    assert_eq!(list(dir.path()).len(), 1);
    assert!(s.docs[0].recovery_entries.iter().any(|(d, _)| same_directory(d, dir.path())), "the checkpoint is recorded");
    // Recovering it gives the unsaved edits back.
    let mut reopened = Session::new();
    assert_eq!(open(&mut reopened, dir.path()).unwrap().len(), 1);
    assert_eq!(reopened.doc().unwrap().doc.spreads[0].items.len(), 1);
    assert!(reopened.doc().unwrap().is_dirty());
    discard(dir.path(), reopened.doc().unwrap().uid);
}

#[test]
fn a_snapshot_that_finishes_after_the_save_is_dropped() {
    // A write of a large document runs for seconds: the user can save while it does. The saved
    // document's checkpoint is consumed then, and the write must not put a stale snapshot back.
    let dir = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.path().to_path_buf());
    let uid = s.docs[0].uid;
    // The snapshot the writer took before the save.
    let entry = Snapshot::of(&prepare_save(&s, dir.path()).unwrap(), dir.path(), &s.docs[0]);
    let saved = dir.path().join("saved.designcraft");
    s.execute("file.saveAs", &json!({"path": saved.to_string_lossy()})).unwrap();
    assert!(!s.docs[0].is_dirty());
    // The write lands after the save.
    let written = write_snapshot(&entry, || false);
    assert!(written.published && written.error.is_none(), "the snapshot landed");
    let (file, _) = files(dir.path(), uid);
    assert!(file.exists());
    assert!(!record_checkpoint(&mut s, dir.path().to_path_buf(), uid), "a saved document records no checkpoint");
    assert!(!file.exists(), "its snapshot is not left to be recovered");
    assert!(list(dir.path()).is_empty());
    // A document still unsaved does record one.
    s.execute("frame.create", &json!({"rect": [10, 10, 50, 50]})).unwrap();
    let entry = Snapshot::of(&prepare_save(&s, dir.path()).unwrap(), dir.path(), &s.docs[0]);
    let written = write_snapshot(&entry, || false);
    assert!(written.published && written.error.is_none(), "the second snapshot landed");
    assert!(record_checkpoint(&mut s, dir.path().to_path_buf(), uid));
    assert!(s.docs[0].recovery_entries.iter().any(|(d, u)| *u == uid && d == dir.path()));
}

#[test]
fn a_shared_folders_entry_is_only_its_writing_sessions() {
    // A recovered entry keeps the uid it was written under, which a document of another session
    // in the same process can have.
    let dir = tempfile::tempdir().unwrap();
    let writer = dirty_session();
    let uid = writer.docs[0].uid;
    assert_eq!(save(&writer, dir.path()).unwrap(), 1);
    let (file, _) = files(dir.path(), uid);
    let snapshot = std::fs::read(&file).unwrap();
    // Another session's clean document with that uid: its discard leaves the entry alone.
    let mut clean = Session::new();
    clean.execute("file.new", &json!({})).unwrap();
    clean.docs[0].uid = uid;
    assert_eq!(save(&clean, dir.path()).unwrap(), 0);
    assert!(file.exists(), "another session's discard removed the entry");
    // Another session's edited document with that uid: it does not write over the entry.
    let mut edited = dirty_session();
    edited.docs[0].uid = uid;
    assert!(save(&edited, dir.path()).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), snapshot, "another session wrote over the entry");
    super::discard(dir.path(), uid, writer.recovery_owner.0);
    assert!(!file.exists(), "the writing session removes its entry");
}

#[test]
fn a_snapshot_queued_as_the_sessions_own_is_recorded_when_it_lands() {
    // A write taken while the shared folder's entry was this session's, published after a cleanup
    // dropped that record: Save and Close must still remove what it put there.
    let dir = tempfile::tempdir().unwrap();
    let s = dirty_session();
    let uid = s.docs[0].uid;
    let owner = s.recovery_owner.0;
    assert_eq!(save(&s, dir.path()).unwrap(), 1);
    let entry = Snapshot::of(&prepare_save(&s, dir.path()).unwrap(), dir.path(), &s.docs[0]);
    assert!(entry.own);
    super::discard(dir.path(), uid, owner);
    let (file, _) = files(dir.path(), uid);
    assert!(!file.exists());
    let written = write_snapshot(&entry, || false);
    assert!(written.published && written.error.is_none(), "the snapshot landed");
    super::discard(dir.path(), uid, owner);
    assert!(!file.exists(), "the snapshot it wrote is the session's to remove");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_snapshot_whose_metadata_failed_is_still_the_documents_to_remove() {
    // The primary document is the recovery data; its metadata is optional. A metadata failure
    // must not leave the published snapshot untracked, or Save and Close would not remove it and
    // it would stay in the folder for ever.
    let root = tempfile::tempdir().unwrap();
    let dir = run_dir(root.path()).unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.clone());
    let uid = s.docs[0].uid;
    let (document, metadata) = files(&dir, uid);
    // The folder is this run's own, so the entry is its own; a folder takes the metadata's name,
    // which makes the optional write fail after the primary landed.
    std::fs::create_dir(&metadata).unwrap();
    let err = save_tracked(&mut s, &dir).unwrap_err();
    assert!(err.to_string().contains(&metadata.display().to_string()), "{err}");
    assert!(document.exists(), "the primary landed");
    assert!(s.docs[0].recovery_entries.iter().any(|(d, u)| *u == uid && same_directory(d, &dir)), "and is tracked");
    // Saving the document removes the snapshot this run wrote.
    let saved = root.path().join("saved.designcraft");
    s.execute("file.saveAs", &json!({"path": saved.to_string_lossy()})).unwrap();
    assert!(!document.exists(), "Save removed the snapshot");
    assert!(s.docs[0].recovery_entries.is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_snapshot_whose_folder_sync_failed_is_still_the_documents_to_remove() {
    // The rename put the snapshot in place before the folder sync failed: it is tracked, so Save
    // removes it.
    let root = tempfile::tempdir().unwrap();
    let dir = run_dir(root.path()).unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.clone());
    let uid = s.docs[0].uid;
    let (document, _) = files(&dir, uid);
    testing::fail_next_sync(&dir);
    let err = save_tracked(&mut s, &dir).unwrap_err();
    assert!(err.to_string().contains("sync failed"), "{err}");
    assert!(document.exists(), "the snapshot landed");
    assert!(s.docs[0].recovery_entries.iter().any(|(d, u)| *u == uid && same_directory(d, &dir)), "and is tracked");
    let saved = root.path().join("saved.designcraft");
    s.execute("file.saveAs", &json!({"path": saved.to_string_lossy()})).unwrap();
    assert!(!document.exists(), "Save removed the snapshot");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_document_undone_to_its_saved_state_while_its_write_is_held_leaves_no_snapshot() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.path().to_path_buf());
    let uid = s.docs[0].uid;
    let held = testing::hold_next_write(dir.path());
    s.execute("file.recovery.save", &json!({"background": true})).unwrap();
    held.wait_until_held();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!s.docs[0].is_dirty(), "back to the state it had");
    // A save of recovery data now drops the clean document's checkpoint, and the held write of
    // the older state must not publish after it.
    save(&s, dir.path()).unwrap();
    held.release();
    testing::wait_until_idle();
    assert!(!files(dir.path(), uid).0.exists(), "the older snapshot was not published");
    let _ = s.execute("file.recovery.progress", &json!({}));
    assert!(list(dir.path()).is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_document_undone_to_its_saved_state_loses_the_snapshot_a_finished_write_left_in_another_folder() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(other.path().to_path_buf());
    let uid = s.docs[0].uid;
    s.execute("file.recovery.save", &json!({"background": true})).unwrap();
    // Written, but its result not yet taken by the session.
    testing::wait_until_idle();
    assert!(files(other.path(), uid).0.exists());
    s.execute("edit.undo", &json!({})).unwrap();
    save(&s, dir.path()).unwrap();
    assert!(!files(other.path(), uid).0.exists(), "the snapshot of the older state is removed");
    let _ = s.execute("file.recovery.progress", &json!({}));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_clean_document_leaves_another_sessions_write_queued_behind_its_own() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut a = dirty_session();
    a.recovery_dir = Some(dir.path().to_path_buf());
    let uid = a.docs[0].uid;
    let mut b = dirty_session();
    b.recovery_dir = Some(dir.path().to_path_buf());
    b.docs[0].uid = uid;
    b.execute("frame.create", &json!({"rect": [20, 20, 60, 60]})).unwrap();
    let held = testing::hold_next_write(dir.path());
    a.execute("file.recovery.save", &json!({"background": true})).unwrap();
    held.wait_until_held();
    // A write now cancels the held one. The other session does not write over the entry it left.
    a.execute("file.recovery.save", &json!({"dir": dir.path().to_string_lossy()})).unwrap();
    let err = b.execute("file.recovery.save", &json!({"background": true})).unwrap_err();
    assert!(err.to_string().contains("did not write"), "{err}");
    a.execute("edit.undo", &json!({})).unwrap();
    assert!(!a.docs[0].is_dirty());
    save(&a, dir.path()).unwrap();
    // Once that entry is gone, the other session's write is queued to follow the held one.
    b.execute("file.recovery.save", &json!({"background": true})).unwrap();
    held.release();
    assert_eq!(finished_writes(&mut b), 1, "the other session's write ran");
    let mut reopened = Session::new();
    assert_eq!(open(&mut reopened, dir.path()).unwrap().len(), 1);
    assert_eq!(reopened.doc().unwrap().doc.spreads[0].items.len(), 2, "its snapshot was kept");
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_background_recovery_write_reports_another_sessions_held_write() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut first = dirty_session();
    first.recovery_dir = Some(dir.path().to_path_buf());
    let uid = first.docs[0].uid;
    let mut second = dirty_session();
    second.recovery_dir = Some(dir.path().to_path_buf());
    second.docs[0].uid = uid;
    second.execute("frame.create", &json!({"rect": [20, 20, 60, 60]})).unwrap();

    let held = testing::hold_next_write(dir.path());
    first.execute("file.recovery.save", &json!({"background": true})).unwrap();
    held.wait_until_held();

    let err = second.execute("file.recovery.save", &json!({"background": true})).unwrap_err();
    assert!(err.to_string().contains("another session"), "{err}");

    held.release();
    assert_eq!(finished_writes(&mut first), 1);
    assert!(files(dir.path(), uid).0.exists(), "the first session's recovery file remains intact");
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_clean_document_in_another_session_leaves_this_sessions_write_alone() {
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut queued_by = dirty_session();
    queued_by.recovery_dir = Some(dir.path().to_path_buf());
    let uid = queued_by.docs[0].uid;
    // The same uid, clean, in another session.
    let mut other = Session::new();
    other.execute("file.new", &json!({})).unwrap();
    other.docs[0].uid = uid;
    assert!(!other.docs[0].is_dirty());
    let held = testing::hold_next_write(dir.path());
    queued_by.execute("file.recovery.save", &json!({"background": true})).unwrap();
    held.wait_until_held();
    save(&other, dir.path()).unwrap();
    held.release();
    assert_eq!(finished_writes(&mut queued_by), 1);
    assert!(files(dir.path(), uid).0.exists(), "the other session's save did not cancel this write");
    discard(dir.path(), uid);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_background_snapshot_whose_metadata_failed_is_still_the_documents_to_remove() {
    // The background writer's result must be applied the same way: a published primary is the
    // document's snapshot even when its optional metadata fails.
    let _turn = background();
    let root = tempfile::tempdir().unwrap();
    let dir = run_dir(root.path()).unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.clone());
    let uid = s.docs[0].uid;
    let (document, metadata) = files(&dir, uid);
    std::fs::create_dir(&metadata).unwrap();
    let queued = s.execute("file.recovery.save", &json!({"background": true})).unwrap();
    assert_eq!(queued["queued"], 1, "{queued}");
    testing::wait_until_idle();
    let v = s.execute("file.recovery.progress", &json!({})).unwrap();
    assert_eq!(v["busy"], false, "{v}");
    let reported = v["errors"].as_array().and_then(|errors| errors.first()).and_then(Value::as_str).map(str::to_string);
    let reported = reported.expect("the metadata failure is reported");
    assert!(reported.contains(&metadata.display().to_string()), "{reported}");
    assert!(document.exists(), "the primary landed");
    assert!(s.docs[0].recovery_entries.iter().any(|(d, u)| *u == uid && same_directory(d, &dir)), "and is tracked");
    // Saving the document removes the snapshot this run wrote.
    let saved = root.path().join("saved.designcraft");
    s.execute("file.saveAs", &json!({"path": saved.to_string_lossy()})).unwrap();
    assert!(!document.exists(), "Save removed the snapshot");
}

#[test]
fn a_write_cancelled_midway_publishes_nothing() {
    // Save/Revert/Close during a write marks it cancelled; the snapshot must not land after it,
    // even if the app crashes before the next interface frame discards it.
    let dir = tempfile::tempdir().unwrap();
    let s = dirty_session();
    let entry = Snapshot::of(&prepare_save(&s, dir.path()).unwrap(), dir.path(), &s.docs[0]);
    let written = write_snapshot(&entry, || true);
    assert!(!written.published && written.error.is_none(), "a cancelled write is not an error");
    assert!(list(dir.path()).is_empty(), "a cancelled write leaves no snapshot");
    assert!(!files(dir.path(), entry.uid).0.exists(), "nor its document");
    assert!(!files(dir.path(), entry.uid).1.exists(), "nor its metadata");
    // The temporary file it was written to is dropped, not left behind.
    assert!(
        std::fs::read_dir(dir.path()).unwrap().all(|e| e.is_ok_and(|e| !e.file_name().to_string_lossy().starts_with(".designcraft-recovery-"))),
        "no temporary file is left"
    );
}

#[test]
fn a_cancel_stops_a_write_before_the_package_is_finished() {
    let dir = tempfile::tempdir().unwrap();
    let s = dirty_session();
    let entry = Snapshot::of(&prepare_save(&s, dir.path()).unwrap(), dir.path(), &s.docs[0]);
    // Cancelled once the write has started: the first check passes, every later one fails.
    let checks = std::cell::Cell::new(0);
    let written = write_snapshot(&entry, || {
        checks.set(checks.get() + 1);
        checks.get() > 1
    });
    assert!(!written.published && written.error.is_none(), "a stopped write is not an error");
    assert!(checks.get() >= 2, "the write checked again before it finished");
    assert!(!files(dir.path(), entry.uid).0.exists(), "nothing was published");
    assert!(
        std::fs::read_dir(dir.path()).unwrap().all(|e| e.is_ok_and(|e| !e.file_name().to_string_lossy().starts_with(".designcraft-recovery-"))),
        "no temporary file is left"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn only_a_queued_write_is_cancelled() {
    // A mark for a document with no write in flight would stay in the writer's set for ever.
    let dir = tempfile::tempdir().unwrap();
    worker::cancel_owned(dir.path(), 7, 1);
    assert!(!worker::cancelled(dir.path(), 7), "nothing was queued for it");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_panicking_recovery_write_is_an_error_and_the_writer_keeps_going() {
    let err = worker::guarded(|| -> Result<()> { panic!("boom") }).unwrap_err();
    assert!(err.to_string().contains("panicked") && err.to_string().contains("boom"), "{err}");
    assert_eq!(worker::guarded(|| Ok(7)).unwrap(), 7, "the next write still runs");
}

#[test]
fn a_saved_document_is_not_written_in_the_background() {
    // The autosave writes unsaved documents only.
    let _turn = background();
    let dir = tempfile::tempdir().unwrap();
    let mut s = dirty_session();
    s.recovery_dir = Some(dir.path().to_path_buf());
    let saved = dir.path().join("saved.designcraft");
    s.execute("file.saveAs", &json!({"path": saved.to_string_lossy()})).unwrap();
    let queued = s.execute("file.recovery.save", &json!({"background": true})).unwrap();
    assert_eq!(queued["queued"], 0, "{queued}");
    assert_eq!(finished_writes(&mut s), 0);
    assert!(list(dir.path()).is_empty());
}

#[test]
fn existing_recovery_directory_is_used() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("recovery");
    std::fs::create_dir(&dir).unwrap();

    assert_eq!(save(&dirty_session(), &dir).unwrap(), 1);
    assert_eq!(list(&dir).len(), 1);
    assert!(std::fs::symlink_metadata(&dir).unwrap().file_type().is_dir());
}

#[test]
fn non_directory_recovery_path_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("recovery");
    std::fs::write(&path, b"not a directory").unwrap();

    assert!(save(&dirty_session(), &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"not a directory");
}

#[cfg(unix)]
#[test]
fn existing_recovery_directory_permissions_are_preserved() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("recovery");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o750)).unwrap();

    assert_eq!(save(&dirty_session(), &dir).unwrap(), 1);
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o750);
}

#[cfg(unix)]
#[test]
fn symlink_recovery_directory_is_rejected() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("destination");
    let link = root.path().join("recovery");
    std::fs::create_dir(&destination).unwrap();
    symlink(&destination, &link).unwrap();

    assert!(save(&dirty_session(), &link).is_err());
    assert!(std::fs::read_dir(&destination).unwrap().next().is_none());
    std::fs::write(destination.join("7.designcraft"), b"unchanged").unwrap();
    std::fs::write(destination.join("7.json"), b"{}").unwrap();
    assert!(list(&link).is_empty());
    discard(&link, 7);
    assert_eq!(std::fs::read(destination.join("7.designcraft")).unwrap(), b"unchanged");
    assert!(open(&mut Session::new(), &link).is_err());
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
}

/// A link planted among a checked folder's entries doesn't get its target read: not as metadata
/// (listed as is) and not as a document (recovered).
#[cfg(unix)]
#[test]
fn linked_recovery_entries_are_not_read() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let (dir, elsewhere) = (root.path().join("recovery"), root.path().join("elsewhere"));
    std::fs::create_dir(&dir).unwrap();
    std::fs::create_dir(&elsewhere).unwrap();
    // A real document whose metadata is a link to a private file.
    assert_eq!(save(&unsaved("real"), &dir).unwrap(), 1);
    let uid = list(&dir)[0].0;
    std::fs::write(elsewhere.join("secret.json"), br#"{"path": "/home/someone/secret.designcraft"}"#).unwrap();
    std::fs::remove_file(dir.join(format!("{uid}.json"))).unwrap();
    symlink(elsewhere.join("secret.json"), dir.join(format!("{uid}.json"))).unwrap();
    // A document entry that is a link to a document elsewhere.
    assert_eq!(save(&unsaved("elsewhere"), &elsewhere).unwrap(), 1);
    let linked = list(&elsewhere)[0].0;
    symlink(elsewhere.join(format!("{linked}.designcraft")), dir.join("9000000.designcraft")).unwrap();

    let entries = list(&dir);
    assert_eq!(entries.iter().map(|e| e.0).collect::<Vec<_>>(), [uid, 9_000_000]);
    assert_eq!(entries[0].1, json!({}), "the linked metadata counts as none");
    let mut s = Session::new();
    assert_eq!(open(&mut s, &dir).unwrap().len(), 1);
    assert_eq!(texts(&s), ["real"]);
    assert!(std::fs::symlink_metadata(dir.join("9000000.designcraft")).unwrap().file_type().is_symlink(), "left as it was");
}

#[test]
fn a_run_has_a_locked_folder_of_its_own() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("recovery");
    let own = run_dir(&dir).unwrap();
    assert_eq!(own.parent(), Some(dir.as_path()));
    assert_eq!(run_dir(&dir).unwrap(), own, "one for the whole run");
    let lock = std::fs::File::open(own.join(LOCK)).unwrap();
    assert!(matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock)), "held while the run lives");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&own).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(own.join(LOCK)).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

fn failed_republication_session() -> (Session, tempfile::TempDir, u64) {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let live_uid = session.docs[0].uid;
    let mut original = unsaved("retained old work");
    original.docs[0].uid = live_uid;
    save(&original, root.path()).unwrap();
    session.recovery_dir = Some(root.path().to_path_buf());
    let recovered = open_with_failed_republication(&mut session, root.path());
    assert_eq!(recovered.unwrap().len(), 1);
    assert_ne!(session.docs[1].uid, live_uid);
    (session, root, live_uid)
}

#[test]
fn retained_source_survives_a_coincident_live_uid_checkpoint_failure() {
    let (mut session, root, live_uid) = failed_republication_session();
    let original = std::fs::read(files(root.path(), live_uid).0).unwrap();
    let recovered_uid = session.docs[1].uid;
    std::fs::create_dir(files(root.path(), recovered_uid).0).unwrap();
    session.execute("file.activate", &json!({"index": 0})).unwrap();
    session.execute("frame.create", &json!({"rect": [0,0,100,100], "content": "text", "text": "other live work"})).unwrap();
    assert!(session.execute("file.recovery.save", &json!({"dir": root.path().to_string_lossy()})).is_err());
    assert_eq!(std::fs::read(files(root.path(), live_uid).0).unwrap(), original);
}

#[test]
fn clean_autosave_preserves_another_documents_retained_source() {
    let (mut session, root, live_uid) = failed_republication_session();
    let checkpoint = files(root.path(), live_uid).0;
    let original = std::fs::read(&checkpoint).unwrap();
    assert!(!session.docs[0].is_dirty());
    assert!(session.docs[1].is_dirty());
    assert!(session.execute("file.recovery.save", &json!({})).is_err());
    assert_eq!(std::fs::read(checkpoint).unwrap(), original);
}

#[test]
fn closing_a_coincident_document_preserves_another_documents_retained_source() {
    let (mut session, root, live_uid) = failed_republication_session();
    let checkpoint = files(root.path(), live_uid).0;
    let original = std::fs::read(&checkpoint).unwrap();
    session.execute("file.close", &json!({"index": 0})).unwrap();
    assert_eq!(std::fs::read(checkpoint).unwrap(), original);
}

#[test]
fn saving_a_coincident_document_preserves_another_documents_retained_source() {
    let (mut session, root, live_uid) = failed_republication_session();
    let checkpoint = files(root.path(), live_uid).0;
    let original = std::fs::read(&checkpoint).unwrap();
    session.execute("file.activate", &json!({"index": 0})).unwrap();
    session.execute("file.saveAs", &json!({"path": root.path().join("saved-other.designcraft").to_string_lossy()})).unwrap();
    assert_eq!(std::fs::read(checkpoint).unwrap(), original);
}

#[test]
fn reverting_a_coincident_document_preserves_another_documents_retained_source() {
    let (mut session, root, live_uid) = failed_republication_session();
    let checkpoint = files(root.path(), live_uid).0;
    let original = std::fs::read(&checkpoint).unwrap();
    let saved = root.path().join("saved-other.designcraft");
    std::fs::write(&saved, designcraft_format::save(&session.docs[0].doc).unwrap()).unwrap();
    session.docs[0].path = Some(saved.to_string_lossy().into_owned());
    session.execute("file.activate", &json!({"index": 0})).unwrap();
    session.execute("frame.create", &json!({"rect": [0,0,100,100]})).unwrap();
    session.execute("file.revert", &json!({})).unwrap();
    assert_eq!(std::fs::read(checkpoint).unwrap(), original);
}

#[test]
fn retained_source_provenance_is_consumed_before_its_uid_is_reused() {
    let (mut session, root, live_uid) = failed_republication_session();
    session.execute("file.saveAs", &json!({"path": root.path().join("saved.designcraft").to_string_lossy()})).unwrap();
    session.execute("file.activate", &json!({"index": 0})).unwrap();
    session.execute("frame.create", &json!({"rect": [0,0,100,100], "content": "text", "text": "other live work"})).unwrap();
    session.execute("file.recovery.save", &json!({"dir": root.path().to_string_lossy()})).unwrap();
    let checkpoint = files(root.path(), live_uid).0;
    let expected = std::fs::read(&checkpoint).unwrap();
    session.execute("file.close", &json!({"index": 1})).unwrap();
    assert_eq!(std::fs::read(checkpoint).unwrap(), expected, "closing the saved recovered document must not delete another document's checkpoint");
}

#[test]
fn explicit_checkpoint_tracking_is_bounded_and_deduplicated_before_writes() {
    let root = tempfile::tempdir().unwrap();
    let mut session = unsaved("bounded checkpoints");
    for n in 0..MAX_RECOVERY_ENTRIES {
        session.execute("file.recovery.save", &json!({"dir": root.path().join(n.to_string()).to_string_lossy()})).unwrap();
    }
    let excess = root.path().join("excess");
    assert!(session.execute("file.recovery.save", &json!({"dir": excess.to_string_lossy()})).is_err());
    assert!(!excess.exists(), "reject before creating or writing the next directory");
    let first_alias = root.path().join("0").join(".");
    session.execute("file.recovery.save", &json!({"dir": first_alias.to_string_lossy()})).unwrap();
    for n in 0..MAX_RECOVERY_ENTRIES {
        assert_eq!(persisted_texts(&root.path().join(n.to_string())), ["bounded checkpoints"]);
    }
    assert!(session.recovery_dir.is_none());
    session.execute("file.close", &json!({})).unwrap();
    for n in 0..MAX_RECOVERY_ENTRIES {
        assert!(list(&root.path().join(n.to_string())).is_empty());
    }
}

fn restart_fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let mut first = unsaved("restart first");
    first.docs[0].uid = 1;
    save(&first, root.path()).unwrap();
    let mut second = unsaved("restart second");
    second.docs[0].uid = 2;
    save(&second, root.path()).unwrap();
    root
}

fn persisted_texts(dir: &Path) -> Vec<String> {
    let mut result = Vec::new();
    for (uid, _) in list(dir) {
        let doc = crate::cmd::from_bytes(&std::fs::read(files(dir, uid).0).unwrap()).unwrap();
        result.extend(doc.stories.values().map(|story| story.text.clone()));
    }
    result.sort();
    result
}

#[test]
#[ignore = "subprocess helper, executed by the restart recovery regression tests"]
fn restart_recovery_child() {
    let source = PathBuf::from(std::env::var("DESIGNCRAFT_TEST_RECOVERY_SOURCE").unwrap());
    let mode = std::env::var("DESIGNCRAFT_TEST_RECOVERY_MODE").unwrap();
    if mode == "occupied_destination" {
        let owned_root = tempfile::tempdir().unwrap();
        let destination = run_dir(owned_root.path()).unwrap();
        let mut existing = unsaved("already in destination");
        existing.docs[0].uid = 3;
        save(&existing, &destination).unwrap();
        let (existing_doc, existing_meta) = files(&destination, 3);
        let doc_bytes = std::fs::read(&existing_doc).unwrap();
        let meta_bytes = std::fs::read(&existing_meta).unwrap();
        let mut session = Session::new();
        session.recovery_dir = Some(destination.clone());
        let opened = open(&mut session, &source).unwrap();
        assert_eq!(opened.len(), 2);
        assert_eq!(texts(&session), ["restart first", "restart second"]);
        assert!(session.docs.iter().all(|document| document.uid != 3));
        assert_eq!(persisted_texts(&destination), ["already in destination", "restart first", "restart second"]);
        assert!(list(&source).is_empty());
        session.execute("file.saveAs", &json!({"path": source.join("saved.designcraft").to_string_lossy()})).unwrap();
        session.execute("file.close", &json!({"index": opened[0]})).unwrap();
        assert_eq!(list(&destination).len(), 1);
        assert_eq!(std::fs::read(existing_doc).unwrap(), doc_bytes);
        assert_eq!(std::fs::read(existing_meta).unwrap(), meta_bytes);
        return;
    }
    let mut session = Session::new();
    if mode == "advance" || mode == "coincident_live" {
        session.execute("file.new", &json!({})).unwrap();
        assert_eq!(session.docs[0].uid, 1);
    }
    let owned_root;
    let destination = match mode.as_str() {
        "cross_dir" => {
            owned_root = tempfile::tempdir().unwrap();
            let own = run_dir(owned_root.path()).unwrap();
            session.recovery_dir = Some(own.clone());
            own
        }
        "alias" => {
            let alias = source.join(".");
            session.recovery_dir = Some(alias.clone());
            alias
        }
        "coincident_live" => {
            session.recovery_dir = Some(source.clone());
            source.clone()
        }
        _ => source.clone(),
    };
    let opened = open(&mut session, &source).unwrap();
    assert_eq!(opened.len(), 2);
    assert_eq!(texts(&session), ["restart first", "restart second"]);
    assert_eq!(persisted_texts(&destination), ["restart first", "restart second"]);
    assert!(session.docs.iter().all(|d| d.uid > 0 && d.uid < u64::MAX));
    if matches!(mode.as_str(), "cross_dir" | "manual" | "alias" | "revert") {
        let was_disabled = session.recovery_dir.is_none();
        if mode == "cross_dir" {
            assert!(list(&source).is_empty());
        }
        // A copy does not consume recovery. A successful Save and a Close do.
        session.execute("file.saveACopy", &json!({"path": source.join("copy.designcraft").to_string_lossy()})).unwrap();
        assert_eq!(list(&destination).len(), 2);
        session.execute("file.saveAs", &json!({"path": source.join("saved.designcraft").to_string_lossy()})).unwrap();
        assert_eq!(list(&destination).len(), 1);
        if mode == "revert" {
            session.execute("frame.create", &json!({"rect": [0, 0, 100, 100], "content": "text", "text": "after saving"})).unwrap();
            session.execute("file.recovery.save", &json!({"dir": destination.to_string_lossy()})).unwrap();
            assert_eq!(list(&destination).len(), 2);
            session.execute("file.revert", &json!({})).unwrap();
            assert_eq!(list(&destination).len(), 1);
        }
        session.execute("file.close", &json!({"index": opened[0]})).unwrap();
        assert!(list(&destination).is_empty());
        assert!(list(&source).is_empty());
        assert_eq!(session.recovery_dir.is_none(), was_disabled, "manual recovery must not configure autosave");
    }
}

/// A child process starts with a copy of every open file of this one until it runs its program, so
/// a lock file closed meanwhile stays locked: a test that closes a lock and expects it free does
/// not run while a child is started.
static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn run_restart_case(mode: &str) {
    let source = restart_fixture();
    if mode == "max_uid" {
        let (doc, meta) = files(source.path(), 2);
        let (max_doc, max_meta) = files(source.path(), u64::MAX);
        std::fs::rename(doc, max_doc).unwrap();
        std::fs::rename(meta, max_meta).unwrap();
    }
    // `spawn` returns once the child runs its program, and its copies of this process's files
    // are closed by then.
    let child = {
        let _spawning = SPAWN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "recovery::tests::restart_recovery_child", "--ignored", "--nocapture"])
            .env("DESIGNCRAFT_TEST_RECOVERY_SOURCE", source.path())
            .env("DESIGNCRAFT_TEST_RECOVERY_MODE", mode)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "fresh child failed: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let child_stdout = String::from_utf8_lossy(&output.stdout);
    assert!(child_stdout.contains("test result: ok. 1 passed; 0 failed;"), "the fresh-process helper did not run exactly once: {child_stdout}");
}

#[test]
fn restart_uid_collision_cannot_delete_the_new_snapshot() {
    run_restart_case("fresh");
}
#[test]
fn restart_uid_collision_cannot_overwrite_an_unopened_entry() {
    run_restart_case("advance");
}
#[test]
fn cross_directory_recovery_uses_the_configured_destination() {
    run_restart_case("cross_dir");
}
#[test]
fn cross_directory_recovery_preserves_occupied_destination_entries() {
    run_restart_case("occupied_destination");
}
#[test]
fn manual_recovery_save_and_close_clean_up_without_enabling_autosave() {
    run_restart_case("manual");
}
#[test]
fn recovery_directory_aliases_do_not_delete_new_snapshots() {
    run_restart_case("alias");
}
#[test]
fn a_maximum_source_uid_does_not_advance_or_overflow_the_allocator() {
    run_restart_case("max_uid");
}
#[test]
fn a_coincident_live_uid_is_not_recovered_entry_provenance() {
    run_restart_case("coincident_live");
}
#[test]
fn manual_recovery_revert_removes_its_previous_checkpoint() {
    run_restart_case("revert");
}

/// The recovery folder was cleared while the app runs: the next save makes the run's folder
/// again, and its lock with it.
#[cfg(unix)]
#[test]
fn a_removed_run_folder_is_made_again_with_its_lock() {
    let root = tempfile::tempdir().unwrap();
    let own = run_dir(root.path()).unwrap();
    std::fs::remove_dir_all(&own).unwrap();
    assert_eq!(save(&dirty_session(), &own).unwrap(), 1);
    let lock = std::fs::File::open(own.join(LOCK)).unwrap();
    assert!(matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
}

/// Replacing the name of a live run's lock must not authorize writes under the old handle.
#[cfg(unix)]
#[test]
fn save_rejects_a_run_whose_lock_was_replaced() {
    let root = tempfile::tempdir().unwrap();
    let own = run_dir(root.path()).unwrap();
    std::fs::remove_file(own.join(LOCK)).unwrap();
    let replacement = new_lock(&own).unwrap();
    replacement.try_lock().unwrap();
    assert!(save(&dirty_session(), &own).is_err());
    assert!(list(&own).is_empty(), "another owner's folder must not receive our recovery data");
}

#[cfg(windows)]
#[test]
fn an_exclusively_locked_windows_recovery_file_validates() {
    let root = tempfile::tempdir().unwrap();
    let lock = new_lock(root.path()).unwrap();
    lock.try_lock().unwrap();
    validate_lock(root.path(), &lock).unwrap();
    let named = std::fs::File::open(root.path().join(LOCK)).unwrap();
    assert!(matches!(named.try_lock(), Err(std::fs::TryLockError::WouldBlock)), "validation must retain the exclusive lock");
}

#[cfg(windows)]
#[test]
fn windows_recovery_rejects_a_replacement_with_the_same_marker() {
    let root = tempfile::tempdir().unwrap();
    let lock = new_lock(root.path()).unwrap();
    let marker = std::fs::read(root.path().join(LOCK)).unwrap();
    lock.try_lock().unwrap();
    let moved = root.path().join("original-lock");
    std::fs::rename(root.path().join(LOCK), &moved).unwrap();
    std::fs::write(root.path().join(LOCK), marker).unwrap();
    assert!(validate_lock(root.path(), &lock).is_err(), "matching marker bytes do not make another file our lock");
    std::fs::remove_file(root.path().join(LOCK)).unwrap();
    assert!(validate_lock(root.path(), &lock).is_err(), "a missing lock must not validate");
}

#[cfg(windows)]
#[test]
fn windows_recovery_does_not_write_or_discard_a_replacement_runs_entries() {
    let root = tempfile::tempdir().unwrap();
    let own = run_dir(root.path()).unwrap();
    std::fs::rename(own.join(LOCK), own.join("original-lock")).unwrap();
    let replacement = new_lock(&own).unwrap();
    replacement.try_lock().unwrap();
    let session = dirty_session();
    let (doc, meta) = files(&own, session.docs[0].uid);
    std::fs::write(&doc, b"another owner's document").unwrap();
    std::fs::write(&meta, b"{}").unwrap();
    assert!(save(&session, &own).is_err());
    discard(&own, session.docs[0].uid);
    assert_eq!(std::fs::read(&doc).unwrap(), b"another owner's document");
    assert_eq!(std::fs::read(&meta).unwrap(), b"{}");
}

/// A blocked attempt to recreate a run's lock must stop the save before it writes documents.
#[cfg(unix)]
#[test]
fn save_rejects_a_run_whose_lock_cannot_be_recreated() {
    let root = tempfile::tempdir().unwrap();
    let own = run_dir(root.path()).unwrap();
    std::fs::remove_file(own.join(LOCK)).unwrap();
    std::fs::create_dir(own.join(LOCK)).unwrap();
    assert!(save(&dirty_session(), &own).is_err());
    assert!(list(&own).is_empty());
}

#[cfg(unix)]
#[test]
fn discarding_a_document_does_not_touch_a_replaced_runs_entries() {
    let root = tempfile::tempdir().unwrap();
    let own = run_dir(root.path()).unwrap();
    std::fs::remove_file(own.join(LOCK)).unwrap();
    let replacement = new_lock(&own).unwrap();
    replacement.try_lock().unwrap();
    let (doc, meta) = files(&own, 1);
    std::fs::write(&doc, b"the replacement owner's document").unwrap();
    std::fs::write(&meta, b"{}").unwrap();
    discard(&own, 1);
    assert_eq!(std::fs::read(doc).unwrap(), b"the replacement owner's document");
    assert!(meta.exists());
}

#[cfg(unix)]
#[test]
fn saving_a_run_rejects_a_symlinked_lock() {
    let root = tempfile::tempdir().unwrap();
    let own = run_dir(root.path()).unwrap();
    std::fs::remove_file(own.join(LOCK)).unwrap();
    let unrelated = root.path().join("unrelated");
    std::fs::write(&unrelated, b"unrelated file").unwrap();
    std::os::unix::fs::symlink(&unrelated, own.join(LOCK)).unwrap();
    assert!(save(&dirty_session(), &own).is_err());
    assert_eq!(std::fs::read(unrelated).unwrap(), b"unrelated file");
}

#[cfg(unix)]
#[test]
fn a_replaced_run_directory_is_not_claimed_or_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let own = run_dir(root.path()).unwrap();
    let session = dirty_session();
    let uid = session.docs[0].uid;
    std::fs::remove_dir_all(&own).unwrap();
    std::fs::create_dir(&own).unwrap();
    let (doc, _) = files(&own, uid);
    std::fs::write(&doc, b"another owner's document").unwrap();
    assert!(save(&session, &own).is_err());
    assert_eq!(std::fs::read(doc).unwrap(), b"another owner's document");
    assert!(!own.join(LOCK).exists());
}

#[cfg(unix)]
#[test]
fn a_run_gets_no_folder_in_a_symlinked_recovery_folder() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("destination");
    let link = root.path().join("recovery");
    std::fs::create_dir(&destination).unwrap();
    symlink(&destination, &link).unwrap();

    assert!(run_dir(&link).is_err());
    assert!(std::fs::read_dir(&destination).unwrap().next().is_none());
}

fn unsaved(text: &str) -> Session {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("frame.create", &json!({"rect": [10, 10, 100, 100], "content": "text", "text": text})).unwrap();
    session
}

fn texts(s: &Session) -> Vec<String> {
    let mut texts: Vec<String> = s.documents().iter().flat_map(|d| d.doc.stories.values().map(|st| st.text.clone())).collect();
    texts.sort();
    texts
}

/// What a run that has ended leaves: its folder with its lock, free now, and the entry of
/// one unsaved document, numbered `uid` as that process numbered it.
fn run_left(root: &Path, name: &str, text: &str, uid: u64) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(LOCK), b"").unwrap();
    let mut s = unsaved(text);
    s.docs[0].uid = uid;
    assert_eq!(save(&s, &dir).unwrap(), 1);
    dir
}

/// Every process numbers its first document 1: two running apps both wrote `1.designcraft`
/// into the one recovery folder, and only the document written last was recovered.
#[test]
fn documents_of_two_apps_that_ended_are_both_recovered() {
    let root = tempfile::tempdir().unwrap();
    let first = run_left(root.path(), "run-7-0", "first app", 1);
    let second = run_left(root.path(), "run-8-0", "second app", 1);
    let own = run_dir(root.path()).unwrap();
    let mut s = Session::new();
    assert_eq!(open(&mut s, &own).unwrap().len(), 2);
    assert_eq!(texts(&s), ["first app", "second app"]);
    assert!(s.documents().iter().all(DocState::is_dirty));
    assert!(!first.exists() && !second.exists(), "nothing is left of the runs that ended");
}

/// An app that started reopened the unsaved documents of one still running, and removed
/// its entries.
#[test]
fn a_running_apps_recovery_data_is_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let running = run_left(root.path(), "run-7-0", "still open", 1);
    // No child starts while the lock is open, so closing it frees it (see `SPAWN`).
    let _no_spawn = SPAWN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let lock = std::fs::File::open(running.join(LOCK)).unwrap();
    lock.try_lock().unwrap();
    let own = run_dir(root.path()).unwrap();
    let mut s = Session::new();
    assert!(open(&mut s, &own).unwrap().is_empty());
    assert_eq!(list(&running).len(), 1);
    // Once it has ended, its document is recovered.
    drop(lock);
    assert_eq!(open(&mut s, &own).unwrap().len(), 1);
    assert_eq!(texts(&s), ["still open"]);
    assert!(!running.exists());
}

/// A process id comes round again: the folder a run left under it is not this run's.
#[test]
fn a_folder_left_under_this_process_id_is_not_reused() {
    let root = tempfile::tempdir().unwrap();
    let left = run_left(root.path(), &format!("run-{}-0", std::process::id()), "left behind", 1);
    let own = run_dir(root.path()).unwrap();
    assert_ne!(own, left);
    let mut s = Session::new();
    assert_eq!(open(&mut s, &own).unwrap().len(), 1);
    assert_eq!(texts(&s), ["left behind"]);
}

/// Entries written straight into the recovery folder, as before runs had folders.
#[test]
fn recovery_data_of_an_earlier_version_is_recovered() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(save(&unsaved("earlier version"), root.path()).unwrap(), 1);
    let own = run_dir(root.path()).unwrap();
    let mut s = Session::new();
    assert_eq!(open(&mut s, &own).unwrap().len(), 1);
    assert_eq!(texts(&s), ["earlier version"]);
    assert!(list(root.path()).is_empty());
}

/// An entry that can't be opened stays on disk, in this run's folder now.
#[test]
fn an_unreadable_entry_of_a_run_that_ended_is_kept() {
    let root = tempfile::tempdir().unwrap();
    let left = run_left(root.path(), "run-7-0", "readable", 1);
    std::fs::write(left.join("2.designcraft"), b"not a document").unwrap();
    let own = run_dir(root.path()).unwrap();
    let mut s = Session::new();
    assert_eq!(open(&mut s, &own).unwrap().len(), 1);
    assert!(list(&own).iter().any(|e| std::fs::read(files(&own, e.0).0).unwrap() == b"not a document"));
    assert!(!left.exists());
}

/// Only a folder named as a run's, with a lock that is free, is a run that ended.
#[test]
fn what_is_no_run_that_ended_is_passed_over() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("run-1-0"), b"a file").unwrap();
    // An app that is starting has made its folder and not yet its lock.
    let starting = root.path().join("run-2-0");
    std::fs::create_dir(&starting).unwrap();
    let other = run_left(root.path(), "backup", "not a run", 1);
    #[cfg(unix)]
    let (_elsewhere, linked) = {
        let elsewhere = tempfile::tempdir().unwrap();
        let linked = run_left(elsewhere.path(), "run-3-0", "behind a link", 1);
        std::os::unix::fs::symlink(&linked, root.path().join("run-3-0")).unwrap();
        (elsewhere, linked)
    };
    let own = run_dir(root.path()).unwrap();
    let mut s = Session::new();
    assert!(open(&mut s, &own).unwrap().is_empty());
    assert!(starting.exists());
    assert_eq!(list(&other).len(), 1);
    #[cfg(unix)]
    assert!(list(&linked).len() == 1 && root.path().join("run-3-0").is_symlink());
}

/// The metadata goes with its document: a document taken over remembers its file.
#[test]
fn a_document_taken_over_remembers_its_file() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("layout.designcraft");
    let recovery = root.path().join("recovery");
    let left = recovery.join("run-7-0");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::write(left.join(LOCK), b"").unwrap();
    let mut a = unsaved("saved once");
    a.execute("file.saveAs", &json!({"path": file.to_string_lossy()})).unwrap();
    a.execute("frame.create", &json!({"rect": [10, 10, 100, 100]})).unwrap();
    assert_eq!(save(&a, &left).unwrap(), 1);
    let own = run_dir(&recovery).unwrap();
    let mut s = Session::new();
    assert_eq!(open(&mut s, &own).unwrap().len(), 1);
    assert_eq!(s.doc().unwrap().path.as_deref(), Some(file.to_string_lossy().as_ref()));
}

/// The folder of a run that ended goes with what the app left in it. A file the app did not
/// write keeps the folder, and is not deleted.
#[test]
fn only_what_the_app_left_goes_with_the_folder_of_a_run_that_ended() {
    let root = tempfile::tempdir().unwrap();
    let emptied = run_left(root.path(), "run-7-0", "moved", 1);
    std::fs::write(emptied.join("9.json"), b"{}").unwrap();
    std::fs::write(emptied.join(".designcraft-recovery-abc.tmp"), b"half written").unwrap();
    let kept = run_left(root.path(), "run-8-0", "moved too", 1);
    std::fs::write(kept.join("copy.designcraft"), b"a copy someone put here").unwrap();
    let own = run_dir(root.path()).unwrap();
    let mut s = Session::new();
    assert_eq!(open(&mut s, &own).unwrap().len(), 2);
    assert!(!emptied.exists());
    assert_eq!(std::fs::read(kept.join("copy.designcraft")).unwrap(), b"a copy someone put here");
    assert!(kept.join(LOCK).exists(), "still the folder of a run that ended");
}

/// A folder that is there is another app's or was left behind, lock or no lock: a run's own
/// is one it made itself.
#[test]
fn a_folder_that_is_there_is_not_made_this_runs() {
    let root = tempfile::tempdir().unwrap();
    let left = root.path().join(format!("run-{}-0", std::process::id()));
    std::fs::create_dir(&left).unwrap();
    std::fs::write(left.join("1.designcraft"), b"left with no lock").unwrap();
    let own = run_dir(root.path()).unwrap();
    assert_ne!(own, left);
    assert!(!left.join(LOCK).exists());
    assert_eq!(std::fs::read(left.join("1.designcraft")).unwrap(), b"left with no lock");
}

/// Another run's entry under uid 7 in a chosen folder, with a document of this run numbered
/// 7 too: document uids start at 1 in every process.
fn chosen_folder_with_another_runs_entry() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let (doc, meta) = files(root.path(), 7);
    std::fs::write(&doc, b"another run's document").unwrap();
    std::fs::write(&meta, b"{\"title\": \"theirs\"}").unwrap();
    (root, doc, meta)
}

#[test]
fn a_clean_document_leaves_another_runs_entry_in_a_chosen_folder() {
    let (root, doc, meta) = chosen_folder_with_another_runs_entry();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.docs[0].uid = 7;
    assert!(!s.docs[0].is_dirty());
    s.execute("file.recovery.save", &json!({"dir": root.path().to_string_lossy()})).unwrap();
    s.execute("file.close", &json!({})).unwrap();
    assert_eq!(std::fs::read(doc).unwrap(), b"another run's document");
    assert_eq!(std::fs::read(meta).unwrap(), b"{\"title\": \"theirs\"}");
}

#[test]
fn a_dirty_document_does_not_overwrite_another_runs_entry_in_a_chosen_folder() {
    let (root, doc, meta) = chosen_folder_with_another_runs_entry();
    let mut s = unsaved("this run's work");
    s.docs[0].uid = 7;
    assert!(s.execute("file.recovery.save", &json!({"dir": root.path().to_string_lossy()})).is_err());
    assert!(save(&s, root.path()).is_err());
    assert_eq!(std::fs::read(doc).unwrap(), b"another run's document");
    assert_eq!(std::fs::read(meta).unwrap(), b"{\"title\": \"theirs\"}");
}

/// A chosen folder that is a running app's run folder is that app's.
#[test]
fn a_folder_another_running_app_holds_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let held = new_lock(root.path()).unwrap();
    held.try_lock().unwrap();
    assert!(save(&dirty_session(), root.path()).is_err());
    assert!(list(root.path()).is_empty());
}
