// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Pinned binary fixtures only; no scoring exports are used to identify overset stories.
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn fixture(variable: &str, name: &str) -> Result<PathBuf> {
    std::env::var(variable)?
        .split(':')
        .map(|folder| Path::new(folder).join(name))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("missing {name}").into())
}

#[test]
#[ignore = "needs PUB_REAL_CORPUS and PUB_CORPUS with the pinned fixtures"]
fn newsletter_overset_stories_are_notes_once_per_chain_and_normal_text_is_not_reported() -> Result<()> {
    let imported = designcraft_pub::import_pub(&std::fs::read(fixture("PUB_REAL_CORPUS", "SampleNewsletter.pub")?)?)?;
    let notes: Vec<_> = imported.notes.iter().filter(|n| n.contains("Some text in this frame does not fit")).collect();
    // Which stories overflow depends on the fonts installed (Publisher's are usually missing):
    // the stories that can, each reported once for its whole chain of frames.
    let stories = [
        "Living and Learning in Class X",
        "What it means to be an American",
        "Our Excursion to the Zoo",
        "Strictly Jazz",
        "This week in class",
        "On Monday",
        "This week’s reminders",
    ];
    assert!(!notes.is_empty() && notes.len() <= stories.len(), "{notes:?}");
    for note in &notes {
        assert!(note.starts_with("Page 1,"), "{note}");
        assert_eq!(stories.iter().filter(|b| note.contains(**b)).count(), 1, "a known story: {note}");
    }
    for beginning in stories {
        assert!(notes.iter().filter(|n| n.contains(beginning)).count() <= 1, "one note per story, not per threaded frame: {notes:?}");
    }
    assert!(!imported.warnings.iter().any(|w| w.contains("Some text in this frame does not fit")));
    let normal = designcraft_pub::import_pub(&std::fs::read(fixture("PUB_CORPUS", "text__base.pub")?)?)?;
    assert!(!normal.notes.iter().any(|n| n.contains("Some text in this frame does not fit")));
    let only_notes = designcraft_pub::import_pub(&std::fs::read(fixture("PUB_CORPUS", "text__size-72.pub")?)?)?;
    assert!(only_notes.warnings.is_empty(), "overset must not become a warning: {:?}", only_notes.warnings);
    assert_eq!(only_notes.notes.iter().filter(|n| n.contains("Some text in this frame does not fit")).count(), 1);
    Ok(())
}
