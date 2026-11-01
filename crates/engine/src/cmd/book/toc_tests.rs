use super::*;
use crate::cmd::toc::book_tests::fixtures::{
    BookFixture, TestResult, chapter, exact_headings, moving_heading_document, set_moving_text, toc_document,
};
use designcraft_doc::{ParaFormat, SpreadRef};
use designcraft_geom::Rect;

fn close(f: &mut BookFixture) -> TestResult {
    f.s.execute("file.close", &json!({}))?;
    Ok(())
}
fn fresh(f: &mut BookFixture) -> TestResult<bool> {
    Ok(f.s.execute("book.tocFreshness", &json!({}))?["fresh"] == true)
}
fn read(f: &BookFixture, name: &str) -> TestResult<Document> {
    Ok(designcraft_format::load(&std::fs::read(f.dir.join(format!("{name}.designcraft")))?)?)
}
fn bytes(f: &BookFixture) -> TestResult<Vec<(PathBuf, Vec<u8>)>> {
    let b = f.s.book.as_ref().ok_or("Book")?;
    let mut paths = vec![PathBuf::from(&b.path)];
    paths.extend(b.documents.iter().map(|n| f.dir.join(n)));
    paths.into_iter().map(|p| Ok((p.clone(), std::fs::read(p)?))).collect()
}

#[test]
fn scenarios_a_b_d_e_f_stabilize_and_persist_with_final_labels() -> TestResult {
    let root = tempfile::tempdir()?;
    for (name, docs, pages, starts, passes) in [
        ("A", vec![("toc", toc_document(160.0, 140.0, false, "\t")), ("head", chapter("Control", 3, 1)?)], vec![1, 1], vec![1, 2], 2),
        (
            "B",
            vec![("toc", toc_document(160.0, 140.0, true, "\t")), ("head", chapter("Growth", 10, 1)?), ("last", chapter("Last", 1, 1)?)],
            vec![2, 1, 1],
            vec![1, 3, 4],
            3,
        ),
        (
            "D",
            vec![
                ("front", chapter("Front", 1, 1)?),
                ("a", chapter("Before", 1, 1)?),
                ("toc", toc_document(160.0, 140.0, true, "\t")),
                ("b", chapter("After", 7, 1)?),
                ("c", chapter("End", 2, 1)?),
            ],
            vec![1, 1, 2, 1, 1],
            vec![1, 2, 3, 5, 6],
            3,
        ),
        (
            "E",
            vec![
                ("toc", toc_document(107.75, 140.0, true, " ")),
                ("padding", chapter("Pad", 0, 7)?),
                ("head", exact_headings("Measured heading", 10)?),
            ],
            vec![3, 7, 1],
            vec![1, 4, 11],
            4,
        ),
        (
            "F",
            vec![
                ("toc", toc_document(113.75, 140.0, true, " ")),
                ("padding", chapter("Pad", 0, 97)?),
                ("head", exact_headings("Measured heading", 10)?),
            ],
            vec![3, 97, 1],
            vec![1, 4, 101],
            4,
        ),
    ] {
        let mut f = BookFixture::new(root.path(), name, docs)?;
        close(&mut f)?;
        let result = f.s.execute("book.refreshToc", &json!({}))?;
        assert_eq!(result["passes"], passes, "{name}: {result}");
        let names = f.s.book.as_ref().ok_or("Book")?.documents.clone();
        for ((file, pages), start) in names.iter().zip(pages).zip(starts) {
            let d = designcraft_format::load(&std::fs::read(f.dir.join(file))?)?;
            assert_eq!(d.page_count(), pages, "{name}");
            assert_eq!(d.page_number(0), start, "{name}");
            d.check()?;
        }
        assert!(fresh(&mut f)?, "{name}: {}", f.s.execute("book.tocFreshness", &json!({}))?);
        let manifest = f.s.book.as_ref().ok_or("Book")?.path.clone();
        f.s.execute("book.close", &json!({}))?;
        f.s.execute("book.open", &json!({"path":manifest}))?;
        assert!(fresh(&mut f)?, "fresh checkpoint survives reload");
    }
    Ok(())
}

#[test]
fn scenario_c_shrinks_only_eligible_primary_pages_and_preserves_fixed_or_unrelated_content() -> TestResult {
    let root = tempfile::tempdir()?;
    for unrelated in [false, true] {
        let mut f = BookFixture::new(
            root.path(),
            if unrelated { "unrelated" } else { "shrink" },
            vec![("toc", toc_document(160.0, 140.0, true, "\t")), ("head", chapter("Growth", 11, 1)?)],
        )?;
        close(&mut f)?;
        f.s.execute("book.refreshToc", &json!({}))?;
        assert_eq!(read(&f, "toc")?.page_count(), 2);
        f.mutate("head", |d| {
            for story in d.stories.values_mut() {
                let n = story.len();
                Arc::make_mut(story).replace(0..n, "Reduced");
            }
            Ok(())
        })?;
        if unrelated {
            f.mutate("toc", |d| {
                d.add_text_frame(SpreadRef::Doc(1), Rect::new(20.0, 20.0, 100.0, 50.0), d.default_layer(), "Keep", ParaFormat::default())?;
                Ok(())
            })?;
        }
        assert!(!fresh(&mut f)?);
        f.s.execute("book.refreshToc", &json!({}))?;
        assert_eq!(read(&f, "toc")?.page_count(), if unrelated { 2 } else { 1 });
        assert!(fresh(&mut f)?);
    }
    let mut fixed = toc_document(160.0, 140.0, false, "\t");
    fixed.insert_pages(Some(0), 1, None)?;
    let mut f = BookFixture::new(root.path(), "fixed", vec![("toc", fixed), ("head", chapter("Small", 1, 1)?)])?;
    close(&mut f)?;
    f.s.execute("book.refreshToc", &json!({}))?;
    assert_eq!(read(&f, "toc")?.page_count(), 2);
    assert_eq!(read(&f, "head")?.page_number(0), 3);
    Ok(())
}

#[test]
fn scenarios_g_h_i_freshness_and_export_reject_stale_even_after_numbering_or_equal_counts() -> TestResult {
    let root = tempfile::tempdir()?;
    for scenario in ["G", "H", "I"] {
        let docs = if scenario == "I" {
            vec![("toc", toc_document(160.0, 140.0, false, "\t")), ("a", moving_heading_document()?)]
        } else {
            vec![("toc", toc_document(160.0, 140.0, false, "\t")), ("a", chapter("Alpha", 1, 1)?), ("b", chapter("Bravo", 1, 1)?)]
        };
        let mut f = BookFixture::new(root.path(), scenario, docs)?;
        close(&mut f)?;
        f.s.execute("book.refreshToc", &json!({}))?;
        assert!(fresh(&mut f)?);
        match scenario {
            "G" => f.mutate("a", |d| {
                d.insert_pages(d.page_count().checked_sub(1), 1, None)?;
                Ok(())
            })?,
            "H" => {
                f.s.execute("book.move", &json!({"from":2,"to":1}))?;
            }
            _ => f.mutate("a", |d| set_moving_text(d, &"Body words fill fixed pages. ".repeat(220)))?,
        }
        assert!(!fresh(&mut f)?);
        let output = f.dir.join("must-not-exist.pdf");
        assert!(f.s.execute("book.exportPdf", &json!({"path":output})).is_err());
        assert!(!output.exists());
        if scenario != "I" {
            f.s.execute("book.paginate", &json!({}))?;
            assert!(!f.s.book.as_ref().ok_or("Book")?.numbering_needs_update);
            assert!(!fresh(&mut f)?);
        }
        f.s.execute("book.refreshToc", &json!({}))?;
        assert!(fresh(&mut f)?);
        let before = bytes(&f)?;
        f.s.execute("book.tocFreshness", &json!({}))?;
        f.s.execute("book.exportPdf", &json!({"path":f.dir.join("fresh.pdf")}))?;
        for (path, old) in before {
            assert_eq!(std::fs::read(path)?, old, "queries/exports do not write source files");
        }
        let d = read(&f, "toc")?;
        let toc = d.toc.as_ref().ok_or("TOC")?;
        let text = &d.story(toc.story).ok_or("story")?.text;
        match scenario {
            "G" => assert!(text.contains("Bravo 01\t4")),
            "H" => assert!(text.starts_with("Bravo 01\t2\nAlpha 01\t3")),
            _ => assert_eq!(text, "Movable 01\t3"),
        };
    }
    Ok(())
}

#[test]
fn failures_preflight_and_source_conflicts_never_write_originals() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut f = BookFixture::new(root.path(), "guard", vec![("toc", toc_document(160.0, 140.0, true, "\t")), ("head", chapter("Growth", 11, 1)?)])?;
    let original = bytes(&f)?;
    assert!(f.s.execute("book.refreshToc", &json!({})).unwrap_err().to_string().contains("close"));
    for (p, b) in &original {
        assert_eq!(&std::fs::read(p)?, b);
    }
    close(&mut f)?;
    let prepared = Prepared::new(&f.s)?;
    f.mutate("head", |d| {
        d.title = "External change".into();
        Ok(())
    })?;
    let conflict = bytes(&f)?;
    assert!(prepared.commit(&mut f.s).unwrap_err().to_string().contains("changed"));
    for (p, b) in conflict {
        assert_eq!(std::fs::read(p)?, b);
    }
    let mut fixed =
        BookFixture::new(root.path(), "overset", vec![("toc", toc_document(160.0, 140.0, false, "\t")), ("head", chapter("Growth", 11, 1)?)])?;
    close(&mut fixed)?;
    let original = bytes(&fixed)?;
    assert!(fixed.s.execute("book.refreshToc", &json!({})).unwrap_err().to_string().contains("does not fit"));
    for (p, b) in original {
        assert_eq!(std::fs::read(p)?, b);
    }
    f.s.book.as_mut().ok_or("Book")?.documents.push("./head.designcraft".into());
    // Persist the alias so the normal manifest-revision check also holds.
    save(f.s.book.as_ref().ok_or("Book")?)?;
    assert!(f.s.execute("book.refreshToc", &json!({})).unwrap_err().to_string().contains("duplicate"));
    Ok(())
}

#[test]
fn provenance_catches_identical_output_reorder_and_definition_change() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut f = BookFixture::new(
        root.path(),
        "same",
        vec![("toc", toc_document(160.0, 140.0, false, "\t")), ("a", chapter("Same", 1, 1)?), ("b", chapter("Same", 1, 1)?)],
    )?;
    close(&mut f)?;
    f.s.execute("book.refreshToc", &json!({}))?;
    let before = read(&f, "toc")?;
    f.s.execute("book.move", &json!({"from":2,"to":1}))?;
    f.s.execute("book.paginate", &json!({}))?;
    assert!(!fresh(&mut f)?);
    f.s.execute("book.refreshToc", &json!({}))?;
    let after = read(&f, "toc")?;
    assert_eq!(
        before.story(before.toc.as_ref().ok_or("TOC")?.story).ok_or("story")?.text,
        after.story(after.toc.as_ref().ok_or("TOC")?.story).ok_or("story")?.text
    );
    f.mutate("toc", |d| {
        let entry = d.toc.as_mut().ok_or("TOC")?.entries.first_mut().ok_or("entry")?;
        entry.level = 2;
        entry.entry_style = Some("TOC Level 1".into());
        Ok(())
    })?;
    assert!(!fresh(&mut f)?);
    Ok(())
}

#[test]
fn four_pass_budget_never_accepts_fourth_changing_or_mismatched_state() {
    let mut c = Confirmation { previous: None, passes: 0 };
    for n in 0..3 {
        assert!(!c.observe(json!({"state":n}), true).unwrap());
    }
    assert!(c.observe(json!({"state":3}), true).unwrap_err().to_string().contains("four passes"));
    let mut c = Confirmation { previous: None, passes: 0 };
    for _ in 0..3 {
        assert!(!c.observe(json!(1), false).unwrap());
    }
    assert!(c.observe(json!(1), false).is_err());
}

#[test]
fn j_parameter_sweep_uses_production_four_pass_operation() -> TestResult {
    let root = tempfile::tempdir()?;
    for height in [100.0, 140.0, 180.0] {
        for count in [6, 8, 10, 12] {
            let name = format!("h{height}n{count}");
            let mut f = BookFixture::new(
                root.path(),
                &name,
                vec![
                    ("toc", toc_document(107.75, height, true, " ")),
                    ("padding", chapter("Pad", 0, 7)?),
                    ("head", exact_headings("Measured heading", count)?),
                ],
            )?;
            close(&mut f)?;
            let result = f.s.execute("book.refreshToc", &json!({}))?;
            assert!(result["passes"].as_u64().is_some_and(|n| n <= 4));
            assert!(fresh(&mut f)?);
        }
    }
    Ok(())
}

#[test]
fn old_book_defaults_current_document_and_no_toc_export_remain_compatible() -> TestResult {
    let root = tempfile::tempdir()?;
    for scope in [None, Some(TocScope::CurrentDocument)] {
        let mut d = toc_document(160.0, 140.0, false, "\t");
        match scope {
            Some(scope) => d.toc.as_mut().ok_or("TOC")?.scope = scope,
            None => d.toc = None,
        };
        let mut f =
            BookFixture::new(root.path(), if scope.is_none() { "none" } else { "local" }, vec![("toc", d), ("head", chapter("Head", 1, 1)?)])?;
        // The existing export path still permits open chapters when no Book TOC is configured.
        assert_eq!(f.s.execute("book.tocFreshness", &json!({}))?, json!({"configured":false,"fresh":true}));
        f.s.execute("book.exportPdf", &json!({"path":f.dir.join("legacy.pdf")}))?;
        close(&mut f)?;
        assert!(f.s.execute("book.refreshToc", &json!({})).unwrap_err().to_string().contains("Entire Book"));
        assert!(f.s.book.as_ref().ok_or("Book")?.toc_state.is_none());
    }
    Ok(())
}

#[test]
fn heading_selection_text_and_generated_content_changes_are_stale() -> TestResult {
    let root = tempfile::tempdir()?;
    for change in ["text", "add", "remove", "style", "definition", "generated"] {
        let mut f =
            BookFixture::new(root.path(), change, vec![("toc", toc_document(160.0, 140.0, false, "\t")), ("head", chapter("Heading", 2, 1)?)])?;
        close(&mut f)?;
        f.s.execute("book.refreshToc", &json!({}))?;
        if change == "definition" {
            f.mutate("toc", |d| {
                d.toc.as_mut().ok_or("TOC")?.title = "New title".into();
                Ok(())
            })?;
        } else if change == "generated" {
            f.mutate("toc", |d| {
                let sid = d.toc.as_ref().ok_or("TOC")?.story;
                let story = d.story_mut(sid).ok_or("story")?;
                story.replace(0..0, "Edited ");
                Ok(())
            })?;
        } else {
            f.mutate("head", |d| {
                let sid = *d.stories.keys().next().ok_or("story")?;
                let story = d.story_mut(sid).ok_or("story")?;
                match change {
                    "text" => {
                        story.replace(0..7, "Renamed");
                    }
                    "add" => {
                        story.replace(story.len()..story.len(), "\nAnother");
                    }
                    "remove" => {
                        story.replace(0..story.len(), "One");
                    }
                    _ => story.format_paras(0..0, |p| p.style = "[Basic Paragraph]".into()),
                };
                Ok(())
            })?;
        }
        assert!(!fresh(&mut f)?, "{change}");
    }
    Ok(())
}

#[test]
fn manifest_conflict_missing_input_open_alias_and_multiple_tocs_abort_without_writes() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut f =
        BookFixture::new(root.path(), "conflict", vec![("toc", toc_document(160.0, 140.0, true, "\t")), ("head", chapter("Growth", 10, 1)?)])?;
    close(&mut f)?;
    let prepared = Prepared::new(&f.s)?;
    let manifest = f.s.book.as_ref().ok_or("Book")?.path.clone();
    let mut other = f.s.book.as_ref().ok_or("Book")?.clone();
    other.style_source = 1;
    save(&other)?;
    let before = bytes(&f)?;
    assert!(prepared.commit(&mut f.s).unwrap_err().to_string().contains("manifest changed"));
    for (p, b) in before {
        assert_eq!(std::fs::read(p)?, b);
    }
    f.s.execute("book.open", &json!({"path":manifest}))?;
    f.s.execute("file.open", &json!({"path":f.dir.join("./head.designcraft")}))?;
    f.s.execute("frame.create", &json!({"rect":[36,36,100,100],"content":"text","text":"Unsaved"}))?;
    let before = bytes(&f)?;
    assert!(f.s.execute("book.refreshToc", &json!({})).unwrap_err().to_string().contains("close"));
    for (p, b) in before {
        assert_eq!(std::fs::read(p)?, b);
    }
    f.s.execute("file.close", &json!({}))?;
    f.s.book.as_mut().ok_or("Book")?.documents.push("missing.designcraft".into());
    save(f.s.book.as_ref().ok_or("Book")?)?;
    assert!(f.s.execute("book.refreshToc", &json!({})).is_err());
    let mut multi = BookFixture::new(
        root.path(),
        "multi",
        vec![("toc", toc_document(160.0, 140.0, true, "\t")), ("other", toc_document(160.0, 140.0, true, "\t")), ("head", chapter("Head", 1, 1)?)],
    )?;
    close(&mut multi)?;
    let before = bytes(&multi)?;
    assert!(multi.s.execute("book.refreshToc", &json!({})).unwrap_err().to_string().contains("multiple"));
    for (p, b) in before {
        assert_eq!(std::fs::read(p)?, b);
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn partial_commit_invalidates_checkpoint_and_reports_completed_prefix() -> TestResult {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir()?;
    let mut f = BookFixture::new(root.path(), "partial", vec![("toc", toc_document(160.0, 140.0, true, "\t")), ("head", chapter("Growth", 11, 1)?)])?;
    close(&mut f)?;
    let prepared = Prepared::new(&f.s)?;
    // Permit read/write preflight while denying atomic replacement of the second affected file.
    let lock = std::fs::OpenOptions::new().read(true).write(true).share_mode(3).open(f.dir.join("head.designcraft"))?;
    let error = prepared.commit(&mut f.s).unwrap_err().to_string();
    drop(lock);
    assert!(error.contains("completed chapter paths"), "{error}");
    assert!(error.contains("toc.designcraft"), "{error}");
    assert_eq!(read(&f, "toc")?.page_count(), 2);
    assert_eq!(read(&f, "head")?.page_number(0), 2);
    assert!(f.s.book.as_ref().ok_or("Book")?.toc_state.is_none());
    assert!(!fresh(&mut f)?);
    let manifest: Book = serde_json::from_slice(&std::fs::read(f.s.book.as_ref().ok_or("Book")?.path.clone())?)?;
    assert!(manifest.toc_state.is_none());
    Ok(())
}

#[test]
fn changing_candidate_trace_exhausts_budget_without_writing_sources() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut f = BookFixture::new(root.path(), "budget", vec![("toc", toc_document(160.0, 140.0, false, "\t")), ("head", chapter("Head", 1, 1)?)])?;
    close(&mut f)?;
    let before = bytes(&f)?;
    let mut snapshot = Snapshot::load(&f.s, true)?;
    let mut confirmation = Confirmation { previous: None, passes: 0 };
    for pass in 1..=PASSES {
        // An adversarial diagnostic trace, not a claim of real static-content oscillation.
        let at = snapshot.toc_index()?;
        snapshot.chapters.get_mut(at).ok_or("TOC")?.doc.toc.as_mut().ok_or("TOC")?.title = format!("Changing title {pass}");
        let (state, labels) = snapshot.pass(&f.s.prefs)?;
        let result = confirmation.observe(state, labels);
        if pass == PASSES {
            assert!(result.unwrap_err().to_string().contains("four passes"));
        } else {
            assert!(!result?);
        }
    }
    for (p, b) in before {
        assert_eq!(std::fs::read(p)?, b);
    }
    Ok(())
}

#[test]
fn two_digit_boundaries_need_fifth_confirmation_and_four_pass_command_leaves_sources_untouched() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut toc = toc_document(107.75, 108.0, true, " ");
    let mut small = toc.styles.para("[Basic Paragraph]").cloned().ok_or("basic style")?;
    small.name = "Small TOC".into();
    small.based_on = Some("[Basic Paragraph]".into());
    small.chars.size = Some(11.35);
    small.para.space_before = Some(4.0);
    toc.styles_mut().paragraph.push(small);
    let mut entry = designcraft_doc::TocEntry::new("Head Two", 1);
    entry.separator = Some(" ".into());
    entry.entry_style = Some("Small TOC".into());
    toc.toc.as_mut().ok_or("TOC")?.entries.push(entry);
    let mut second = exact_headings("Measured heading", 2)?;
    let mut head = second.styles.para("Head").cloned().ok_or("Head")?;
    head.name = "Head Two".into();
    second.styles_mut().paragraph.push(head);
    for story in second.stories.values_mut() {
        for p in &mut Arc::make_mut(story).paras {
            p.style = "Head Two".into();
        }
    }
    let mut f = BookFixture::new(
        root.path(),
        "two-boundaries",
        vec![
            ("toc", toc),
            ("pad-a", chapter("Pad", 0, 7)?),
            ("head-a", exact_headings("Measured heading", 8)?),
            ("pad-b", chapter("Pad", 0, 88)?),
            ("head-b", second),
        ],
    )?;
    let live = f.s.doc()?.doc.clone();
    let undo = f.s.doc()?.history.undo.len();
    let originals = bytes(&f)?;
    let active_error = f.s.execute("toc.update", &json!({})).unwrap_err().to_string();
    assert!(active_error.contains("four passes"), "{active_error}");
    assert!(Arc::ptr_eq(&live, &f.s.doc()?.doc));
    assert_eq!(f.s.doc()?.history.undo.len(), undo);
    assert_eq!(bytes(&f)?, originals);
    close(&mut f)?;
    let before = bytes(&f)?;
    // A diagnostic fifth observation establishes why the public four-pass command must refuse.
    let mut candidate = Snapshot::load(&f.s, true)?;
    let mut states = Vec::new();
    let mut pages = Vec::new();
    for _ in 0..5 {
        let (state, labels) = candidate.pass(&f.s.prefs)?;
        pages.push(candidate.document(candidate.toc_index()?)?.page_count());
        states.push((state, labels));
    }
    assert_eq!(pages, vec![2, 3, 4, 4, 4]);
    assert_ne!(states[2].0, states[3].0);
    assert_eq!(states[3], states[4]);
    let error = f.s.execute("book.refreshToc", &json!({})).unwrap_err().to_string();
    assert!(error.contains("four passes"), "{error}");
    for (p, b) in before {
        assert_eq!(std::fs::read(p)?, b);
    }
    Ok(())
}

#[test]
fn active_generate_growth_uses_live_edits_one_undo_and_save_finalizes_freshness() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut toc = toc_document(107.75, 140.0, true, " ");
    toc.toc = None;
    let mut f = BookFixture::new(
        root.path(),
        "active-generate",
        vec![("toc", toc), ("padding", chapter("Pad", 0, 7)?), ("head", exact_headings("Measured heading", 10)?)],
    )?;
    f.s.execute("frame.create", &json!({"rect":[1,1,18,18],"content":"text","text":"Live edit"}))?;
    let before = f.s.doc()?.doc.clone();
    let undo = f.s.doc()?.history.undo.len();
    let backing = std::fs::read(f.dir.join("toc.designcraft"))?;
    let result = f
        .s
        .execute("toc.generate", &json!({"scope":"entireBook","title":"","replace":false,"entries":[{"style":"Head","level":1,"separator":" "}]}))?;
    assert_eq!(result["passes"], 4);
    assert_eq!(result["pendingSave"], true);
    assert_eq!(result["fresh"], false);
    assert_eq!(f.s.documents().len(), 1);
    assert!(f.s.doc()?.is_dirty());
    assert_eq!(std::fs::read(f.dir.join("toc.designcraft"))?, backing);
    let stable = f.s.doc()?.doc.clone();
    assert_eq!(stable.page_count(), 3);
    assert!(stable.stories.values().any(|s| s.text == "Live edit"));
    assert!(stable.story(stable.toc.as_ref().ok_or("TOC")?.story).ok_or("story")?.text.contains("Measured heading 11"));
    assert_eq!(f.s.doc()?.history.undo.len(), undo + 1);
    assert!(!fresh(&mut f)?);
    assert!(f.s.execute("book.exportPdf", &json!({"path":f.dir.join("unsaved.pdf")})).is_err());
    f.s.execute("edit.undo", &json!({}))?;
    assert!(Arc::ptr_eq(&before, &f.s.doc()?.doc));
    // Saving an undone state must never publish the pending stable checkpoint.
    f.s.execute("file.save", &json!({}))?;
    assert!(f.s.book.as_ref().ok_or("Book")?.toc_state.is_none());
    f.s.execute("edit.redo", &json!({}))?;
    assert!(Arc::ptr_eq(&stable, &f.s.doc()?.doc));
    f.s.execute("file.save", &json!({}))?;
    assert!(!f.s.doc()?.is_dirty());
    close(&mut f)?;
    assert!(fresh(&mut f)?);
    f.s.execute("book.exportPdf", &json!({"path":f.dir.join("fresh.pdf")}))?;
    Ok(())
}

#[test]
fn active_update_reorders_labels_and_rejects_other_open_participants() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut f = BookFixture::new(
        root.path(),
        "active-update",
        vec![("toc", toc_document(160.0, 140.0, true, "\t")), ("a", chapter("Alpha", 8, 1)?), ("b", chapter("Beta", 2, 1)?)],
    )?;
    f.s.execute("toc.update", &json!({}))?;
    let backing = std::fs::read(f.dir.join("toc.designcraft"))?;
    f.s.execute("book.move", &json!({"from":2,"to":1}))?;
    let result = f.s.execute("toc.update", &json!({}))?;
    assert!(result["passes"].as_u64().ok_or("passes")? >= 2);
    let st = f.s.doc()?;
    let text = &st.doc.story(st.doc.toc.as_ref().ok_or("TOC")?.story).ok_or("story")?.text;
    assert!(text.find("Beta").ok_or("beta")? < text.find("Alpha").ok_or("alpha")?);
    assert_eq!(std::fs::read(f.dir.join("toc.designcraft"))?, backing);
    f.s.execute("book.openDocument", &json!({"index":1}))?;
    f.s.set_active(0);
    let before = f.s.doc()?.doc.clone();
    let undo = f.s.doc()?.history.undo.len();
    let sources = bytes(&f)?;
    let error = f.s.execute("toc.update", &json!({})).unwrap_err().to_string();
    assert!(error.contains("Close the other Book documents"), "{error}");
    assert!(Arc::ptr_eq(&before, &f.s.doc()?.doc));
    assert_eq!(f.s.doc()?.history.undo.len(), undo);
    assert_eq!(bytes(&f)?, sources);
    Ok(())
}

#[test]
fn active_capacity_and_nonbacked_fail_without_model_history_or_file_changes() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut f =
        BookFixture::new(root.path(), "active-capacity", vec![("toc", toc_document(50.0, 10.0, false, "\t")), ("head", chapter("Heading", 10, 1)?)])?;
    let before = f.s.doc()?.doc.clone();
    let undo = f.s.doc()?.history.undo.len();
    let sources = bytes(&f)?;
    let error = f.s.execute("toc.update", &json!({})).unwrap_err().to_string();
    assert!(error.contains("does not fit"), "{error}");
    assert!(Arc::ptr_eq(&before, &f.s.doc()?.doc));
    assert_eq!(f.s.doc()?.history.undo.len(), undo);
    assert_eq!(bytes(&f)?, sources);
    f.s.doc_mut()?.path = None;
    let error = f.s.execute("toc.update", &json!({})).unwrap_err().to_string();
    assert!(error.contains("Save this document"), "{error}");
    assert!(Arc::ptr_eq(&before, &f.s.doc()?.doc));
    assert_eq!(bytes(&f)?, sources);
    Ok(())
}

#[test]
fn active_source_or_manifest_conflict_prevents_adoption_and_commit() -> TestResult {
    let root = tempfile::tempdir()?;
    for manifest in [false, true] {
        let mut f = BookFixture::new(
            root.path(),
            &format!("active-conflict-{manifest}"),
            vec![("toc", toc_document(160.0, 140.0, true, "\t")), ("head", chapter("Head", 10, 1)?)],
        )?;
        let st = f.s.doc()?;
        let original = st.doc.clone();
        let undo = st.history.undo.len();
        let snapshot = Snapshot::load_with(&f.s, true, Some(((*st.doc).clone(), st.selection.clone())))?;
        let prepared = Prepared::from_snapshot(&f.s, snapshot)?;
        let path = if manifest { f.dir.join("test.dcbook") } else { f.dir.join("head.designcraft") };
        let mut changed = std::fs::read(&path)?;
        changed.push(b' ');
        std::fs::write(path, changed)?;
        let before = bytes(&f)?;
        assert!(prepared.commit(&mut f.s).is_err());
        assert!(Arc::ptr_eq(&original, &f.s.doc()?.doc));
        assert_eq!(f.s.doc()?.history.undo.len(), undo);
        assert_eq!(bytes(&f)?, before);
    }
    Ok(())
}

fn pending_save_fixture(root: &std::path::Path, name: &str) -> TestResult<BookFixture> {
    let mut f = BookFixture::new(root, name, vec![("toc", toc_document(160.0, 140.0, true, "\t")), ("head", chapter("Head", 2, 1)?)])?;
    f.s.execute("toc.update", &json!({}))?;
    assert!(f.s.pending_book_toc.is_some());
    Ok(f)
}

fn assert_saved_without_freshness(f: &BookFixture) -> TestResult {
    assert!(!f.s.doc()?.is_dirty());
    assert_eq!(value(&read(f, "toc")?)?, value(&f.s.doc()?.doc)?);
    assert!(f.s.book.as_ref().ok_or("Book")?.toc_state.is_none());
    Ok(())
}

#[test]
fn post_save_identity_failure_preserves_saved_document_and_error_cause() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut f = pending_save_fixture(root.path(), "identity-error")?;
    let manifest = f.dir.join("test.dcbook");
    let moved = f.dir.join("moved.dcbook");
    std::fs::rename(&manifest, &moved)?;
    let cause = identity(manifest.to_str().ok_or("path")?).err().ok_or("identity should fail")?.to_string();
    let error = f.s.execute("file.save", &json!({})).expect_err("freshness cannot identify the missing Book");
    assert!(matches!(error, EngineError::BadParams { ref cmd, .. } if cmd == "file.save"));
    let message = error.to_string();
    assert!(message.contains("Document saved, but Book TOC freshness was not recorded:"), "{message}");
    assert!(message.contains(&cause), "{message}");
    assert_saved_without_freshness(&f)?;
    let book: Value = serde_json::from_slice(&std::fs::read(moved)?)?;
    assert!(book.get("tocState").is_none_or(Value::is_null));
    Ok(())
}

#[test]
fn post_save_snapshot_conflict_preserves_saved_document_and_error_cause() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut f = pending_save_fixture(root.path(), "verification-error")?;
    // Save normally, then reproduce an external change between snapshot loading and final verification.
    let pending = f.s.pending_book_toc.take().ok_or("pending confirmation")?;
    f.s.execute("file.save", &json!({}))?;
    let st = f.s.doc()?;
    let snapshot = Snapshot::load_with(&f.s, false, Some(((*st.doc).clone(), st.selection.clone())))?;
    f.s.pending_book_toc = Some(pending);
    let path = f.dir.join("head.designcraft");
    let mut changed = std::fs::read(&path)?;
    changed.push(b' ');
    std::fs::write(&path, &changed)?;
    let expected = f.s.pending_book_toc.as_ref().ok_or("pending")?.state.clone();
    let error = publish_saved_toc(&mut f.s, snapshot, expected).expect_err("changed source cannot publish freshness");
    let message = error.to_string();
    assert!(message.contains("Document saved, but Book TOC freshness was not recorded:"), "{message}");
    assert!(message.contains("Book source") && message.contains("head.designcraft") && message.contains("changed during processing"), "{message}");
    assert_saved_without_freshness(&f)?;
    assert_eq!(std::fs::read(path)?, changed);
    let book: Value = serde_json::from_slice(&std::fs::read(f.dir.join("test.dcbook"))?)?;
    assert!(book.get("tocState").is_none_or(Value::is_null));
    Ok(())
}

#[test]
fn post_save_nonmatching_pending_confirmation_remains_a_noop() -> TestResult {
    let root = tempfile::tempdir()?;
    for case in ["no-pending", "other-document", "other-book"] {
        let mut f = pending_save_fixture(root.path(), case)?;
        match case {
            "no-pending" => {
                f.s.pending_book_toc = None;
            }
            "other-document" => {
                f.s.pending_book_toc.as_mut().ok_or("pending")?.uid += 1;
            }
            _ => {
                let other = f.dir.join("other.dcbook");
                std::fs::copy(f.dir.join("test.dcbook"), &other)?;
                f.s.book.as_mut().ok_or("Book")?.path = other.to_string_lossy().into_owned();
            }
        }
        let before = bytes(&f)?;
        f.s.execute("file.save", &json!({}))?;
        assert_saved_without_freshness(&f)?;
        assert_eq!(std::fs::read(&before[0].0)?, before[0].1, "{case}: Book must not be rewritten");
    }
    Ok(())
}
