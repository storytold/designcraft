//! Existing two-entry reuse: identity is a performance contract; fresh composition is the oracle.
//! One serial test keeps process-global font publication out of otherwise independent cases.

use std::sync::Arc;

use designcraft_compose::{Cache, ComposeOptions, ComposedStory, compose_story};
use designcraft_doc::{
    Align, Condition, Document, GridAlign, Item, ItemId, PageSide, ParaFormat, Shape, SpreadRef, StoryId, Table, WrapMode,
    build::NewDocument,
    vars::{TextVariable, VarKind, var_char},
};
use designcraft_geom::{Rect, shapes};

fn document(text: &str) -> (Document, ItemId, StoryId) {
    let mut d = Document::new(&NewDocument { pages: 3, facing_pages: false, ..Default::default() });
    let (frame, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 220.0, 320.0), d.default_layer(), text, ParaFormat::default()).unwrap();
    (d, frame, sid)
}

fn edited(d: &Document, sid: StoryId) -> Document {
    let mut b = d.clone();
    b.story_mut(sid).unwrap().insert(0, "Changed é. ");
    b
}

fn assert_fresh(d: &Document, sid: StoryId, got: &ComposedStory) {
    let fresh = compose_story(d, sid, &ComposeOptions::default());
    // Compare all public layout fields, including glyph faces/geometry, ranges, columns,
    // overflow and RunStyle metadata; changed_frames intentionally compares a smaller subset.
    assert_eq!(format!("{got:?}"), format!("{fresh:?}"));
    let faces = |cs: &ComposedStory| cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).map(|g| g.face.id()).collect::<Vec<_>>();
    assert_eq!(faces(got), faces(&fresh), "font Debug names alone do not establish face identity");
}

fn assert_recomposed(d: &Document, sid: StoryId) {
    let cache = Cache::new();
    let a = cache.get(d, sid, None);
    let b = edited(d, sid);
    cache.get(&b, sid, None);
    let again = cache.get(d, sid, None);
    assert!(!Arc::ptr_eq(&a, &again), "excluded content must still compose freshly");
    assert_fresh(d, sid, &again);
}

fn assert_context_change(d: &Document, sid: StoryId, change: impl FnOnce(&mut Document)) {
    let cache = Cache::new();
    let a = cache.get(d, sid, None);
    cache.get(&edited(d, sid), sid, None);
    let mut different = d.clone();
    change(&mut different);
    assert!(Arc::ptr_eq(&d.stories[&sid], &different.stories[&sid]));
    assert!(Arc::ptr_eq(&d.styles, &different.styles));
    let got = cache.get(&different, sid, None);
    assert!(!Arc::ptr_eq(&a, &got), "supplemental context must veto stale previous layout");
    assert_fresh(&different, sid, &got);
}

fn insert_and_style_round_trips() {
    let (a, _, sid) = document(&"Ordinary body copy with shaping and reflow. ".repeat(24));
    for b in [edited(&a, sid), {
        let mut b = a.clone();
        for style in &mut b.styles_mut().paragraph {
            style.chars.font_family = Some("Source Sans 3".into());
            style.chars.underline = Some(true);
        }
        b
    }] {
        let cache = Cache::new();
        let first = cache.get(&a, sid, None);
        let second = cache.get(&b, sid, None);
        assert!(!Arc::ptr_eq(&first, &second));
        assert_fresh(&b, sid, &second);
        for _ in 0..8 {
            let undo = cache.get(&a, sid, None);
            assert!(Arc::ptr_eq(&first, &undo), "ordinary undo must reuse the retained allocation");
            assert_fresh(&a, sid, &undo);
            let redo = cache.get(&b, sid, None);
            assert!(Arc::ptr_eq(&second, &redo), "ordinary redo must swap the same two allocations");
            assert_fresh(&b, sid, &redo);
        }
    }
}

fn only_two_layouts_and_no_unrelated_spread_retention() {
    let (a, _, sid) = document("Two layouts only.");
    let cache = Cache::new();
    let first = cache.get(&a, sid, None);
    let old = Arc::downgrade(&first);
    let spread = Arc::downgrade(&a.spreads[0]);
    drop(first);
    let b = edited(&a, sid);
    let second = cache.get(&b, sid, None);
    let previous = Arc::downgrade(&second);
    drop(second);
    let c = edited(&b, sid);
    let third = cache.get(&c, sid, None);
    let current = Arc::downgrade(&third);
    drop(third);
    assert!(old.upgrade().is_none(), "the third state must release the oldest layout");
    assert!(previous.upgrade().is_some() && current.upgrade().is_some());
    drop(a);
    drop(b);
    drop(c);
    assert!(spread.upgrade().is_none(), "metadata must not pin ordinary spread snapshots");
    cache.clear();
    assert!(previous.upgrade().is_none() && current.upgrade().is_none());
}

fn supplemental_document_context() {
    let (mut d, _, sid) = document(&"Grid-aware text that flows across lines. ".repeat(10));
    d.story_mut(sid).unwrap().paras[0].para.grid_align = Some(GridAlign::AllLines);
    assert_context_change(&d, sid, |d| d.settings.baseline_grid.increment = 24.0);

    d.story_mut(sid).unwrap().paras[0].para.align = Some(Align::AwayFromSpine);
    assert_context_change(&d, sid, |d| Arc::make_mut(&mut d.spreads[0]).pages[0].side = PageSide::Left);
    assert_context_change(&d, sid, |d| Arc::make_mut(&mut d.spreads[0]).pages[0].width += 40.0);
    assert_context_change(&d, sid, |d| Arc::make_mut(&mut d.spreads[0]).pages[0].margins.top += 5.0);
    assert_context_change(&d, sid, |d| d.spreads.swap(0, 1));
    assert_context_change(&d, sid, |d| d.sections[0].start_number = Some(41));
    assert_eq!(50.0_f64.to_bits() as u32, 75.0_f64.to_bits() as u32, "this regression collides in the old 32-bit signature");
    d.settings.advanced_type.superscript_size = 50.0;
    d.story_mut(sid).unwrap().chars[0].format.over.position = Some(designcraft_doc::Position::Superscript);
    assert_context_change(&d, sid, |d| d.settings.advanced_type.superscript_size = 75.0);

    let id = ItemId(d.alloc());
    let mut wrap = Item::new(id, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(36.0, 36.0, 110.0, 100.0)));
    wrap.wrap.mode = WrapMode::BoundingBox;
    d.insert_item(SpreadRef::Doc(0), wrap, None).unwrap();
    assert_context_change(&d, sid, |d| d.layers[0].visible = false);
}

fn excluded_contexts_and_bounded_metadata() {
    let (base, frame, sid) = document("Excluded contexts.");
    for kind in [
        VarKind::OutputDate { format: "yyyy-MM-dd HH:mm".into() },
        VarKind::FileName { extension: true },
        VarKind::RunningHeader { style: "[Basic Paragraph]".into(), use_: Default::default(), character: false },
    ] {
        let mut d = base.clone();
        d.text_variables = vec![TextVariable::new("Dynamic", kind)];
        d.story_mut(sid).unwrap().insert(0, &var_char(0).unwrap().to_string());
        assert_recomposed(&d, sid);
    }
    let mut d = base.clone();
    d.story_mut(sid).unwrap().insert_note(0, "A note", ParaFormat::default());
    assert_recomposed(&d, sid);
    let mut d = base.clone();
    d.story_mut(sid).unwrap().insert_endnote(0, "An endnote", ParaFormat::default());
    assert_recomposed(&d, sid);
    let mut d = base.clone();
    d.story_mut(sid).unwrap().insert_table(0, Table::new(1, 1, 1, 0, 0, 100.0));
    assert_recomposed(&d, sid);
    let mut d = base.clone();
    d.story_mut(sid).unwrap().insert_xref(0, designcraft_doc::CrossRef { target: 0, format: "Page Number".into() });
    assert_recomposed(&d, sid);
    let mut d = base.clone();
    let object = Item::new(ItemId(d.alloc()), d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
    d.story_mut(sid).unwrap().insert_object(0, designcraft_doc::AnchoredObject::new(object, Default::default()));
    assert_recomposed(&d, sid);
    let mut d = base.clone();
    d.conditions.push(Condition { name: "Condition".into(), visible: true, color: [0, 0, 0] });
    assert_recomposed(&d, sid);
    let mut d = base.clone();
    d.hyphenation_exceptions.push("ex~cep~tion".into());
    assert_recomposed(&d, sid);
    let mut d = base.clone();
    d.settings.lists.push(designcraft_doc::NumberedList { name: "Named list".into(), continue_across_stories: true });
    assert_recomposed(&d, sid);

    let mut d = base.clone();
    d.item_mut(frame).unwrap().text_frame_mut().unwrap().options.column_rule_color = "x".repeat(128 * 1024);
    assert_recomposed(&d, sid);
    let mut d = base.clone();
    d.item_mut(frame).unwrap().text_frame_mut().unwrap().options.column_rule_weight = f64::NAN;
    assert_recomposed(&d, sid);

    // Nested group frames stay on main's existing composition path. Reuse does not import
    // the separate ancestor-identity changes from other work.
    let mut grouped = base.clone();
    let mut group =
        Item::new(ItemId(grouped.alloc()), grouped.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 300.0, 400.0)));
    let child = Arc::make_mut(&mut grouped.spreads[0]).items.remove(0);
    group.content = designcraft_doc::Content::Group { items: vec![child] };
    grouped.insert_item(SpreadRef::Doc(0), group, None).unwrap();
    assert_eq!(grouped.find(frame).unwrap().path.len(), 2);
    assert_recomposed(&grouped, sid);

    // Receiving-page variants keep their existing key/compose semantics.
    let cache = Cache::new();
    let first = cache.get(&base, sid, Some("1"));
    cache.get(&edited(&base, sid), sid, Some("1"));
    assert!(!Arc::ptr_eq(&first, &cache.get(&base, sid, Some("1"))));
    let mut d = base.clone();
    let (_, parent) =
        d.add_text_frame(SpreadRef::Parent(0), Rect::new(36.0, 36.0, 220.0, 60.0), d.default_layer(), "Parent text", ParaFormat::default()).unwrap();
    assert_recomposed(&d, parent);
}

fn concurrent_lookup_and_clear() {
    let (a, _, sid) = document("Concurrent cache users.");
    let b = edited(&a, sid);
    let cache = Arc::new(Cache::new());
    let mut jobs = Vec::new();
    for thread in 0..4 {
        let (a, b, cache) = (a.clone(), b.clone(), cache.clone());
        jobs.push(std::thread::spawn(move || {
            for step in 0..20 {
                if (step + thread) % 5 == 0 {
                    cache.clear();
                }
                let doc = if (step + thread) % 2 == 0 { &a } else { &b };
                assert_fresh(doc, sid, &cache.get(doc, sid, None));
            }
        }));
    }
    for job in jobs {
        job.join().unwrap();
    }
    let current = cache.get(&a, sid, None);
    let weak = Arc::downgrade(&current);
    drop(current);
    cache.clear();
    assert!(weak.upgrade().is_none(), "completed clear must release both slots after users finish");
}

fn font_publication_and_retryable_misses() {
    let db = designcraft_fonts::FontDb::global();
    let (a, _, sid) = document("A stable loaded family.");
    let cache = Cache::new();
    let first = cache.get(&a, sid, None);
    cache.get(&edited(&a, sid), sid, None);
    db.set_system_fallback(false);
    let again = cache.get(&a, sid, None);
    assert!(!Arc::ptr_eq(&first, &again), "font state changes invalidate previous promotion");
    db.set_system_fallback(true);
    assert!(Arc::ptr_eq(&again, &cache.get(&a, sid, None)), "current-hit behavior is intentionally unchanged");

    for text in ["", "A requested font is unavailable."] {
        let (mut d, _, sid) = document(text);
        for style in &mut d.styles_mut().paragraph {
            style.chars.font_family = Some("DesignCraft absent primary face".into());
        }
        let cache = Cache::new();
        let before = db.composition_epoch();
        let first = cache.get(&d, sid, None);
        assert_ne!(before, db.composition_epoch(), "even an empty paragraph resolves font metrics");
        assert!(Arc::ptr_eq(&first, &cache.get(&d, sid, None)), "missing fonts must not cause repeated current recomposition");
        cache.get(&edited(&d, sid), sid, None);
        let again = cache.get(&d, sid, None);
        assert!(!Arc::ptr_eq(&first, &again), "pre/post epoch mismatch must prevent storing a reusable layout");
        assert_fresh(&d, sid, &again);
    }
}

#[test]
fn previous_layout_reuse_contract() {
    insert_and_style_round_trips();
    only_two_layouts_and_no_unrelated_spread_retention();
    supplemental_document_context();
    excluded_contexts_and_bounded_metadata();
    font_publication_and_retryable_misses();
    concurrent_lookup_and_clear();
}
