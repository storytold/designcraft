//! Each test owns its database and font folder; no test changes the process-wide database.

use super::*;

#[test]
fn each_atomic_face_batch_advances_once_but_duplicates_and_invalid_fonts_do_not() {
    let db = FontDb::with_font_dirs(Vec::new());
    let before = db.composition_epoch().unwrap();
    assert_eq!(db.add_font(b"not a font".to_vec()), 0);
    assert_eq!(db.composition_epoch(), Some(before));

    let regular = renamed("SourceSans3-Regular.ttf");
    assert_eq!(db.add_font(regular.clone()), 1);
    assert_eq!(db.composition_epoch(), Some(before + 1));
    assert_eq!(db.add_font(regular.clone()), 0);
    assert_eq!(db.composition_epoch(), Some(before + 1));

    let fonts = collection(&[regular, renamed("SourceSans3-Bold.ttf"), renamed("SourceSans3-Semibold.ttf")]);
    assert_eq!(db.add_font(fonts), 2);
    assert_eq!(db.composition_epoch(), Some(before + 2), "a collection publishes its new faces atomically");
    assert_eq!(db.face(FAMILY, "Bold").style, "Bold");
    assert_eq!(db.composition_epoch(), Some(before + 2), "an already loaded face is read-only");
}

#[test]
fn initial_catalog_and_lazy_face_publication_each_advance_the_epoch() {
    let dir = font_dir("epoch-lazy");
    let db = FontDb::with_font_dirs(vec![dir.clone()]);
    let before = db.composition_epoch().unwrap();
    assert_eq!(db.styles(FAMILY), ["Regular", "Bold"]);
    assert_eq!(db.composition_epoch(), Some(before + 1), "the first catalog is published");
    assert!(!db.is_loaded(FAMILY));

    let bold = db.face(FAMILY, "Bold");
    assert_eq!((bold.family.as_str(), bold.style.as_str()), (FAMILY, "Bold"));
    assert_eq!(db.composition_epoch(), Some(before + 2), "the cataloged family publishes all its faces in one batch");
    assert!(Arc::ptr_eq(&bold, &db.face(FAMILY, "Bold")));
    assert_eq!(db.face(FAMILY, "Regular").style, "Regular");
    assert!(db.has_family(FAMILY));
    assert!(has(&db.families(), FAMILY));
    assert_eq!(db.composition_epoch(), Some(before + 2), "ordinary resolved lookups do not invalidate reuse");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rescans_advance_the_epoch_even_when_the_catalog_is_unchanged_or_becomes_empty() {
    let dir = font_dir("epoch-rescan");
    let db = FontDb::with_font_dirs(vec![dir.clone()]);
    let before = db.composition_epoch().unwrap();
    assert_eq!(db.load_system_fonts(), 2);
    assert_eq!(db.composition_epoch(), Some(before + 1));
    assert_eq!(db.load_system_fonts(), 2);
    assert_eq!(db.composition_epoch(), Some(before + 2));
    std::fs::remove_file(dir.join("Sysfont-Regular.ttf")).unwrap();
    std::fs::remove_file(dir.join("Sub/Sysfont-Bold.TTF")).unwrap();
    assert_eq!(db.load_system_fonts(), 0);
    assert_eq!(db.composition_epoch(), Some(before + 3));
    assert!(!db.has_family(FAMILY));
    assert_eq!(db.composition_epoch(), Some(before + 3));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn system_fallback_setting_changes_advance_the_epoch_only_when_the_value_changes() {
    let db = FontDb::with_font_dirs(Vec::new());
    let before = db.composition_epoch().unwrap();
    db.set_system_fallback(true);
    assert_eq!(db.composition_epoch(), Some(before));
    db.set_system_fallback(false);
    assert_eq!(db.composition_epoch(), Some(before + 1));
    db.set_system_fallback(false);
    assert_eq!(db.composition_epoch(), Some(before + 1));
    db.set_system_fallback(true);
    assert_eq!(db.composition_epoch(), Some(before + 2));
}

#[test]
fn new_system_fallback_misses_advance_the_epoch_but_remembered_misses_do_not() {
    let db = FontDb::with_font_dirs(Vec::new());
    assert_eq!(db.load_system_fonts(), 0);
    let before = db.composition_epoch().unwrap();
    // Unicode noncharacters have no fallback in the bundled fonts or optional craft-fonts.
    assert!(db.fallback_for('\u{10ffff}', 0, None).is_none());
    assert_eq!(db.composition_epoch(), Some(before + 1));
    assert!(db.fallback_for('\u{10ffff}', 0, None).is_none());
    assert_eq!(db.composition_epoch(), Some(before + 1));
    assert!(!db.system_fallback('\n'));
    assert!(!db.system_fallback(' '));
    assert_eq!(db.composition_epoch(), Some(before + 1), "ignored characters do not add miss state");

    db.set_system_fallback(false);
    let disabled = db.composition_epoch().unwrap();
    assert!(db.fallback_for('\u{10fffe}', 0, None).is_none());
    assert_eq!(db.composition_epoch(), Some(disabled), "disabled fallback does not remember a new miss");
    db.set_system_fallback(true);
    assert!(db.fallback_for('\u{10fffe}', 0, None).is_none());
    assert_eq!(db.composition_epoch(), Some(disabled + 2), "reenabling and the new miss both advance the epoch");
}

#[test]
fn cjk_chain_attempts_advance_once_and_disabled_attempts_do_not_change_state() {
    const MISSING: &str = "No Such Epoch Chain Family";
    const DISABLED: &str = "No Such Disabled Epoch Chain Family";
    let db = FontDb::with_font_dirs(Vec::new());
    assert_eq!(db.load_system_fonts(), 0);
    let before = db.composition_epoch().unwrap();
    let fonts = db.scoped(0);
    assert!(fonts.chain_face(MISSING, '一', 0).is_none());
    assert_eq!(db.composition_epoch(), Some(before + 1));
    assert!(fonts.chain_face(MISSING, '一', 0).is_none());
    assert_eq!(db.composition_epoch(), Some(before + 1));

    db.set_system_fallback(false);
    assert!(fonts.chain_face(DISABLED, '一', 0).is_none());
    assert_eq!(db.composition_epoch(), Some(before + 2));
    assert!(!db.sys.lock().unwrap().chain_tried.contains(DISABLED));
    db.set_system_fallback(true);
    assert!(fonts.chain_face(DISABLED, '一', 0).is_none());
    assert_eq!(db.composition_epoch(), Some(before + 4));
}

#[test]
fn document_scope_publication_close_and_cached_reopen_each_advance_once() {
    use crate::testing::{font_with_glyph, with_table_u16};

    const SCOPED: &str = "Epoch Scoped Metrics";
    let root = font_dir("epoch-scopes");
    let (a, b) = (root.join("a"), root.join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let font = |glyph, ascent| {
        let bytes = font_with_glyph(SCOPED, &['x'], glyph).unwrap();
        let bytes = with_table_u16(bytes, b"hhea", 4, ascent).unwrap();
        with_table_u16(bytes, b"OS/2", 68, ascent).unwrap()
    };
    std::fs::write(a.join("font.ttf"), font('i', 700)).unwrap();
    std::fs::write(b.join("font.ttf"), font('W', 900)).unwrap();
    let db = FontDb::with_font_dirs(Vec::new());
    assert_eq!(db.add_font(font('m', 800)), 1);
    let shared = db.face(SCOPED, "Regular");
    let before = db.composition_epoch().unwrap();

    // Reading/caching a document file does not publish it into any lookup scope.
    let path = a.join("font.ttf");
    let meta = std::fs::metadata(&path).unwrap();
    let (cached, _) = db.document_file(&path, meta.len(), meta.modified().ok(), MAX_DOCUMENT_FONTS_TOTAL).unwrap();
    assert_eq!(db.composition_epoch(), Some(before));
    assert!(Arc::ptr_eq(&db.face(SCOPED, "Regular"), &shared));

    let scope_a = db.load_document_fonts(&a);
    assert_eq!(scope_a.faces, 1);
    assert_eq!(db.composition_epoch(), Some(before + 1));
    let scope_b = db.load_document_fonts(&b);
    assert_eq!(scope_b.faces, 1);
    assert_eq!(db.composition_epoch(), Some(before + 2));
    let face_a = db.scoped(scope_a.scope).face(SCOPED, "Regular");
    let face_b = db.scoped(scope_b.scope).face(SCOPED, "Regular");
    assert!(Arc::ptr_eq(&face_a, &cached[0]));
    assert_ne!(face_a.id(), face_b.id());
    assert_eq!((face_a.ascent, shared.ascent, face_b.ascent), (700.0, 800.0, 900.0));
    assert!(face_a.advance(face_a.glyph_for('x')) < face_b.advance(face_b.glyph_for('x')));
    assert!(Arc::ptr_eq(&db.face(SCOPED, "Regular"), &shared));
    assert_eq!(db.composition_epoch(), Some(before + 2), "resolved scoped lookups are read-only");

    db.close_scope(scope_a.scope);
    assert_eq!(db.composition_epoch(), Some(before + 3));
    assert!(Arc::ptr_eq(&db.scoped(scope_a.scope).face(SCOPED, "Regular"), &shared));
    assert!(Arc::ptr_eq(&db.scoped(scope_b.scope).face(SCOPED, "Regular"), &face_b));
    db.close_scope(scope_a.scope);
    db.close_scope(0);
    assert_eq!(db.composition_epoch(), Some(before + 3), "closing an absent scope changes nothing");

    let reopened = db.load_document_fonts(&a);
    assert_ne!(reopened.scope, scope_a.scope);
    assert_eq!(db.composition_epoch(), Some(before + 4));
    assert!(Arc::ptr_eq(&db.scoped(reopened.scope).face(SCOPED, "Regular"), &face_a));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_unresolved_primary_lookup_advances_the_epoch_even_for_metric_only_use() {
    let db = FontDb::with_font_dirs(Vec::new());
    assert_eq!(db.load_system_fonts(), 0);
    let fallback = db.face(FALLBACK_FAMILY, "Regular");
    let before = db.composition_epoch().unwrap();

    // Empty paragraphs still resolve their primary face for baseline and line metrics, with
    // no character fallback or shaped glyph needed to discover the unresolved request.
    for step in 1..=2 {
        let face = db.face("No Such Epoch Test Family", "Regular");
        assert!(Arc::ptr_eq(&face, &fallback));
        assert_eq!(face.vertical_metrics(), fallback.vertical_metrics());
        assert_eq!(db.composition_epoch(), Some(before + step));
        assert!(crate::shape(&face, "", &[], |c| c).is_empty());
        assert_eq!(db.composition_epoch(), Some(before + step), "the primary metric lookup, not shaping, invalidates reuse");
    }
}

#[test]
fn cataloged_unreadable_font_invalidates_each_lookup_and_loads_when_readable_again() {
    let dir = font_dir("epoch-retry");
    // Leave no same-family sibling that could satisfy the request while Regular is absent.
    std::fs::remove_file(dir.join("Sub/Sysfont-Bold.TTF")).unwrap();
    let db = FontDb::with_font_dirs(vec![dir.clone()]);
    assert_eq!(db.styles(FAMILY), ["Regular"]);
    assert!(!db.is_loaded(FAMILY));
    let before = db.composition_epoch().unwrap();
    let font = dir.join("Sysfont-Regular.ttf");
    let parked = dir.join("Sysfont-Regular.parked");
    // A missing cataloged path is unreadable even for privileged test processes, unlike a
    // permissions-only fixture. Restore that exact path without rescanning the catalog.
    std::fs::rename(&font, &parked).unwrap();
    assert!(db.has_family(FAMILY), "the existing catalog still advertises this family");
    for step in 1..=2 {
        let fallback = db.face(FAMILY, "Regular");
        assert_eq!(fallback.family, FALLBACK_FAMILY);
        assert!(fallback.ascent > 0.0);
        assert!(!db.is_loaded(FAMILY));
        assert_eq!(db.composition_epoch(), Some(before + step), "retryable primary misses cannot certify a layout");
    }
    std::fs::rename(&parked, &font).unwrap();
    assert_eq!(db.composition_epoch(), Some(before + 2), "the filesystem change itself has no publication hook");
    let restored = db.face(FAMILY, "Regular");
    assert_eq!((restored.family.as_str(), restored.style.as_str()), (FAMILY, "Regular"));
    assert_eq!(db.composition_epoch(), Some(before + 3), "successful retry publishes the newly loaded face");
    assert!(Arc::ptr_eq(&restored, &db.face(FAMILY, "Regular")));
    assert_eq!(db.composition_epoch(), Some(before + 3));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn variable_instance_publication_advances_the_epoch_and_a_cached_instance_does_not() {
    let db = FontDb::with_font_dirs(Vec::new());
    assert_eq!(db.add_font(crate::testing::with_weight_axis(&renamed("SourceSans3-Regular.ttf")).unwrap()), 1);
    let base = db.face(FAMILY, "Regular");
    let axis = base.skrifa().unwrap().axes().get(0).unwrap();
    assert_eq!(axis.tag().to_be_bytes(), *b"wght");
    assert_eq!((axis.min_value(), axis.default_value(), axis.max_value()), (100.0, 400.0, 900.0));
    let before = db.composition_epoch().unwrap();
    let count = db.read_faces().len();

    let custom = db.face(FAMILY, "Regular {wght:650}");
    assert!(custom.is_variable());
    assert_eq!(custom.coords, vec![(*b"wght", 650.0)]);
    assert_eq!(custom.weight, 650.0);
    assert_eq!(db.read_faces().len(), count, "custom instances do not enter shared family lookup");
    assert_eq!(db.instances.read().unwrap().len(), 1);
    assert_eq!(db.composition_epoch(), Some(before + 1));
    assert!(Arc::ptr_eq(&custom, &db.face(FAMILY, "Regular {wght:650}")));
    assert_eq!(db.read_faces().len(), count, "custom instances do not enter shared family lookup");
    assert_eq!(db.instances.read().unwrap().len(), 1);
    assert_eq!(db.composition_epoch(), Some(before + 1));

    let static_face = db.face(FALLBACK_FAMILY, "Regular {wght:650}");
    assert!(!static_face.is_variable());
    assert_eq!(db.composition_epoch(), Some(before + 1), "axis syntax on a static face creates no instance");
}

#[test]
fn concurrent_variable_lookups_publish_only_one_instance() {
    let db = FontDb::with_font_dirs(Vec::new());
    assert_eq!(db.add_font(crate::testing::with_weight_axis(&renamed("SourceSans3-Regular.ttf")).unwrap()), 1);
    let before = db.composition_epoch().unwrap();
    let start = std::sync::Barrier::new(8);
    let faces = std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    db.face(FAMILY, "Regular {wght:650}")
                })
            })
            .collect();
        jobs.into_iter().map(|job| job.join().unwrap()).collect::<Vec<_>>()
    });
    assert!(faces.iter().all(|face| Arc::ptr_eq(face, &faces[0])));
    assert_eq!(db.instances.read().unwrap().len(), 1);
    assert_eq!(db.composition_epoch(), Some(before + 1));
}

#[test]
fn same_named_scoped_variable_instances_keep_their_own_metrics() {
    use crate::testing::{font_with_glyph, with_weight_axis};

    const SCOPED: &str = "Epoch Scoped Variable";
    const STYLE: &str = "Regular {wght:650}";
    let root = font_dir("epoch-scoped-instances");
    let (a, b) = (root.join("a"), root.join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let font = |glyph| with_weight_axis(&font_with_glyph(SCOPED, &['x'], glyph).unwrap()).unwrap();
    std::fs::write(a.join("font.ttf"), font('i')).unwrap();
    std::fs::write(b.join("font.ttf"), font('W')).unwrap();
    let db = FontDb::with_font_dirs(Vec::new());
    assert_eq!(db.add_font(font('m')), 1);
    let scope_a = db.load_document_fonts(&a).scope;
    let scope_b = db.load_document_fonts(&b).scope;
    let before = db.composition_epoch().unwrap();
    let face_a = db.scoped(scope_a).face(SCOPED, STYLE);
    let face_b = db.scoped(scope_b).face(SCOPED, STYLE);
    let shared = db.face(SCOPED, STYLE);
    assert_eq!(db.composition_epoch(), Some(before + 3));
    assert_ne!(face_a.id(), face_b.id());
    assert_ne!(face_a.id(), shared.id());
    assert_ne!(face_b.id(), shared.id());
    assert!(face_a.advance(face_a.glyph_for('x')) < face_b.advance(face_b.glyph_for('x')));
    assert!(Arc::ptr_eq(&db.scoped(scope_a).face(SCOPED, STYLE), &face_a));
    assert!(Arc::ptr_eq(&db.scoped(scope_b).face(SCOPED, STYLE), &face_b));
    assert!(Arc::ptr_eq(&db.face(SCOPED, STYLE), &shared));
    assert_eq!(db.composition_epoch(), Some(before + 3));

    db.close_scope(scope_a);
    assert!(Arc::ptr_eq(&db.scoped(scope_a).face(SCOPED, STYLE), &shared));
    let reopened = db.load_document_fonts(&a).scope;
    assert!(Arc::ptr_eq(&db.scoped(reopened).face(SCOPED, STYLE), &face_a));
    assert_eq!(db.composition_epoch(), Some(before + 5), "reopening republishes the scope and reuses its base and instance");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unresolved_variable_style_advances_the_epoch_even_when_the_primary_family_is_loaded() {
    let db = FontDb::with_font_dirs(Vec::new());
    assert_eq!(db.add_font(crate::testing::with_weight_axis(&renamed("SourceSans3-Regular.ttf")).unwrap()), 1);
    assert_eq!(db.load_system_fonts(), 0);
    assert!(db.is_loaded(FAMILY));
    let before = db.composition_epoch().unwrap();
    let count = db.read_faces().len();

    for step in 1..=2 {
        let fallback = db.face(FAMILY, "Regular {wght:invalid}");
        assert_eq!(fallback.family, FALLBACK_FAMILY);
        assert_eq!(db.read_faces().len(), count, "invalid coordinates do not publish an instance");
        assert_eq!(db.composition_epoch(), Some(before + step), "an unresolved primary style also invalidates reuse");
    }
}

#[test]
fn saturated_epoch_stays_unavailable_across_further_publications() {
    let db = FontDb::with_font_dirs(Vec::new());
    db.composition_epoch.store(u64::MAX - 2, Ordering::Release);
    assert_eq!(db.composition_epoch(), Some(u64::MAX - 2));
    assert_eq!(db.add_font(renamed("SourceSans3-Regular.ttf")), 1);
    assert_eq!(db.composition_epoch(), Some(u64::MAX - 1));
    db.set_system_fallback(false);
    assert_eq!(db.composition_epoch(), None);

    assert_eq!(db.add_font(renamed("SourceSans3-Bold.ttf")), 1);
    assert_eq!(db.load_system_fonts(), 0);
    assert_eq!(db.face("No Such Saturation Test Family", "Regular").family, FALLBACK_FAMILY);
    db.set_system_fallback(true);
    assert_eq!(db.composition_epoch(), None);
    assert_eq!(db.composition_epoch.load(Ordering::Acquire), u64::MAX, "the epoch must never wrap back to a reusable value");
}
