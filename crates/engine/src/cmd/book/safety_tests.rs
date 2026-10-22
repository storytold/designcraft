use super::{ChapterBatch, MAX_BATCH_CHAPTERS};
use crate::Session;
use serde_json::json;

#[test]
fn a_commit_failure_reports_the_completed_prefix_and_cleans_staging() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.designcraft");
    let second = dir.path().join("second.designcraft");
    let third = dir.path().join("third.designcraft");
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let old = (*s.doc().unwrap().doc).clone();
    let mut changed = old.clone();
    changed.sections.first_mut().unwrap().start_number = Some(9);
    let old_bytes = designcraft_format::save(&old).unwrap();
    for path in [&first, &second, &third] {
        std::fs::write(path, &old_bytes).unwrap();
    }
    let mut batch = ChapterBatch::new(3, "book.paginate").unwrap();
    for path in [&first, &second, &third] {
        batch.stage(path.to_str().unwrap(), &changed).unwrap();
    }
    let staging = batch.directory.path().to_path_buf();
    let mut calls = 0;
    let error = batch
        .commit_with(|path, staged| {
            calls += 1;
            if path == second { Err(std::io::Error::other("synthetic disk full")) } else { crate::io::replace_with(path, staged) }
        })
        .unwrap_err()
        .to_string();
    assert_eq!(calls, 2);
    assert!(error.contains("synthetic disk full"), "{error}");
    assert!(error.contains(second.to_str().unwrap()), "{error}");
    let completed = error.split("completed chapter paths:").nth(1).unwrap();
    // The completed list uses debug formatting, which escapes Windows path separators.
    assert_eq!(completed.trim(), format!("{:?}", [first.to_str().unwrap()]), "{error}");
    assert_eq!(designcraft_format::load(&std::fs::read(first).unwrap()).unwrap().sections.first().unwrap().start_number, Some(9));
    assert_eq!(std::fs::read(second).unwrap(), old_bytes);
    assert_eq!(std::fs::read(third).unwrap(), old_bytes);
    assert!(!staging.exists(), "temporary chapter snapshots must be removed on a partial failure");
}

#[test]
fn chapter_record_limit_is_checked_before_staging_allocation() {
    let error = match ChapterBatch::new(MAX_BATCH_CHAPTERS + 1, "book.paginate") {
        Err(e) => e,
        Ok(_) => panic!("oversized book batch accepted"),
    };
    assert!(error.to_string().contains("4096"));
}
