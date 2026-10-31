use super::{Book, ChapterBatch, MAX_BATCH_CHAPTERS, outside_book_folder};
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

/// A book's folder is taken from the path it was stored with, which `book.new` does not resolve.
/// A relative book path left the folder relative while every chapter resolves absolute, so the
/// comparison mixed the two forms (noted in review of #1597).
#[test]
fn a_relative_book_path_still_knows_which_chapters_are_beside_it() {
    let here = std::env::current_dir().unwrap();
    let beside = here.join("sub").join("ch1.designcraft").to_string_lossy().into_owned();
    let elsewhere = here.join("other").join("mine.designcraft").to_string_lossy().into_owned();

    // `dir/Guide.dcbook`: the folder was the relative "sub", which no absolute chapter starts
    // with, so every chapter was reported as outside the book's own folder.
    let b = Book { path: "sub/Guide.dcbook".into(), documents: vec![beside.clone(), elsewhere.clone()], style_source: 0 };
    assert_eq!(outside_book_folder(&b), vec![elsewhere.clone()], "only the chapter that really is elsewhere");

    // A bare name: the folder was empty, and every absolute path starts with an empty path, so
    // nothing was reported however far outside it lived. The chapter has to be outside the
    // working directory itself to test this -- `here/other/x` is inside `here`, and reporting it
    // would be wrong.
    //
    // A sibling of the working directory, not `temp_dir()`: that honours `TMPDIR`, and a `TMPDIR`
    // inside the working directory would leave the chapter inside the book's folder and fail the
    // test for a reason that has nothing to do with the code (noted in review).
    let outside = here.parent().expect("the working directory has a parent").join("dc-book-away");
    let away = outside.join("mine.designcraft").to_string_lossy().into_owned();
    let b = Book { path: "Guide.dcbook".into(), documents: vec![beside.clone(), away.clone()], style_source: 0 };
    assert_eq!(outside_book_folder(&b), vec![away.clone()], "the chapter outside the working directory is reported");

    // `..` in the book's path: `std::path::absolute` keeps it while the chapters' resolution
    // folds it, so the folder was `here/sub/..` -- which `here/sub/ch1.designcraft` does not
    // start with, and every chapter was reported as outside (noted in review).
    let b = Book { path: "sub/../sub/Guide.dcbook".into(), documents: vec![beside.clone(), elsewhere.clone()], style_source: 0 };
    assert_eq!(outside_book_folder(&b), vec![elsewhere.clone()], "the folder is folded as the chapters are");

    // And one that climbs out of the working directory: the chapter beside the book is now the
    // one outside, so the comparison is following the book rather than the working directory.
    let b = Book { path: "../dc-book-away/Guide.dcbook".into(), documents: vec![beside.clone(), away], style_source: 0 };
    assert_eq!(outside_book_folder(&b), vec![beside], "the book's own folder is what counts");
}
