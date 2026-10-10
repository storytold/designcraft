//! Structural guard tests avoid assumptions about the concurrently used global font database.

use super::*;
use designcraft_doc::{ItemId, TextFrameOptions};
use designcraft_geom::Rect;

fn frame() -> FrameSpec {
    FrameSpec {
        id: ItemId(1),
        area: Rect::new(0.0, 0.0, 180.0, 240.0),
        opts: TextFrameOptions::default(),
        vertical: false,
        exclusions: vec![],
        page_name: Some("1".into()),
        page: Some(0),
        grid: Some((36.0, 12.0)),
        left_page: false,
        page_rect: Some((Rect::new(0.0, 0.0, 612.0, 792.0), Rect::new(36.0, 36.0, 576.0, 756.0))),
    }
}

#[test]
fn full_width_advanced_type_guard_rejects_a_legacy_32_bit_signature_collision() {
    let a = [50.0_f64.to_bits(), 33.0_f64.to_bits(), 50.0_f64.to_bits(), 33.0_f64.to_bits()];
    let mut b = a;
    b[0] = 75.0_f64.to_bits();
    assert_eq!(a.map(|v| v as u32), b.map(|v| v as u32), "the old wasm32 signature cannot distinguish these values");
    let witness = ReuseWitness { frames: vec![frame()], font_epoch: 7, advanced_type: a };
    assert!(witness.matches(&[frame()], 7, a));
    assert!(!witness.matches(&[frame()], 7, b), "the production supplemental predicate must independently reject the collision");
}

#[test]
fn publication_crossing_composition_never_receives_the_new_epoch_as_a_certificate() {
    let advanced = [0; 4];
    let stable = ReuseWitness::after_composition(vec![frame()], Some(7), Some(7), advanced).unwrap();
    assert!(stable.matches(&[frame()], 7, advanced));
    assert!(!stable.matches(&[frame()], 8, advanced), "publication after composition rejects later promotion");
    assert!(ReuseWitness::after_composition(vec![frame()], Some(7), Some(8), advanced).is_none());
    assert!(ReuseWitness::after_composition(vec![frame()], Some(7), None, advanced).is_none());
    assert!(ReuseWitness::after_composition(vec![frame()], None, None, advanced).is_none(), "saturation never certifies reuse");
}

#[test]
fn publication_during_frame_preparation_vetoes_an_otherwise_matching_previous_entry() {
    let frames = vec![frame()];
    let witness = ReuseWitness { frames: frames.clone(), font_epoch: 7, advanced_type: [0; 4] };
    // Snapshot E, prepare and compare frames, then publication E+1 completes. The early
    // predicate still matches exactly; the final production promotion gate must reject it.
    assert!(witness.matches(&frames, 7, [0; 4]));
    assert!(promotion_epoch_is_current(7, Some(7)));
    assert!(!promotion_epoch_is_current(7, Some(8)));
    assert!(!promotion_epoch_is_current(7, None), "saturation during preparation also rejects promotion");
}

#[test]
fn exact_frame_fields_independently_veto_promotion() {
    let original = frame();
    let witness = ReuseWitness { frames: vec![original.clone()], font_epoch: 1, advanced_type: [0; 4] };
    let changes: [fn(&mut FrameSpec); 11] = [
        |f| f.id = ItemId(2),
        |f| f.area.x1 += 1.0,
        |f| f.vertical = true,
        |f| f.opts.inset[0] += 1.0,
        |f| f.exclusions.push(crate::Exclusion { rect: Rect::new(1.0, 2.0, 3.0, 4.0), mode: WrapMode::BoundingBox }),
        |f| f.page_name = Some("2".into()),
        |f| f.page = Some(1),
        |f| f.grid = Some((36.0, 24.0)),
        |f| f.left_page = true,
        |f| f.page_rect.as_mut().unwrap().0.x1 += 1.0,
        |f| f.page_rect.as_mut().unwrap().1.y0 += 1.0,
    ];
    for change in changes {
        let mut changed = original.clone();
        change(&mut changed);
        assert!(!witness.matches(&[changed], 1, [0; 4]));
    }
}

#[test]
fn metadata_budget_counts_capacity_not_only_visible_elements_or_string_lengths() {
    let frames = vec![frame()];
    assert!(bounded_body_context(&frames, frames.capacity()));
    assert!(!bounded_body_context(&frames, MAX_REUSE_CONTEXT_BYTES / std::mem::size_of::<FrameSpec>() + 1));

    let mut name = frame();
    name.page_name = Some(String::with_capacity(MAX_REUSE_CONTEXT_BYTES));
    name.page_name.as_mut().unwrap().push('1');
    assert_eq!(name.page_name.as_ref().unwrap().len(), 1);
    assert!(!bounded_body_context(&[name], 1));
    let mut color = frame();
    color.opts.column_rule_color = String::with_capacity(MAX_REUSE_CONTEXT_BYTES);
    color.opts.column_rule_color.push('x');
    assert!(!bounded_body_context(&[color], 1));
    let mut wraps = frame();
    wraps.exclusions = Vec::with_capacity(MAX_REUSE_CONTEXT_BYTES / std::mem::size_of::<crate::Exclusion>() + 1);
    assert!(wraps.exclusions.is_empty());
    assert!(!bounded_body_context(&[wraps], 1));
}

#[test]
fn parent_empty_and_nonfinite_contexts_cannot_be_certified() {
    assert!(!bounded_body_context(&[], 0));
    let mut parent = frame();
    parent.page = None;
    assert!(!bounded_body_context(&[parent], 1));
    for change in [
        (|f: &mut FrameSpec| f.area.x0 = f64::NAN) as fn(&mut FrameSpec),
        |f| f.opts.column_rule_weight = f64::NAN,
        |f| f.grid = Some((f64::NAN, 12.0)),
        |f| f.page_rect.as_mut().unwrap().0.x0 = f64::NAN,
        |f| f.area.y1 = f64::INFINITY,
        |f| f.opts.column_rule_weight = f64::INFINITY,
        |f| f.opts.inset[2] = f64::NEG_INFINITY,
        |f| f.opts.gutter = f64::INFINITY,
        |f| f.opts.baseline_grid = Some((f64::INFINITY, 12.0)),
        |f| f.grid = Some((0.0, f64::INFINITY)),
        |f| f.page_rect.as_mut().unwrap().1.y1 = f64::NEG_INFINITY,
        |f| f.exclusions.push(crate::Exclusion { rect: Rect::new(0.0, 0.0, f64::INFINITY, 20.0), mode: WrapMode::BoundingBox }),
    ] {
        let mut invalid = frame();
        change(&mut invalid);
        assert!(!bounded_body_context(&[invalid], 1));
    }
}

#[test]
fn clear_keeps_current_locked_until_it_can_clear_previous() {
    use std::sync::TryLockError;
    use std::time::{Duration, Instant};

    let cache = Arc::new(Cache::new());
    // Deliberately stop clear at its second lock, then observe that lookup/promotion's
    // first lock stays unavailable. This establishes the interleaving rather than hoping
    // two unsynchronized workers overlap. No font lookup or global epoch is involved.
    let previous = cache.previous.lock().unwrap();
    let clearing = cache.clone();
    let job = std::thread::spawn(move || clearing.clear());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match cache.map.try_lock() {
            Err(TryLockError::WouldBlock) => break,
            Err(TryLockError::Poisoned(_)) => panic!("cache map must not be poisoned"),
            Ok(guard) => drop(guard),
        }
        assert!(Instant::now() < deadline, "clear released current before obtaining previous; promotion can resurrect a stale entry");
        std::thread::yield_now();
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let lookup = cache.clone();
    let waiter = std::thread::spawn(move || {
        let map = lookup.map.lock().unwrap();
        tx.send(map.0.len()).unwrap();
    });
    assert!(rx.recv_timeout(Duration::from_millis(20)).is_err(), "promotion must remain blocked behind clear");
    drop(previous);
    job.join().unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 0);
    waiter.join().unwrap();
}

fn body_document(text: &str) -> (Document, StoryId) {
    let mut doc = Document::new(&Default::default());
    let (_, sid) = doc
        .add_text_frame(designcraft_doc::SpreadRef::Doc(0), Rect::new(36.0, 36.0, 200.0, 300.0), doc.default_layer(), text, Default::default())
        .unwrap();
    (doc, sid)
}

fn changed_body(doc: &Document, sid: StoryId) -> Document {
    let mut changed = doc.clone();
    changed.story_mut(sid).unwrap().insert(0, "Edited ");
    changed
}

fn lookup(cache: &Cache, doc: &Document, sid: StoryId, db: &designcraft_fonts::FontDb) -> Arc<ComposedStory> {
    cache.get_with_db(doc, sid, None, db, &mut |_| {})
}

fn assert_fresh_with_db(doc: &Document, sid: StoryId, db: &designcraft_fonts::FontDb, actual: &ComposedStory) {
    let expected = crate::compose_with_db(doc, doc.story(sid).unwrap(), &crate::frame_specs(doc, sid), &ComposeOptions::default(), db);
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    let face_ids = |out: &ComposedStory| out.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).map(|g| g.face.id()).collect::<Vec<_>>();
    assert_eq!(face_ids(actual), face_ids(&expected));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn real_lookup_rechecks_shared_catalog_and_fallback_publications_at_promotion() {
    use designcraft_fonts::{FontDb, testing::font_with};
    for stage in [LookupStage::CapturedEpoch, LookupStage::BeforePromotion] {
        for publication in 0..5 {
            let db = FontDb::with_font_dirs(vec![]);
            db.load_system_fonts();
            let (a, sid) = body_document("Stable ordinary text.");
            let b = changed_body(&a, sid);
            let cache = Cache::new();
            let first = lookup(&cache, &a, sid, &db);
            lookup(&cache, &b, sid, &db);
            assert!(cache.previous.lock().unwrap().get(&sid).unwrap().reuse.is_some());
            let before = db.composition_epoch();
            let mut published = false;
            let actual = cache.get_with_db(&a, sid, None, &db, &mut |at| {
                if at == stage && !published {
                    published = true;
                    std::thread::scope(|scope| {
                        scope
                            .spawn(|| match publication {
                                0 => assert_eq!(db.add_font(font_with("Cache race shared publication", &['x']).unwrap()), 1),
                                1 => assert_eq!(db.load_system_fonts(), 0),
                                2 => db.set_system_fallback(false),
                                3 => assert!(db.fallback_for('\u{10ffff}', 0, None).is_none()),
                                _ => {
                                    let _ = db.fallback_for('가', 0, Some("ko"));
                                }
                            })
                            .join()
                            .unwrap();
                    });
                    assert_ne!(db.composition_epoch(), before);
                }
            });
            assert!(published, "the scheduled production lookup stage ran");
            assert!(!Arc::ptr_eq(&first, &actual), "publication {publication} at {stage:?} must veto the real swap");
            assert_fresh_with_db(&a, sid, &db, &actual);
            assert!(cache.map.lock().unwrap().0.get(&(sid, None)).unwrap().reuse.is_none(), "the captured epoch crossed publication");
            assert!(Arc::ptr_eq(&actual, &lookup(&cache, &a, sid, &db)), "current hits retain their original behavior");
        }
    }
}

#[test]
fn real_lookup_never_certifies_composition_crossing_font_publication() {
    use designcraft_fonts::{FontDb, testing::font_with};
    let db = FontDb::with_font_dirs(vec![]);
    let (a, sid) = body_document("Fresh layout across publication.");
    let cache = Cache::new();
    let first = cache.get_with_db(&a, sid, None, &db, &mut |stage| {
        if stage == LookupStage::AfterComposition {
            assert_eq!(db.add_font(font_with("Cache post composition publication", &['x']).unwrap()), 1);
        }
    });
    assert!(cache.map.lock().unwrap().0.get(&(sid, None)).unwrap().reuse.is_none());
    lookup(&cache, &changed_body(&a, sid), sid, &db);
    let again = lookup(&cache, &a, sid, &db);
    assert!(!Arc::ptr_eq(&first, &again));
    assert_fresh_with_db(&a, sid, &db, &again);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn real_lookup_preserves_scope_identity_metrics_and_close_reopen_publication() {
    use designcraft_fonts::{FontDb, testing::font_with_glyph};
    let root = std::env::temp_dir().join(format!("dc-cache-reuse-scopes-{}", std::process::id()));
    let dirs = [root.join("wide"), root.join("narrow")];
    for (dir, glyph) in dirs.iter().zip(['W', 'i']) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("font.ttf"), font_with_glyph("Reuse scoped face", &['a'], glyph).unwrap()).unwrap();
    }
    let db = FontDb::with_font_dirs(vec![]);
    let scopes = dirs.each_ref().map(|dir| db.load_document_fonts(dir).scope);
    assert_ne!(scopes[0], scopes[1]);
    let (mut wide, sid) = body_document("aaaa");
    for style in &mut wide.styles_mut().paragraph {
        style.chars.font_family = Some("Reuse scoped face".into());
    }
    wide.font_scope = scopes[0];
    let mut narrow = wide.clone();
    narrow.font_scope = scopes[1];
    let cache = Cache::new();
    let first = lookup(&cache, &wide, sid, &db);
    let second = lookup(&cache, &narrow, sid, &db);
    let end_x = |out: &ComposedStory| out.frames[0].lines[0].end_x;
    assert!(end_x(&first) > end_x(&second), "same family name in separate scopes has independently specified wide/narrow glyphs");
    assert!(Arc::ptr_eq(&first, &lookup(&cache, &wide, sid, &db)));
    assert!(Arc::ptr_eq(&second, &lookup(&cache, &narrow, sid, &db)));
    assert_fresh_with_db(&wide, sid, &db, &first);
    assert_fresh_with_db(&narrow, sid, &db, &second);

    let mut closed = false;
    let after_close = cache.get_with_db(&wide, sid, None, &db, &mut |stage| {
        if stage == LookupStage::BeforePromotion && !closed {
            closed = true;
            db.close_scope(scopes[0]);
        }
    });
    assert!(closed && !Arc::ptr_eq(&first, &after_close));
    assert_fresh_with_db(&wide, sid, &db, &after_close);
    let reopened = db.load_document_fonts(&dirs[0]).scope;
    assert_ne!(reopened, scopes[0]);
    wide.font_scope = reopened;
    let after_reopen = lookup(&cache, &wide, sid, &db);
    assert!(!Arc::ptr_eq(&first, &after_reopen));
    assert_eq!(end_x(&after_reopen), end_x(&first));
    assert_fresh_with_db(&wide, sid, &db, &after_reopen);
    let after_narrow = lookup(&cache, &narrow, sid, &db);
    assert_eq!(end_x(&after_narrow), end_x(&second), "closing another scope leaves this document's metrics alone");
    db.close_scope(scopes[1]);
    db.close_scope(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn real_lookup_distinguishes_variable_instance_publication_from_cached_hits() {
    use designcraft_fonts::{
        FontDb,
        testing::{font_with, with_weight_axis},
    };
    let db = FontDb::with_font_dirs(vec![]);
    assert_eq!(db.add_font(with_weight_axis(&font_with("Reuse variable face", &['a']).unwrap()).unwrap()), 1);
    let known = db.face("Reuse variable face", "Regular {wght:650}");
    assert!(known.is_variable());
    let (a, sid) = body_document("Stable body across a variable instance lookup.");
    let b = changed_body(&a, sid);
    let cache = Cache::new();
    let first = lookup(&cache, &a, sid, &db);
    let second = lookup(&cache, &b, sid, &db);
    let epoch = db.composition_epoch();
    let cached = cache.get_with_db(&a, sid, None, &db, &mut |stage| {
        if stage == LookupStage::BeforePromotion {
            assert!(Arc::ptr_eq(&known, &db.face("Reuse variable face", "Regular {wght:650}")));
            assert_eq!(db.composition_epoch(), epoch);
        }
    });
    assert!(Arc::ptr_eq(&first, &cached), "reading a cached instance must permit the actual promotion");
    assert!(Arc::ptr_eq(&second, &lookup(&cache, &b, sid, &db)));
    let changed = cache.get_with_db(&a, sid, None, &db, &mut |stage| {
        if stage == LookupStage::BeforePromotion {
            let published = db.face("Reuse variable face", "Regular {wght:720}");
            assert_eq!(published.coords, vec![(*b"wght", 720.0)]);
            assert_ne!(db.composition_epoch(), epoch);
        }
    });
    assert!(!Arc::ptr_eq(&first, &changed));
    assert_fresh_with_db(&a, sid, &db, &changed);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn real_lookup_retries_an_unreadable_cataloged_primary_even_for_empty_text() {
    use designcraft_fonts::{FontDb, testing::font_with_glyph};
    let root = std::env::temp_dir().join(format!("dc-cache-reuse-retry-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (case, text) in ["", "aaaa"].into_iter().enumerate() {
        let folder = root.join(case.to_string());
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("font.ttf");
        let parked = folder.join("font.parked");
        std::fs::write(&path, font_with_glyph("Reuse retry primary", &['a'], 'W').unwrap()).unwrap();
        let db = FontDb::with_font_dirs(vec![folder]);
        assert_eq!(db.load_system_fonts(), 1);
        std::fs::rename(&path, &parked).unwrap();
        let (mut a, sid) = body_document(text);
        for style in &mut a.styles_mut().paragraph {
            style.chars.font_family = Some("Reuse retry primary".into());
        }
        let cache = Cache::new();
        let epoch = db.composition_epoch();
        let first = lookup(&cache, &a, sid, &db);
        assert_ne!(db.composition_epoch(), epoch, "empty paragraphs still resolve a primary face for metrics");
        assert!(cache.map.lock().unwrap().0.get(&(sid, None)).unwrap().reuse.is_none());
        assert!(Arc::ptr_eq(&first, &lookup(&cache, &a, sid, &db)), "unresolved fonts leave the current-hit contract alone");
        lookup(&cache, &changed_body(&a, sid), sid, &db);
        let before_restore = db.composition_epoch();
        std::fs::rename(&parked, &path).unwrap();
        assert_eq!(db.composition_epoch(), before_restore, "filesystem restoration is not a database publication");
        let again = lookup(&cache, &a, sid, &db);
        assert!(!Arc::ptr_eq(&first, &again), "an uncertified previous layout must retry the real catalog lookup");
        assert_ne!(db.composition_epoch(), before_restore);
        assert_fresh_with_db(&a, sid, &db, &again);
        assert_eq!(db.face("Reuse retry primary", "Regular").family, "Reuse retry primary");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn many_edits_retain_two_layouts_and_pin_only_their_signature_owners_until_clear() {
    let db = designcraft_fonts::FontDb::with_font_dirs(vec![]);
    let (mut doc, sid) = body_document("A");
    let cache = Cache::new();
    let mut layouts = Vec::new();
    for _ in 0..96 {
        let out = lookup(&cache, &doc, sid, &db);
        layouts.push(Arc::downgrade(&out));
        drop(out);
        assert_eq!(layouts.iter().filter(|weak| weak.strong_count() > 0).count(), layouts.len().min(2));
        doc = changed_body(&doc, sid);
    }
    let (story, styles, item) = {
        let previous = cache.previous.lock().unwrap();
        let old = previous.get(&sid).unwrap();
        (Arc::downgrade(&old._keep.0), Arc::downgrade(&old._keep.1), Arc::downgrade(&old._keep.2[0]))
    };
    drop(doc);
    assert!(story.upgrade().is_some() && styles.upgrade().is_some() && item.upgrade().is_some());
    assert_eq!(cache.map.lock().unwrap().0.len(), 1);
    assert_eq!(cache.previous.lock().unwrap().len(), 1);
    cache.clear();
    assert!(layouts.iter().all(|weak| weak.upgrade().is_none()));
    assert!(story.upgrade().is_none() && styles.upgrade().is_none() && item.upgrade().is_none());
}
