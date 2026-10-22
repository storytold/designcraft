//! Real Affinity documents, local only: any folder of `.af`/`.afdesign`/`.afphoto`/`.afpub`
//! files named by `AFFINITY_SAMPLES` (searched recursively). Without it these tests skip; no
//! Affinity documents are committed or fetched.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use designcraft_affinity::{Archive, Limits, stream};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| ["af", "afdesign", "afphoto", "afpub", "aftemplate"].contains(&e)) {
            out.push(p);
        }
    }
}

fn samples() -> Option<PathBuf> {
    std::env::var_os("AFFINITY_SAMPLES").map(PathBuf::from)
}

#[test]
fn public_documents_parse_completely() {
    let Some(dir) = samples() else { return };
    let mut paths = Vec::new();
    files(&dir, &mut paths);
    assert!(!paths.is_empty(), "no Affinity documents under {dir:?}");
    let mut failures = Vec::new();
    for p in &paths {
        let bytes = std::fs::read(p).unwrap();
        let result = Archive::open(&bytes, Limits::default()).and_then(|mut a| {
            let doc = a.read("doc.dat")?;
            stream::parse(&doc).map(|s| s.objects.len())
        });
        match result {
            Ok(n) => assert!(n > 0),
            Err(e) => failures.push(format!("{}: {e}", p.display())),
        }
    }
    assert!(failures.is_empty(), "{} of {} failed:\n{}", failures.len(), paths.len(), failures.join("\n"));
    eprintln!("{} Affinity documents parsed", paths.len());
}

#[test]
fn public_documents_map_to_the_model() {
    let Some(dir) = samples() else { return };
    let mut paths = Vec::new();
    files(&dir, &mut paths);
    assert!(!paths.is_empty(), "no Affinity documents under {dir:?}");
    let mut warnings = std::collections::BTreeMap::<String, usize>::new();
    let mut failures = Vec::new();
    let (mut nodes, mut docs) = (0usize, 0usize);
    fn count(n: &[designcraft_affinity::model::Node]) -> usize {
        n.iter().map(|n| 1 + count(&n.children)).sum()
    }
    for p in &paths {
        let bytes = std::fs::read(p).unwrap();
        match designcraft_affinity::read(&bytes, Limits::default()) {
            Ok(d) => {
                docs += 1;
                nodes += d.spreads.iter().map(|s| count(&s.nodes)).sum::<usize>();
                for w in d.warnings {
                    let key = w.split(" (").next().unwrap_or(&w).to_string();
                    *warnings.entry(key).or_default() += 1;
                }
            }
            Err(e) => failures.push(format!("{}: {e}", p.display())),
        }
    }
    eprintln!("{docs} documents, {nodes} nodes");
    for (w, n) in &warnings {
        eprintln!("{n:4} documents: {w}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every local document imports into a valid DesignCraft document, or fails with a reason.
#[test]
fn local_documents_import_into_designcraft() {
    let Some(dir) = samples() else { return };
    let mut paths = Vec::new();
    files(&dir, &mut paths);
    let mut warnings = std::collections::BTreeMap::<String, usize>::new();
    let (mut ok, mut pages, mut errors) = (0usize, 0usize, Vec::new());
    for p in &paths {
        let bytes = std::fs::read(p).unwrap();
        match designcraft_affinity::import(&bytes) {
            Ok(imported) => {
                imported.document.check().unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                ok += 1;
                pages += imported.document.page_count();
                for w in imported.warnings {
                    let key = w.split(" (").next().unwrap_or(&w).to_string();
                    *warnings.entry(key).or_default() += 1;
                }
            }
            // A safety limit is an honest refusal; anything else is a failure to read the file.
            Err(designcraft_affinity::Error::Limit(why) | designcraft_affinity::Error::Unsupported(why)) => {
                eprintln!("{}: safety limit: {why}", p.display())
            }
            Err(e) => errors.push(format!("{}: {e}", p.display())),
        }
    }
    eprintln!("{ok} of {} documents imported, {pages} pages", paths.len());
    for (w, n) in &warnings {
        eprintln!("{n:4} documents: {w}");
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
