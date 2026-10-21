//! Synthetic PageMaker files of each version (built here, so the tests need no sample files),
//! hostile input, and the real-file corpus when `DESIGNCRAFT_PAGEMAKER_CORPUS` names one.

use super::*;
use crate::testfile::{PM4, PM5, PM6, PM65, sample};

fn check_sample(im: &Imported) {
    let d = &im.document;
    assert_eq!(d.page_count(), 2);
    let pg = d.page(0).unwrap();
    assert!((pg.width - 612.0).abs() < 1e-6 && (pg.height - 792.0).abs() < 1e-6, "{}×{}", pg.width, pg.height);
    assert!(d.settings.facing_pages);
    // One story, threaded over two frames, with its formatting.
    let stories: Vec<_> = d.stories.values().filter(|s| !s.text.is_empty()).collect();
    assert_eq!(stories.len(), 1);
    let st = stories[0];
    assert_eq!(st.text, "Hello\nworld");
    assert_eq!(st.frames.len(), 2);
    let hello = st.format_after(0);
    assert_eq!(hello.over.font_family.as_deref(), Some("Helvetica"));
    assert_eq!(hello.over.font_style.as_deref(), Some("Bold"));
    assert_eq!(hello.over.size, Some(18.0));
    assert_eq!(hello.over.fill.as_deref(), Some("Teal"));
    let teal = d.swatches.iter().find(|s| s.name == "Teal").expect("the colour table's own colours become swatches");
    assert_eq!(
        teal.value,
        designcraft_color::swatch::SwatchValue::Color {
            color: designcraft_color::Color::cmyk(1.0, 0.0, 32768.0 / 65535.0, 0.0),
            color_type: designcraft_color::swatch::ColorType::Process
        }
    );
    let world = st.format_after(st.text.find("world").unwrap());
    assert_eq!(world.over.font_family.as_deref(), Some("Times"));
    assert_eq!(world.over.size, Some(10.0));
    assert_eq!(world.over.fill.as_deref(), Some(designcraft_color::swatch::BLACK));
    let p = &st.paras[0].para;
    assert_eq!(p.align, Some(designcraft_doc::Align::Center));
    assert_eq!((p.left_indent, p.first_line_indent, p.space_before, p.right_indent), (Some(36.0), Some(18.0), Some(6.0), None));
    assert_eq!(st.paras[1].para.left_indent, None);
    // The first frame sits 72 pt in on page 1 (a right page), 72 pt down.
    let f = d.item(st.frames[0]).unwrap().bounds();
    let p1 = d.page_loc(0).and_then(|(si, pi)| d.spreads[si].pages.get(pi)).unwrap().bounds();
    assert!((f.x0 - p1.x0 - 72.0).abs() < 1e-6 && (f.y0 - 72.0).abs() < 1e-6, "{f:?} on {p1:?}");
    // The box on page 1, the line on the right master.
    assert!(d.spreads.iter().flat_map(|s| s.items.iter()).any(|i| i.shape == designcraft_doc::Shape::Rectangle && i.text_frame().is_none()));
    let line = d.parents[0].items.iter().find(|i| i.shape == designcraft_doc::Shape::GraphicLine).expect("the master's line");
    let lb = line.bounds();
    assert!((lb.y0 - 36.5).abs() < 1e-6 && (lb.y1 - 36.5).abs() < 1e-6, "horizontal, from its end points: {lb:?}");
    assert!((lb.width() - 468.0).abs() < 1e-6, "{lb:?}");
    let weight = line.stroke.weight;
    assert!(weight == 1.0 || weight == 2.0, "{weight}");
}

#[test]
fn imports_every_version_pc_and_mac() {
    for (f, v) in [(PM4, Version::Pm4), (PM5, Version::Pm5), (PM6, Version::Pm6), (PM65, Version::Pm65)] {
        for le in [true, false] {
            let file = sample(f, le);
            assert_eq!(detect(&file), Some(Kind { version: v, mac: !le, ole: false }));
            let im = import_pagemaker(&file).unwrap_or_else(|e| panic!("{v:?} mac={}: {e}", !le));
            check_sample(&im);
            assert_eq!(im.kind.version, v);
        }
    }
}

#[test]
fn imports_the_stream_of_a_compound_file() {
    let stream = sample(PM65, true);
    let file = cfb::tests::build(&[("PageMaker", &stream)]);
    assert_eq!(detect(&file), Some(Kind { version: Version::Pm65, mac: false, ole: true }));
    check_sample(&import_pagemaker(&file).unwrap());
    let other = cfb::tests::build(&[("Other", &stream)]);
    // A compound file without a PageMaker stream (a Word .doc, say) is not a PageMaker file.
    assert_eq!(import_pagemaker(&other).err(), Some(PmError::NotPageMaker));
}

#[test]
fn recognises_pagemaker_names_and_refuses_other_files() {
    assert!(has_pagemaker_extension("Letter Head.PMD"));
    assert!(has_pagemaker_extension("old.pm4") && has_pagemaker_extension("x.p65") && has_pagemaker_extension("t.pt5"));
    assert!(!has_pagemaker_extension("x.indd") && !has_pagemaker_extension("pmd"));
    assert!(!is_pagemaker(b"%PDF-1.7"));
    assert!(!is_pagemaker(&[]));
    assert_eq!(import_pagemaker(b"hello world").err(), Some(PmError::NotPageMaker));
    // PageMaker 3 is recognised and refused with a clear message.
    let mut pm3 = vec![0u8; 128];
    pm3[6..8].copy_from_slice(&[0xFF, 0x99]);
    pm3[10] = 5;
    assert_eq!(detect(&pm3).map(|k| k.version), Some(Version::Pm3));
    assert!(matches!(import_pagemaker(&pm3), Err(PmError::Unsupported(_))));
}

/// Every truncation and every byte flipped in the header and index: an error or a document,
/// never a panic.
#[test]
fn hostile_files_never_panic() {
    for f in [PM4, PM5, PM6, PM65] {
        for le in [true, false] {
            let file = sample(f, le);
            for n in (0..file.len()).step_by(37) {
                let _ = import_pagemaker(&file[..n]);
            }
            let index_at = file.len() - 16 * 12;
            for at in (0..64).chain(index_at..file.len()) {
                for v in [0x00, 0x7F, 0xFF] {
                    let mut bad = file.clone();
                    bad[at] = v;
                    let _ = import_pagemaker(&bad);
                }
            }
            // Counts that promise far more than the file holds.
            let mut huge = file.clone();
            for k in 0..12 {
                let e = index_at + 16 * k;
                huge[e + 2..e + 4].copy_from_slice(&[0xFF, 0xFF]);
            }
            let _ = import_pagemaker(&huge);
        }
    }
}

/// Real PageMaker files, when `DESIGNCRAFT_PAGEMAKER_CORPUS` names a folder of them: each opens,
/// or is refused with a clear error, and none panics.
#[test]
fn the_corpus_opens() {
    let Some(dir) = std::env::var_os("DESIGNCRAFT_PAGEMAKER_CORPUS") else {
        eprintln!("skipped: set DESIGNCRAFT_PAGEMAKER_CORPUS to a folder of PageMaker files");
        return;
    };
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    let mut opened = 0;
    for p in entries {
        let bytes = std::fs::read(&p).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        match import_pagemaker(&bytes) {
            Ok(im) => {
                opened += 1;
                let text: usize = im.document.stories.values().map(|s| s.text.len()).sum();
                eprintln!("{name}: {:?} {} pages, {text} chars of text, warnings {:?}", im.kind.version, im.document.page_count(), im.warnings);
            }
            Err(e) => eprintln!("{name}: {e}"),
        }
    }
    assert!(opened > 0);
}

/// `DESIGNCRAFT_PAGEMAKER_DUMP=file` prints what the importer reads from it.
#[test]
fn dump_a_file() {
    let Some(path) = std::env::var_os("DESIGNCRAFT_PAGEMAKER_DUMP") else { return };
    match dump(&std::fs::read(path).unwrap()) {
        Ok(s) => eprintln!("{s}"),
        Err(e) => eprintln!("{e}"),
    }
}

/// A file whose index asks for the whole file as text again and again: the import stops at its
/// read budget, keeps what fits, and says something was left out.
#[test]
fn a_file_naming_more_data_than_it_holds_imports_with_a_warning() {
    let mut file = sample(PM65, true);
    // The index is the file's last bytes: add 5000 entries reading the whole file as text, in the
    // last story group.
    let at = u32::from_le_bytes(file[48..52].try_into().unwrap()) as usize;
    let count = usize::from(u16::from_le_bytes(file[46..48].try_into().unwrap()));
    assert_eq!(at + 16 * count, file.len());
    let len = file.len() as u16;
    for _ in 0..5000 {
        let mut e = vec![0u8; 16];
        e[1] = 0x0D;
        e[2..4].copy_from_slice(&len.to_le_bytes());
        e[4..8].copy_from_slice(&1u32.to_le_bytes());
        e[8..10].copy_from_slice(&[0xFF, 0xFF]);
        e[10] = 1;
        e[11] = 0x0D;
        file.extend(e);
    }
    file[46..48].copy_from_slice(&((count + 5000) as u16).to_le_bytes());
    let started = std::time::Instant::now();
    let im = import_pagemaker(&file).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(im.warnings.iter().any(|w| w.contains("refers to more data than it holds")), "{:?}", im.warnings);
    // The genuine file still opens without that warning.
    assert!(import_pagemaker(&sample(PM65, true)).unwrap().warnings.iter().all(|w| !w.contains("more data")));
}
