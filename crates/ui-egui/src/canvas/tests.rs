use designcraft_geom::Unit;
use egui_kittest::{Harness, kittest::Queryable};

use super::*;

fn density_fixture() -> (DesignApp, designcraft_doc::ItemId) {
    let mut doc = designcraft_doc::Document::new(&designcraft_doc::build::NewDocument {
        width: 100.0,
        height: 100.0,
        facing_pages: false,
        ..Default::default()
    });
    let id = designcraft_doc::ItemId(doc.alloc());
    let item = designcraft_doc::Item::new(
        id,
        doc.default_layer(),
        designcraft_doc::Shape::Rectangle,
        designcraft_geom::shapes::rectangle(DRect::new(60.0, 20.0, 80.0, 40.0)),
    );
    doc.insert_item(designcraft_doc::SpreadRef::Doc(0), item, None).unwrap();
    let mut session = designcraft_engine::Session::new();
    session.add_document(designcraft_engine::DocState::new(doc, None));
    session.execute("selection.set", &json!({"ids":[id.0]})).unwrap();
    (DesignApp::new(session, Default::default()), id)
}

#[test]
fn density_change_rejects_a_partial_upload_into_the_old_pixel_size() {
    let (mut app, _) = density_fixture();
    let ctx = egui::Context::default();
    let st = app.session.active().unwrap();
    app.canvas.shown = Some(crate::Shown {
        doc: (st.uid, std::sync::Arc::as_ptr(&st.doc) as u64, st.revision, false),
        editing_parents: false,
        origin: Point::new(-50.0, 0.0),
        zoom: 1.0,
        size: (100.0, 100.0),
    });
    app.canvas.shown_doc = Some(st.doc.clone());
    app.canvas.texture = Some(ctx.load_texture("old-1x-canvas", egui::ColorImage::filled([100, 100], Color32::WHITE), egui::TextureOptions::LINEAR));
    app.session.execute("transform.move", &json!({"dx":1.0})).unwrap();
    let st = app.session.active().unwrap();
    let key = (st.uid, std::sync::Arc::as_ptr(&st.doc) as u64, st.revision, false);
    let layout = CanvasLayout::new(&st.doc, false);
    // At 2x this damage starts beyond x=100. The previous implementation queued
    // the invalid update; the native wgpu renderer then panicked on write_texture.
    assert!(!patch_texture(&mut app, &layout, 2.0, key, false), "a stale 1x texture must be replaced before any 2x patch");
    assert_eq!(app.canvas.patches, 0);
    assert_eq!(app.canvas.texture.as_ref().unwrap().size(), [100, 100]);
}

fn density_frame(app: &mut DesignApp, ctx: &egui::Context, scale: f32) {
    let mut input =
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), max_texture_side: Some(8192), ..Default::default() };
    input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(scale);
    let mut out = ctx.run_ui(input, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
}

#[test]
fn density_changes_refresh_the_canvas_before_subsequent_edits() {
    let (mut app, _) = density_fixture();
    let ctx = egui::Context::default();
    for scale in [1.0, 2.0, 1.5, 1.0] {
        density_frame(&mut app, &ctx, scale);
        density_frame(&mut app, &ctx, scale);
        assert!((ctx.pixels_per_point() - scale).abs() < 0.001);
        let matches = |a: &DesignApp| {
            a.canvas
                .shown
                .zip(a.canvas.texture.as_ref())
                .is_some_and(|(sh, tex)| tex.size() == [(sh.size.0 * scale).round().max(1.0) as usize, (sh.size.1 * scale).round().max(1.0) as usize])
        };
        assert!(matches(&app) || app.canvas.pending.is_some(), "density change must invalidate the old canvas even with no document edits");
        app.session.execute("transform.move", &json!({"dx":1.0})).unwrap();
        let start = std::time::Instant::now();
        loop {
            density_frame(&mut app, &ctx, scale);
            if matches(&app) && app.canvas.pending.is_none() {
                break;
            }
            assert!(start.elapsed().as_secs() < 10, "density refresh stalled");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!app.canvas.low_quality);
    }
}

#[test]
fn a_press_after_a_drag_is_not_a_double_click() {
    let (delay, dist) = (0.3, 6.0);
    let press = |last, x: f32, now| press_count(last, pos2(x, 0.0), now, delay, dist);
    // Click, click: a double click.
    let n = press(None, 0.0, 0.0);
    let up = release_clicks(Clicks { at: f64::NEG_INFINITY, pos: pos2(0.0, 0.0), n }, pos2(0.0, 0.0), 0.1, dist);
    assert_eq!(press(Some(up), 1.0, 0.2), 2);
    // A drag from 0 to 100, then a fast press where it started or where it ended: a single click.
    let up = release_clicks(Clicks { at: f64::NEG_INFINITY, pos: pos2(0.0, 0.0), n: 1 }, pos2(100.0, 0.0), 0.1, dist);
    assert_eq!(press(Some(up), 0.0, 0.2), 1);
    assert_eq!(press(Some(up), 100.0, 0.2), 1);
    // Too slow: a single click.
    let up = release_clicks(Clicks { at: f64::NEG_INFINITY, pos: pos2(0.0, 0.0), n: 1 }, pos2(0.0, 0.0), 0.1, dist);
    assert_eq!(press(Some(up), 0.0, 1.0), 1);
}

/// While a page's render is on its way the page shows paper, not the black of its shadow.
#[test]
fn pages_show_paper_under_their_render() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", serde_json::json!({"pages": 3, "facingPages": true})).unwrap();
    let ctx = egui::Context::default();
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| show(&mut app, ui));
        out.textures_delta.clear();
        shapes = out.shapes;
    }
    let st = app.session.active().unwrap();
    let layout = CanvasLayout::new(&st.doc, false);
    let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    // The paper swatch on screen (white, as the colour settings show it).
    let [r, g, b, _] = st.doc.resolve_color(designcraft_color::swatch::PAPER, 1.0).unwrap().to_rgba8(1.0);
    let paper: Vec<Rect> = shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(s) if s.fill == Color32::from_rgb(r, g, b) => Some(s.rect),
            _ => None,
        })
        .collect();
    // Each visible page has its own paper (a spread's pages may differ in size).
    // Distant spreads are culled, so their offscreen paper need not be painted.
    let pages: Vec<Rect> = (0..st.doc.page_count()).map(|abs| xf.rect(layout.page_rect(&st.doc, abs).unwrap())).collect();
    let visible: Vec<_> = pages.iter().filter(|r| r.intersects(app.canvas_rect.unwrap())).collect();
    assert!(!visible.is_empty());
    let same = |a: &Rect, b: &Rect| (a.min - b.min).length() < 0.01 && (a.max - b.max).length() < 0.01;
    for want in visible {
        assert!(paper.iter().any(|r| same(r, want)), "visible page {want:?}");
    }
    assert!(paper.iter().all(|r| pages.iter().any(|p| same(r, p))), "paper follows individual page bounds");
}

/// Bleed and Slug are preview modes: like Preview they hide the guides, and what the page
/// shows widens to the bleed or the slug box. The slug guide is drawn in Normal, in its own
/// colour, beside the bleed guide.
#[test]
fn bleed_and_slug_are_preview_modes_that_show_their_area() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", serde_json::json!({"bleed": 9, "slug": 18})).unwrap();
    let strokes = |app: &mut DesignApp, want: [u8; 3]| -> usize {
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| show(app, ui));
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        let want = c32(want);
        shapes.iter().filter(|c| matches!(&c.shape, egui::Shape::Rect(s) if s.stroke.color == want)).count()
    };
    let s = app.session.active().unwrap().doc.settings.clone();
    // Normal draws every guide, the slug's among them.
    app.ui.screen_mode = ScreenMode::Normal;
    assert!(strokes(&mut app, s.margin_color) > 0, "Normal draws the margin guide");
    assert!(strokes(&mut app, s.bleed_color) > 0, "Normal draws the bleed guide");
    assert!(strokes(&mut app, s.slug_color) > 0, "Normal draws the slug guide");
    // Every preview mode hides them.
    for mode in [ScreenMode::Preview, ScreenMode::Bleed, ScreenMode::Slug, ScreenMode::Presentation] {
        app.ui.screen_mode = mode;
        assert_eq!(strokes(&mut app, s.margin_color), 0, "{mode:?} hides the margin guide");
        assert_eq!(strokes(&mut app, s.slug_color), 0, "{mode:?} hides the slug guide");
    }
}

/// Each preview mode's shown area: Preview stops at the trim, Bleed reaches the bleed box and
/// Slug the slug box. A slug narrower than the bleed never pulls the edge back in.
#[test]
fn shown_area_widens_to_the_bleed_and_slug_boxes() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", serde_json::json!({"bleed": 9, "slug": 18})).unwrap();
    let doc = app.session.active().unwrap().doc.clone();
    let layout = CanvasLayout::new(&doc, false);
    let xf = Xf::new(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0)), &View::default());
    let slot = &layout.slots[0];
    let area = |app: &mut DesignApp, mode| {
        app.ui.screen_mode = mode;
        shown_area(app, &doc, &xf, slot)
    };
    let trim = xf.rect(slot.bounds);
    assert_eq!(area(&mut app, ScreenMode::Preview), trim, "Preview stops at the trim");
    assert_eq!(area(&mut app, ScreenMode::Presentation), trim, "Presentation stops at the trim");
    let bleed = area(&mut app, ScreenMode::Bleed);
    let slug = area(&mut app, ScreenMode::Slug);
    assert!(bleed.contains_rect(trim) && bleed != trim, "Bleed reaches past the trim");
    assert!(slug.contains_rect(bleed) && slug != bleed, "Slug reaches past the bleed");
    // 9 pt of bleed and 18 pt of slug at this zoom, on every edge.
    let grew = |r: Rect, pt: f64| {
        let d = (pt * xf.zoom) as f32;
        (r.min - trim.min + vec2(d, d)).length() < 0.01 && (r.max - trim.max - vec2(d, d)).length() < 0.01
    };
    assert!(grew(bleed, 9.0), "Bleed grows by the bleed: {bleed:?}");
    assert!(grew(slug, 18.0), "Slug grows by the slug: {slug:?}");
    // A slug inside the bleed: Slug still shows every printing element, so the bleed too.
    app.run("file.new", serde_json::json!({"bleed": 18, "slug": 9})).unwrap();
    let doc = app.session.active().unwrap().doc.clone();
    app.ui.screen_mode = ScreenMode::Slug;
    assert!(grew(shown_area(&app, &doc, &xf, slot), 18.0), "the wider of the two boxes wins");
    // No bleed and no slug: nothing to widen to.
    app.run("file.new", serde_json::json!({})).unwrap();
    let doc = app.session.active().unwrap().doc.clone();
    for mode in [ScreenMode::Bleed, ScreenMode::Slug] {
        app.ui.screen_mode = mode;
        assert_eq!(shown_area(&app, &doc, &xf, slot), trim, "{mode:?} without a bleed or slug");
    }
}

/// Build the four-fps app harness used by canvas tests.
fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
    harness_with_step(app, 1.0 / 4.0)
}

/// A window whose frames last `step_dt` seconds. Wheel motion is tested at a realistic frame
/// rate: egui ends a scroll action after 150 ms without wheel events, so quarter-second
/// frames would split every motion into separate actions.
fn harness_with_step(app: DesignApp, step_dt: f32) -> Harness<'static, DesignApp> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(step_dt).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

#[test]
fn display_option_change_drops_the_render_in_flight() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let shown = crate::Shown { doc: (1, 2, 3, false), origin: Point::new(0.0, 0.0), zoom: 1.0, size: (10.0, 10.0), editing_parents: false };
    invalidate_on_display_change(&mut app);
    app.canvas.shown = Some(shown);
    app.canvas.pending = Some((7, shown));
    // Nothing changed: the texture and the request stay.
    invalidate_on_display_change(&mut app);
    assert!(app.canvas.shown.is_some() && app.canvas.pending.is_some());
    // A display option changed while the render ran: its result must not be accepted.
    app.ui.display_quality = designcraft_render::DisplayQuality::Fast;
    invalidate_on_display_change(&mut app);
    assert!(app.canvas.shown.is_none() && app.canvas.pending.is_none() && app.canvas.pending_doc.is_none());
    // The document grid is drawn into the texture too.
    app.canvas.shown = Some(shown);
    app.ui.document_grid = !app.ui.document_grid;
    invalidate_on_display_change(&mut app);
    assert!(app.canvas.shown.is_none());
}

/// A primary-button drag from `a` to `b` over a few frames.
fn drag(h: &mut Harness<'_, DesignApp>, a: Pos2, b: Pos2) {
    h.hover_at(a);
    h.step();
    h.event(egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    for k in 1..=4 {
        h.hover_at(a + (b - a) * (k as f32 / 4.0));
        h.step();
    }
    h.event(egui::Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
}

#[test]
fn the_page_tool_draws_liquid_guides_from_the_rulers() {
    // Chris Cicchino's Page Tool and Liquid Layout video (11:00): with the Page tool, a guide
    // dragged from the ruler is a liquid guide (dashed).
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.session.set_tool("page");
    let mut h = harness(app);
    let r = h.state().canvas_rect.unwrap();
    let guides = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.spreads[0].pages[0].guides.clone();
    drag(&mut h, pos2(r.center().x, r.min.y - 8.0), r.center());
    let g = guides(&h);
    assert_eq!(g.len(), 1, "{g:?}");
    assert!(g[0].liquid && g[0].orientation == designcraft_doc::Orientation::Horizontal);
    // With the Selection tool it's a ruler guide.
    h.state_mut().session.set_tool("selection");
    drag(&mut h, pos2(r.center().x, r.min.y - 8.0), r.center() + vec2(0.0, 40.0));
    let g = guides(&h);
    assert_eq!(g.len(), 2);
    assert!(!g[1].liquid);
}

/// Dragging a column guide with the Selection tool moves its gutter (one undo step); with
/// column guides locked the same drag leaves the columns alone.
#[test]
fn the_selection_tool_drags_a_column_guide() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    // Two columns across the 540 pt margin box: the gutter's left edge is at x = 300.
    app.run("layout.marginsAndColumns", json!({"parent": "A", "columns": 2, "gutter": 12})).unwrap();
    let mut h = harness(app);
    let page = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.spreads[0].pages[0].clone();
    let screen = |h: &Harness<'_, DesignApp>, x: f64| {
        let app = h.state();
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        let layout = CanvasLayout::new(&app.session.active().unwrap().doc, false);
        let slot = layout.slots.iter().find(|s| s.spread == SpreadRef::Doc(0)).unwrap();
        (xf.to_screen(slot.xf * Point::new(x, 400.0)), xf.zoom as f32)
    };
    let (at, zoom) = screen(&h, 300.0);
    drag(&mut h, at, at + vec2(72.0 * zoom, 0.0));
    assert_eq!(page(&h).columns.positions, None, "column guides are locked in a new document");
    h.hover_at(at);
    h.run_steps(2);
    assert_ne!(h.output().platform_output.cursor_icon, egui::CursorIcon::ResizeHorizontal);

    h.state_mut().run("view.lockColumnGuides", json!({"on": false})).unwrap();
    h.run_steps(2);
    let (at, zoom) = screen(&h, 300.0);
    drag(&mut h, at, at + vec2(72.0 * zoom, 0.0));
    let pg = page(&h);
    let cols = pg.column_rects();
    assert!((cols[0].x1 - 372.0).abs() < 1.0 && (cols[1].x0 - cols[0].x1 - 12.0).abs() < 1e-9, "{cols:?}");
    assert!(pg.local_margins, "the dragged page's columns are its own");
    // Over the moved guide: a resize cursor.
    h.hover_at(screen(&h, 372.0).0);
    h.run_steps(2);
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::ResizeHorizontal);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(page(&h).columns.positions, None, "one undo step");

    // Escape during the drag puts the gutter back.
    h.hover_at(at);
    h.step();
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    h.hover_at(at + vec2(40.0, 0.0));
    h.step();
    assert!(page(&h).columns.positions.is_some(), "the drag previews");
    h.event(egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() });
    h.step();
    h.event(egui::Event::PointerButton {
        pos: at + vec2(40.0, 0.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    h.run_steps(3);
    assert_eq!(page(&h).columns.positions, None);
    assert!(!page(&h).local_margins);

    // Space pressed during the drag and the button released under it: the drag ends with the
    // release and the gutter stops following the pointer.
    let key = |pressed| egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
    h.hover_at(at);
    h.step();
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.step();
    h.hover_at(at + vec2(40.0, 0.0));
    h.step();
    h.event(key(true));
    h.step();
    h.event(egui::Event::PointerButton {
        pos: at + vec2(40.0, 0.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    h.step();
    h.event(key(false));
    h.run_steps(2);
    let moved = page(&h).columns.positions;
    assert!(moved.is_some());
    h.hover_at(at + vec2(120.0, 0.0));
    h.run_steps(3);
    assert_eq!(page(&h).columns.positions, moved, "the gutter stays where the drag ended");
    assert!(h.state().session.active().unwrap().interaction.is_none());
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(page(&h).columns.positions, None, "the drag was one undo step");
}

/// A column guide is not picked where the screen mode hides guides, where an object on the
/// parent being edited lies over it, or where its columns cannot move.
#[test]
fn column_guides_are_picked_only_where_a_drag_can_move_them() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 1})).unwrap();
    app.run("layout.marginsAndColumns", json!({"parent": "A", "columns": 2, "gutter": 12})).unwrap();
    app.run("view.lockColumnGuides", json!({"on": false})).unwrap();
    let mut h = harness(app);
    let hit = |h: &Harness<'_, DesignApp>, r: SpreadRef, x: f64| {
        let app = h.state();
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        let st = app.session.active().unwrap();
        let layout = CanvasLayout::new(&st.doc, st.editing_parents);
        let slot = layout.slots.iter().find(|s| s.spread == r).unwrap();
        column_guide_at(app, &xf, xf.to_screen(slot.xf * Point::new(x, 400.0)))
    };
    assert!(hit(&h, SpreadRef::Doc(0), 300.0).is_some());
    h.state_mut().ui.screen_mode = ScreenMode::Preview;
    assert!(hit(&h, SpreadRef::Doc(0), 300.0).is_none(), "Preview hides the guides");
    h.state_mut().ui.screen_mode = ScreenMode::Normal;

    // Editing parent A: its right page (the one page 0 follows) and a frame over its gutter.
    h.state_mut().run("layout.parents.edit", json!({"on": true, "parent": "A-Parent"})).unwrap();
    h.run_steps(2);
    let parent = SpreadRef::Parent(0);
    let x = h.state().session.active().unwrap().doc.parents[0].pages[1].column_rects()[0].x1;
    assert!(hit(&h, parent, x).is_some());
    h.state_mut()
        .run("frame.create", json!({"spread": {"kind": "parent", "index": 0}, "rect": [x - 20.0, 300.0, x + 20.0, 500.0], "content": "text"}))
        .unwrap();
    h.run_steps(2);
    assert!(hit(&h, parent, x).is_none(), "the parent's frame takes the press");
    h.state_mut().run("layout.parents.edit", json!({"on": false})).unwrap();
    h.run_steps(2);

    // Gutters wider than the margin box: the even split cannot be moved.
    h.state_mut().run("layout.marginsAndColumns", json!({"pages": [0], "columns": 2, "gutter": 1000})).unwrap();
    h.run_steps(2);
    let x = h.state().session.active().unwrap().doc.spreads[0].pages[0].column_rects()[0].x1;
    assert!(hit(&h, SpreadRef::Doc(0), x).is_none());
}

fn right_click(h: &mut Harness<'_, DesignApp>, p: Pos2) {
    h.hover_at(p);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() });
    }
    h.run_steps(3);
}

fn new_document() -> Harness<'static, DesignApp> {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    harness(app)
}

/// [`new_document`] at 60 frames a second, the cadence a wheel motion really arrives at.
fn new_document_60fps() -> Harness<'static, DesignApp> {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    harness_with_step(app, 1.0 / 60.0)
}

fn held(modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::ModifiersChanged(modifiers)
}

/// Construct a point-unit vertical mouse-wheel event with an unknown source.
fn wheel(phase: egui::TouchPhase, up: f32, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, up), phase, modifiers, source: egui::MouseWheelSource::Unknown }
}

// Plays `events` with the pointer at `at`, a frame each, and lets the motion settle.
fn play(h: &mut Harness<'_, DesignApp>, at: Pos2, events: &[egui::Event]) -> View {
    h.hover_at(at);
    for event in events {
        h.event(event.clone());
        h.step();
    }
    h.run_steps(30);
    *h.state().view().unwrap()
}

/// Alt + wheel scrolled the view like the plain wheel; it zooms at the pointer (#470).
#[test]
fn alt_wheel_zooms_at_the_pointer() {
    use egui::{Modifiers, TouchPhase::Move};
    let mut h = new_document();
    let rect = h.state().canvas_rect.unwrap();
    let at = rect.center() + vec2(150.0, 90.0);
    let before = *h.state().view().unwrap();
    let under = Xf::new(rect, &before).to_canvas(at);
    let zoomed = play(&mut h, at, &[held(Modifiers::ALT), wheel(Move, 100.0, Modifiers::ALT)]);
    assert!(zoomed.zoom > before.zoom * 1.2, "wheel up zooms in: {} -> {}", before.zoom, zoomed.zoom);
    let still = Xf::new(rect, &zoomed).to_canvas(at);
    assert!((still.x - under.x).abs() < 0.01 && (still.y - under.y).abs() < 0.01, "the point under the pointer stays: {under:?} -> {still:?}");
    let back = play(&mut h, at, &[held(Modifiers::ALT), wheel(Move, -100.0, Modifiers::ALT)]);
    assert!((back.zoom - before.zoom).abs() < 1e-6, "wheel down zooms out: {} -> {}", zoomed.zoom, back.zoom);
    // The plain wheel still scrolls.
    let scrolled = play(&mut h, at, &[held(Modifiers::NONE), wheel(Move, 100.0, Modifiers::NONE)]);
    assert!(scrolled.zoom == back.zoom && scrolled.origin.y < back.origin.y, "{back:?} -> {scrolled:?}");
}

/// Alt zooms exactly as Ctrl/Cmd does, also when the key is let go while the wheel's motion is
/// still arriving: the rest of it zooms too and does not scroll the page.
#[test]
fn alt_wheel_zooms_like_ctrl_wheel_to_the_end_of_the_motion() {
    use egui::{
        Modifiers,
        TouchPhase::{End, Move, Start},
    };
    const NONE: Modifiers = Modifiers::NONE;
    // Each motion in steps; the views are compared after every step.
    let motions: [fn(Modifiers) -> Vec<Vec<egui::Event>>; 4] = [
        |key| vec![vec![held(key), wheel(Move, 100.0, key)]],
        // A wheel turned fast, the key let go a frame later.
        |key| vec![vec![held(key), wheel(Move, 400.0, key), held(NONE)]],
        // A trackpad swipe that coasts on after the key is let go; once it has ended, the wheel scrolls.
        |key| {
            vec![
                vec![held(key), wheel(Start, 0.0, key), wheel(Move, 50.0, key), held(NONE), wheel(Move, 200.0, NONE)],
                vec![wheel(End, 0.0, NONE), wheel(Move, 100.0, NONE)],
            ]
        },
        |key| vec![vec![held(key | Modifiers::SHIFT), wheel(Move, 100.0, key | Modifiers::SHIFT)]],
    ];
    for (i, motion) in motions.iter().enumerate() {
        let (mut with_alt, mut with_command) = (new_document_60fps(), new_document_60fps());
        let at = with_alt.state().canvas_rect.unwrap().center() + vec2(150.0, 90.0);
        let start = *with_alt.state().view().unwrap();
        for (alt, command) in motion(Modifiers::ALT).iter().zip(motion(Modifiers::COMMAND)) {
            let (alt, command) = (play(&mut with_alt, at, alt), play(&mut with_command, at, &command));
            assert!(command.zoom > start.zoom, "motion {i} zooms");
            assert_eq!(alt, command, "motion {i}");
        }
    }
}

fn units(h: &Harness<'_, DesignApp>) -> (Unit, Unit) {
    let s = &h.state().session.doc().unwrap().doc.settings;
    (s.horizontal_units, s.vertical_units)
}

fn zoom_app(tool: &str) -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.select_tool(tool);
    app.ui.rulers = false;
    app
}

/// Run one app frame with the supplied modifiers, install fonts, draw the canvas when ready and return the cursor icon.
fn canvas_frame(app: &mut DesignApp, ctx: &egui::Context, m: egui::Modifiers, mut events: Vec<egui::Event>) -> egui::CursorIcon {
    events.insert(0, egui::Event::ModifiersChanged(m));
    let mut output =
        ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), events, ..Default::default() }, |ui| {
            app.logic(ui.ctx());
            // Fonts are installed by the first real UI pass, just as in the full application.
            app.ui_setup(ui.ctx());
            if app.fonts_ready {
                show(app, ui);
            }
        });
    output.textures_delta.clear();
    output.platform_output.cursor_icon
}

/// The cursor icon the canvas sets for `c` on this platform (bitmap and painted cursors
/// report their own icon).
fn shown(c: Cursor) -> egui::CursorIcon {
    crate::cursors::show(&egui::Context::default(), &mut crate::cursors::Images::default(), c, Pos2::ZERO)
}

fn space_key(pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed, repeat: false, modifiers }
}

fn press(pos: Pos2, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers }
}

fn hover(app: &mut DesignApp, ctx: &egui::Context, p: Pos2) {
    for _ in 0..3 {
        canvas_frame(app, ctx, egui::Modifiers::NONE, vec![egui::Event::PointerMoved(p)]);
    }
}

const CMD: egui::Modifiers = egui::Modifiers { command: true, mac_cmd: true, ..egui::Modifiers::NONE };

#[test]
fn spring_zoom_click_is_anchored_view_only_and_release_restores_previous_tool() {
    for tool in ["rectangleFrame", "directSelection", "type", "hand", "zoom"] {
        let mut app = zoom_app(tool);
        if tool == "rectangleFrame" {
            let frame = app.run("frame.create", json!({"rect":[36,36,300,200],"content":"text","text":"Hello"})).unwrap();
            app.run("text.select", json!({"story":frame["story"],"anchor":5})).unwrap();
        }
        let ctx = egui::Context::default();
        let p = pos2(300.0, 250.0);
        hover(&mut app, &ctx, p);
        let before = *app.view().unwrap();
        let doc = app.session.doc().unwrap().doc.clone();
        let selection = app.session.doc().unwrap().selection.clone();
        let history = app.run("document.history", json!({})).unwrap();
        let cursor = canvas_frame(&mut app, &ctx, CMD, vec![space_key(true, CMD)]);
        assert_eq!(cursor, shown(Cursor::ZoomIn), "{tool}");
        canvas_frame(&mut app, &ctx, CMD, vec![press(p, true, CMD)]);
        canvas_frame(&mut app, &ctx, CMD, vec![press(p, false, CMD)]);
        let after = *app.view().unwrap();
        assert_eq!(after.zoom, before.zoom * 2.0, "{tool}");
        let anchor = Xf::new(app.canvas_rect.unwrap(), &before).to_canvas(p);
        assert!((Xf::new(app.canvas_rect.unwrap(), &after).to_canvas(p) - anchor).hypot() < 1e-9);
        let cursor = canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![space_key(false, egui::Modifiers::NONE)]);
        assert_eq!(app.session.tool_id(), tool);
        let expected = app.session.cursor(anchor, Mods::default(), app.view_info());
        assert_eq!(cursor, shown(expected), "{tool}");
        assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc().unwrap().doc));
        assert_eq!(selection, app.session.doc().unwrap().selection);
        assert_eq!(history, app.run("document.history", json!({})).unwrap());
    }
}

#[test]
fn spring_zoom_alt_clicks_out_and_modifier_release_switches_back_to_hand() {
    let mut app = zoom_app("rectangleFrame");
    let ctx = egui::Context::default();
    let p = pos2(300.0, 250.0);
    hover(&mut app, &ctx, p);
    let z = app.view().unwrap().zoom;
    let alt = egui::Modifiers { alt: true, ..CMD };
    assert_eq!(canvas_frame(&mut app, &ctx, CMD, vec![space_key(true, CMD)]), shown(Cursor::ZoomIn));
    assert_eq!(canvas_frame(&mut app, &ctx, alt, vec![]), shown(Cursor::ZoomOut));
    canvas_frame(&mut app, &ctx, alt, vec![press(p, true, alt)]);
    canvas_frame(&mut app, &ctx, alt, vec![press(p, false, alt)]);
    assert_eq!(app.view().unwrap().zoom, z * 0.5);
    assert_eq!(canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![]), shown(Cursor::Hand));
    let before = *app.view().unwrap();
    canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(p, true, egui::Modifiers::NONE)]);
    canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![egui::Event::PointerMoved(p + vec2(80.0, 0.0))]);
    canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(p + vec2(80.0, 0.0), false, egui::Modifiers::NONE)]);
    assert_eq!(app.view().unwrap().zoom, before.zoom);
    assert!(app.view().unwrap().origin.x < before.origin.x);
    assert_eq!(app.session.tool_id(), "rectangleFrame");
}

#[test]
fn spring_zoom_scrubs_and_owns_pointer_release_after_shortcut_release() {
    let mut app = zoom_app("rectangleFrame");
    let ctx = egui::Context::default();
    let p = pos2(300.0, 250.0);
    hover(&mut app, &ctx, p);
    let z = app.view().unwrap().zoom;
    let doc = app.session.doc().unwrap().doc.clone();
    // Ctrl is the platform command modifier on Windows/Linux, using the same input path.
    let cmd = egui::Modifiers { command: true, ctrl: true, ..egui::Modifiers::NONE };
    canvas_frame(&mut app, &ctx, cmd, vec![space_key(true, cmd), press(p, true, cmd)]);
    let end = p + vec2(150.0, 0.0);
    canvas_frame(&mut app, &ctx, cmd, vec![egui::Event::PointerMoved(end)]);
    assert!((app.view().unwrap().zoom - z * std::f64::consts::E).abs() < 1e-9);
    let scrubbed = app.view().unwrap().zoom;
    assert_eq!(canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![space_key(false, egui::Modifiers::NONE)]), shown(Cursor::Crosshair));
    canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(end, false, egui::Modifiers::NONE)]);
    assert_eq!(app.view().unwrap().zoom, scrubbed, "no extra click zoom at the end of a scrub");
    assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc().unwrap().doc));
    // The original tool still draws after the temporary gesture.
    canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(p, true, egui::Modifiers::NONE)]);
    canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![egui::Event::PointerMoved(p + vec2(50.0, 50.0))]);
    canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(p + vec2(50.0, 50.0), false, egui::Modifiers::NONE)]);
    assert!(!std::sync::Arc::ptr_eq(&doc, &app.session.doc().unwrap().doc));
    assert_eq!(app.session.tool_id(), "rectangleFrame");
}

#[test]
fn spring_zoom_is_suppressed_while_typing_in_a_text_frame() {
    let mut app = zoom_app("type");
    let frame = app.run("frame.create", json!({"rect":[36,36,300,200],"content":"text","text":"Hello"})).unwrap();
    app.run("text.select", json!({"story":frame["story"],"anchor":5})).unwrap();
    assert!(app.session.wants_text());
    let ctx = egui::Context::default();
    let p = pos2(300.0, 250.0);
    hover(&mut app, &ctx, p);
    let before = *app.view().unwrap();
    let alt = egui::Modifiers { alt: true, ..CMD };
    for m in [CMD, alt] {
        app.run("text.select", json!({"story":frame["story"],"anchor":5})).unwrap();
        assert!(app.session.wants_text());
        assert!(!matches!(canvas_frame(&mut app, &ctx, m, vec![space_key(true, m)]), egui::CursorIcon::ZoomIn | egui::CursorIcon::ZoomOut));
        canvas_frame(&mut app, &ctx, m, vec![press(p, true, m)]);
        canvas_frame(&mut app, &ctx, m, vec![press(p, false, m)]);
        assert_eq!(app.view().unwrap().zoom, before.zoom);
        assert_eq!(app.view().unwrap().origin, before.origin);
        canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![space_key(false, egui::Modifiers::NONE)]);
    }
}

#[test]
fn spring_zoom_is_suppressed_while_a_text_field_has_focus() {
    let mut app = zoom_app("rectangleFrame");
    let ctx = egui::Context::default();
    let mut text = String::new();
    let p = pos2(300.0, 250.0);
    hover(&mut app, &ctx, p);
    let before = *app.view().unwrap();
    for m in [egui::Modifiers::NONE, CMD, egui::Modifiers { alt: true, ..CMD }] {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                events: vec![egui::Event::ModifiersChanged(m), space_key(true, m), egui::Event::Text("x".into()), egui::Event::PointerMoved(p)],
                ..Default::default()
            },
            |ui| {
                ui.text_edit_singleline(&mut text).request_focus();
                show(&mut app, ui);
            },
        );
        output.textures_delta.clear();
        assert!(ctx.text_edit_focused());
        assert!(!matches!(output.platform_output.cursor_icon, egui::CursorIcon::ZoomIn | egui::CursorIcon::ZoomOut | egui::CursorIcon::Grab));
        assert_eq!(app.view().unwrap().zoom, before.zoom);
        assert_eq!(app.session.tool_id(), "rectangleFrame");
    }
    assert!(!text.is_empty(), "typing still reaches the field");
}

#[test]
fn spring_zoom_does_not_interrupt_an_existing_tool_drag() {
    let mut app = zoom_app("rectangleFrame");
    let ctx = egui::Context::default();
    let p = pos2(300.0, 250.0);
    hover(&mut app, &ctx, p);
    let before = *app.view().unwrap();
    let doc = app.session.doc().unwrap().doc.clone();
    canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(p, true, egui::Modifiers::NONE)]);
    let end = p + vec2(100.0, 100.0);
    canvas_frame(&mut app, &ctx, CMD, vec![space_key(true, CMD), egui::Event::PointerMoved(end)]);
    canvas_frame(&mut app, &ctx, CMD, vec![press(end, false, CMD)]);
    assert_eq!(app.view().unwrap().zoom, before.zoom);
    assert_eq!(app.view().unwrap().origin, before.origin);
    assert!(!std::sync::Arc::ptr_eq(&doc, &app.session.doc().unwrap().doc));
}

#[test]
fn spring_zoom_does_not_zoom_on_the_rulers() {
    let mut app = zoom_app("rectangleFrame");
    app.ui.rulers = true;
    let ctx = egui::Context::default();
    hover(&mut app, &ctx, pos2(300.0, 250.0));
    let rect = app.canvas_rect.unwrap();
    let p = pos2(rect.min.x - RULER / 2.0, rect.center().y);
    hover(&mut app, &ctx, p);
    let before = *app.view().unwrap();
    canvas_frame(&mut app, &ctx, CMD, vec![space_key(true, CMD)]);
    canvas_frame(&mut app, &ctx, CMD, vec![press(p, true, CMD)]);
    canvas_frame(&mut app, &ctx, CMD, vec![press(p, false, CMD)]);
    assert_eq!(app.view().unwrap().zoom, before.zoom);
    assert_eq!(app.view().unwrap().origin, before.origin);
}

#[test]
fn parent_guides_on_a_page_are_not_picked_there() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    // 100 pt into A's right page, which page 1 (a right page at x = 0) shows.
    let parent_guide = json!({"spread": {"kind": "parent", "index": 0}, "page": 1, "orientation": "vertical", "position": 712});
    app.run("guide.add", parent_guide).unwrap();
    let mut h = harness(app);
    let at = |app: &DesignApp| {
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        let layout = CanvasLayout::new(&app.session.active().unwrap().doc, false);
        let slot = layout.slots.iter().find(|s| s.spread == SpreadRef::Doc(0)).unwrap();
        guide_at(app, &xf, xf.to_screen(slot.xf * Point::new(100.0, 300.0)))
    };
    assert_eq!(at(h.state()), None, "the parent's guide is edited on the parent");
    h.state_mut().run("guide.add", json!({"orientation": "vertical", "position": 100})).unwrap();
    h.run_steps(2);
    assert!(matches!(at(h.state()), Some((SpreadRef::Doc(0), 0, 0, _))), "the page's own guide is");
}

#[test]
fn mouse_double_clicking_a_frame_handle_fits_with_one_undo_step() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = designcraft_doc::ItemId(
        app.run("frame.create", json!({"rect": [100, 100, 400, 300], "content": "text", "text": "Fit this text"})).unwrap()["id"].as_u64().unwrap(),
    );
    app.run("selection.set", json!({"ids": [id.0]})).unwrap();
    let original = app.session.doc().unwrap().doc.item(id).unwrap().bounds();
    // 60 frames a second: the default 0.25 s step puts two clicks past egui's double-click delay.
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    let app = h.state();
    let st = app.session.doc().unwrap();
    let layout = CanvasLayout::new(&st.doc, false);
    let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    let p = xf.to_screen(layout.xf(SpreadRef::Doc(0)) * Point::new(original.x1, original.y1));
    h.hover_at(p);
    h.step();
    // Separate frames model the actual mouse, including egui's release-time double click.
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
            h.step();
        }
    }
    h.run_steps(2);
    let fitted = h.state().session.doc().unwrap().doc.item(id).unwrap().bounds();
    assert!(fitted.width() < original.width() && fitted.height() < original.height(), "{fitted:?}");
    assert_eq!(h.state().session.tool_id(), "selection");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(h.state().session.doc().unwrap().doc.item(id).unwrap().bounds(), original);
}

const ART: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50" width="100pt" height="50pt"><path d="M0,0 C30,0 70,50 100,50" fill="none" stroke="#000" stroke-width="2"/></svg>"##;

#[test]
fn pasting_svg_from_another_app_places_editable_paths_in_the_middle_of_the_view() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let mut h = harness(app);
    h.run_steps(2);
    let undo = h.state().session.doc().unwrap().history.undo.len();
    h.event(egui::Event::Paste(ART.into()));
    h.run_steps(2);
    let app = h.state();
    let st = app.session.doc().unwrap();
    assert_eq!(st.history.undo.len(), undo + 1, "one undo step");
    let id = *st.selection.items.first().expect("the pasted path is selected");
    let it = st.doc.item(id).unwrap();
    assert_eq!(it.shape, designcraft_doc::Shape::Path);
    assert_eq!(it.path.subpaths[0].anchors.len(), 2);
    assert!((it.stroke.weight - 2.0).abs() < 1e-9);
    // Centred where text from another app would go: the middle of the view.
    let (_, centre) = view_centre(app).unwrap();
    assert!((it.bounds().center() - centre).hypot() < 0.01, "{:?} vs {centre:?}", it.bounds());
}

#[test]
fn the_paste_in_place_shortcut_pastes_in_place_and_plain_paste_offsets() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let original = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
    app.run("edit.copy", json!({})).unwrap();
    let clip = app.object_clip_text.clone().expect("the copy's placeholder text");
    let mut h = harness(app);
    let paste = |h: &mut Harness<'static, DesignApp>, modifiers: egui::Modifiers| {
        // As egui-winit delivers the native Paste in Place chord: the chord's paste and the V release, no V press; here the
        // modifiers are released within the same frame.
        let release = egui::Event::Key { key: egui::Key::V, physical_key: None, pressed: false, repeat: false, modifiers };
        let none = egui::Event::ModifiersChanged(egui::Modifiers::NONE);
        h.input_mut().events.extend([egui::Event::ModifiersChanged(modifiers), egui::Event::Paste(clip.clone()), release, none]);
        h.run_steps(2);
        let st = h.state().session.doc().unwrap();
        let pasted = *st.selection.items.first().expect("the copy is selected");
        assert_ne!(pasted.0, id);
        st.doc.item(pasted).unwrap().bounds()
    };
    let chord = crate::test_window::command_modifiers() | egui::Modifiers::ALT | egui::Modifiers::SHIFT;
    assert_eq!(paste(&mut h, chord), original, "Paste in Place");
    let moved = paste(&mut h, egui::Modifiers::COMMAND);
    assert!((moved.x0 - original.x0 - 12.0).abs() < 1e-9 && (moved.y0 - original.y0 - 12.0).abs() < 1e-9, "{moved:?}");
    // The modifiers went down in an earlier frame than the paste.
    h.event_modifiers(egui::Event::Paste(clip.clone()), chord);
    h.run_steps(2);
    let st = h.state().session.doc().unwrap();
    let pasted = *st.selection.items.first().expect("the copy is selected");
    assert_eq!(st.doc.item(pasted).unwrap().bounds(), original);
}

#[test]
fn edit_paste_from_the_menu_pastes_svg_on_the_system_clipboard_as_paths() {
    let services = crate::Services { clipboard_text: Some(Box::new(|| Some(ART.to_string()))), ..Default::default() };
    let mut app = DesignApp::new(designcraft_engine::Session::new(), services);
    app.native_menu = true;
    app.run("file.new", json!({})).unwrap();
    crate::menus::activate(&mut app, "edit.paste", &Value::Null);
    let st = app.session.doc().unwrap();
    let id = *st.selection.items.first().expect("the pasted path is selected");
    assert_eq!(st.doc.item(id).unwrap().shape, designcraft_doc::Shape::Path);
    assert!(st.selection.text.is_none(), "paths, not a text frame of markup");
    // With an insertion point, the markup is text like any other.
    let story = app.run("frame.create", json!({"rect": [10, 10, 300, 300], "content": "text"})).unwrap()["story"].clone();
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
    let items = app.session.doc().unwrap().doc.spreads[0].items.len();
    crate::menus::activate(&mut app, "edit.paste", &Value::Null);
    assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), items, "no paths added");
}

#[test]
fn selected_merged_owners_paint_the_same_row_and_column_targets() {
    for column in [true, false] {
        for merged in [false, true] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.run("file.new", json!({"pages": 1})).unwrap();
            app.run("frame.create", json!({"rect": [36, 36, 300, 300], "content": "text"})).unwrap();
            app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
            if merged {
                let range = if column { json!({"rows": [0, 0], "cols": [0, 1]}) } else { json!({"rows": [0, 1], "cols": [0, 0]}) };
                app.run("table.select", range).unwrap();
                app.run("table.merge", json!({})).unwrap();
            }
            app.run("table.select", json!({"rows": [1, 1], "cols": [1, 1]})).unwrap();
            app.run(if column { "table.selectColumn" } else { "table.selectRow" }, json!({})).unwrap();
            let st = app.session.doc().unwrap();
            let selection = st.selection.clone();
            let ts = selection.text.unwrap();
            let selected = selection.cells.unwrap();
            assert_eq!(
                selected.range,
                if column { designcraft_doc::CellRange::new(0, 1, 1, 1) } else { designcraft_doc::CellRange::new(1, 0, 1, 1) }
            );
            let layout = CanvasLayout::new(&st.doc, st.editing_parents);
            let cs = app.session.cache.get(&st.doc, ts.story, None);
            let ft = &cs.frames[0];
            let table = ft.tables.iter().find(|t| t.table == selected.table).unwrap();
            let (a, _) = item_canvas_xf(&st.doc, &layout, ft.frame).unwrap();
            let m = a * st.doc.text_xf(st.doc.item(ft.frame).unwrap());
            let xf = Xf { min: pos2(0.0, 0.0), origin: Point::new(0.0, 0.0), zoom: 1.0, rot: 0, pivot: pos2(0.0, 0.0) };
            let owners = if merged {
                [(0, 0), (1, 1)]
            } else if column {
                [(0, 1), (1, 1)]
            } else {
                [(1, 0), (1, 1)]
            };
            let expected: Vec<_> = owners
                .into_iter()
                .map(|(r, c)| {
                    let rect = table.cells.iter().find(|cell| cell.row == r && cell.col == c).unwrap().rect;
                    vec![
                        xf.to_screen(m * Point::new(rect.x0, rect.y0)),
                        xf.to_screen(m * Point::new(rect.x1, rect.y0)),
                        xf.to_screen(m * Point::new(rect.x1, rect.y1)),
                        xf.to_screen(m * Point::new(rect.x0, rect.y1)),
                    ]
                })
                .collect();
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                draw_text_selection(&app, ui.painter(), &xf, &st.doc, &layout, ts, Some(selected));
            });
            output.textures_delta.clear();
            let tint = Color32::from_rgba_unmultiplied(80, 140, 255, 90);
            let painted: Vec<_> = output
                .shapes
                .into_iter()
                .filter_map(|s| match s.shape {
                    egui::Shape::Path(path) if path.fill == tint => {
                        assert!(path.closed);
                        assert_eq!(path.stroke, egui::epaint::PathStroke::NONE);
                        Some(path.points)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(painted, expected, "only the two touched owners are tinted; column={column}, merged={merged}");
            assert_eq!(app.session.doc().unwrap().selection, selection);
        }
    }
}

/// A selected group lists the text frames inside it for ports.
#[test]
fn grouped_text_frames_get_ports() {
    let mut s = designcraft_engine::Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let a = s.execute("frame.create", &json!({"rect": [10, 10, 110, 60], "content": "text"})).unwrap()["id"].as_u64().unwrap();
    let b = s.execute("frame.create", &json!({"rect": [10, 80, 110, 130]})).unwrap()["id"].as_u64().unwrap();
    s.execute("selection.set", &json!({"ids": [a, b]})).unwrap();
    let g = s.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap();
    let doc = &s.doc().unwrap().doc;
    let group = doc.item(designcraft_doc::ItemId(g)).unwrap();
    let mut out = Vec::new();
    text_frames_in(group, 0, &mut out);
    assert_eq!(out.iter().map(|(it, _)| it.id.0).collect::<Vec<_>>(), vec![a], "one text frame in the group");
    // A plain text frame lists itself.
    out.clear();
    text_frames_in(out_frame(doc, a), 0, &mut out);
    assert_eq!(out.len(), 1);
}

/// A selected group paints both ports of the overset text frame inside it, on top of the
/// group's own selection marks.
#[test]
fn a_selected_group_paints_both_ports_of_its_text_frame() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let s = &mut app.session;
    let text = "far too much text for a frame this small, over and over again ".repeat(8);
    let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 140], "content": "text", "text": text})).unwrap()["id"].as_u64().unwrap();
    let b = s.execute("frame.create", &json!({"rect": [100, 200, 200, 260]})).unwrap()["id"].as_u64().unwrap();
    s.execute("selection.set", &json!({"ids": [a, b]})).unwrap();
    let g = s.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap();
    s.execute("transform.rotate", &json!({"angle": 30, "ids": [g]})).unwrap();
    s.execute("selection.set", &json!({"ids": [g]})).unwrap();
    let doc = app.session.doc().unwrap().doc.clone();
    let layout = CanvasLayout::new(&doc, false);
    let xf = Xf { min: pos2(0.0, 0.0), origin: Point::new(0.0, 0.0), zoom: 1.0, rot: 0, pivot: pos2(0.0, 0.0) };
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        draw_selection(&app, ui.painter(), &xf, &doc, &layout);
    });
    output.textures_delta.clear();
    let white: Vec<Rect> = output
        .shapes
        .into_iter()
        .filter_map(|s| match s.shape {
            egui::Shape::Rect(r) if r.fill == Color32::WHITE => Some(r.rect),
            _ => None,
        })
        .collect();
    for out in [false, true] {
        let c = xf.to_screen(designcraft_tools::thread::port_center(&doc, &layout, designcraft_doc::ItemId(a), out, xf.zoom).unwrap());
        let port = white.iter().position(|r| (r.center() - c).length() < 0.01).unwrap_or_else(|| panic!("port (out: {out}) at {c:?}"));
        assert!(!white[port + 1..].iter().any(|r| r.contains(c)), "nothing painted later covers the port (out: {out})");
    }
}

fn out_frame(doc: &Document, id: u64) -> &Item {
    doc.item(designcraft_doc::ItemId(id)).unwrap()
}

/// An overset frame inside a group is listed for the red mark even when nothing is
/// selected; a frame with room, or a hidden one, is not.
#[test]
fn overset_frames_are_found_inside_groups() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let s = &mut app.session;
    let frame_with = |s: &mut designcraft_engine::Session, rect: [f64; 4], text: &str| {
        s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": text})).unwrap()["id"].as_u64().unwrap()
    };
    let tiny = frame_with(s, [10.0, 10.0, 40.0, 16.0], "far too much text for a frame this small, over and over again");
    let roomy = frame_with(s, [10.0, 100.0, 300.0, 300.0], "fits");
    s.execute("selection.set", &json!({"ids": [tiny, roomy]})).unwrap();
    s.execute("object.group", &json!({})).unwrap();
    s.execute("selection.set", &json!({"ids": []})).unwrap();
    let doc = app.session.doc().unwrap().doc.clone();
    let mut out = Vec::new();
    overset_frames(&app, &doc, &doc.spreads[0].items, 0, &mut out);
    assert_eq!(out.iter().map(|it| it.id.0).collect::<Vec<_>>(), vec![tiny]);
    // Hidden: no mark.
    app.session.execute("object.hide", &json!({"ids": [tiny]})).unwrap();
    let doc = app.session.doc().unwrap().doc.clone();
    out.clear();
    overset_frames(&app, &doc, &doc.spreads[0].items, 0, &mut out);
    assert!(out.is_empty());
}

/// The always-on overset mark sits at the frame's out port, a step along its own right edge
/// (so a rotated frame gets it inside its edge, not straight up from the corner), and a
/// selected overset frame gets only the mark its ports draw.
#[test]
fn overset_mark_sits_on_the_out_port_once() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let s = &mut app.session;
    let id = s
        .execute("frame.create", &json!({"rect": [100, 100, 200, 160], "content": "text", "text": "far too much text for a frame this small, over and over again ".repeat(8)}))
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    s.execute("transform.rotate", &json!({"angle": 90, "ids": [id]})).unwrap();
    s.execute("selection.set", &json!({"ids": []})).unwrap();
    let doc = app.session.doc().unwrap().doc.clone();
    let layout = CanvasLayout::new(&doc, false);
    let zoom = 1.0;
    let marks = overset_marks(&app, &doc, &layout, &doc.spreads[0].items, zoom, &ported_frames(&app, &doc));
    assert_eq!(marks.len(), 1);
    let it = out_frame(&doc, id);
    let (a, _) = item_canvas_xf(&doc, &layout, it.id).unwrap();
    let m = a * it.xf;
    let r = it.inner_bounds();
    let corner = m * Point::new(r.x1, r.y1);
    let edge = (m * Point::new(r.x1, r.y0)) - corner;
    let d = marks[0].1 - corner;
    assert!((d.x * edge.y - d.y * edge.x).abs() < 1e-6, "the mark lies on the frame's right edge: {d:?} along {edge:?}");
    assert!((d.hypot() - 12.0 / zoom).abs() < 1e-6, "12 screen px from the corner: {d:?}");
    // Selected: the ports draw the mark, so no always-on mark is added.
    app.session.execute("selection.set", &json!({"ids": [id]})).unwrap();
    let marks = overset_marks(&app, &doc, &layout, &doc.spreads[0].items, zoom, &ported_frames(&app, &doc));
    assert!(marks.is_empty());
}

#[test]
fn right_clicking_a_ruler_sets_that_rulers_units() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let mut h = harness(app);
    let rect = h.state().canvas_rect.unwrap();
    assert_eq!(units(&h), (Unit::Picas, Unit::Picas));
    // Vertical ruler: the current unit is checked; picking one sets only the vertical units.
    right_click(&mut h, pos2(rect.min.x - RULER / 2.0, rect.center().y));
    h.get_by_label("✓ Picas");
    h.get_by_label("   Millimeters").click();
    h.run_steps(3);
    assert_eq!(units(&h), (Unit::Picas, Unit::Millimeters));
    // Horizontal ruler.
    right_click(&mut h, pos2(rect.center().x, rect.min.y - RULER / 2.0));
    h.get_by_label("✓ Picas");
    h.get_by_label("   Inches").click();
    h.run_steps(3);
    assert_eq!(units(&h), (Unit::Inches, Unit::Millimeters));
    // The corner where the rulers meet sets both, as InDesign's: nothing is checked while
    // they differ.
    let corner = rect.min - vec2(RULER / 2.0, RULER / 2.0);
    right_click(&mut h, corner);
    assert!(h.query_by_label("✓ Inches").is_none() && h.query_by_label("✓ Millimeters").is_none());
    let _ = h.get_by_label_contains("Hide Rulers");
    let _ = h.get_by_label("   Lock Zero Point");
    assert!(h.query_by_label("   Delete All Guides on Spread").is_none(), "only on the horizontal ruler");
    h.get_by_label("   Points").click();
    h.run_steps(3);
    assert_eq!(units(&h), (Unit::Points, Unit::Points));
    // InDesign's list: no Q or Ha.
    right_click(&mut h, corner);
    h.get_by_label("✓ Points");
    assert!(h.query_by_label("   Q").is_none() && h.query_by_label("   Ha").is_none());
    let _ = h.get_by_label("   Inches Decimal");
    // The horizontal ruler's menu also deletes the spread's guides; Hide Rulers hides them.
    right_click(&mut h, pos2(rect.center().x, rect.min.y - RULER / 2.0));
    let _ = h.get_by_label("   Delete All Guides on Spread");
    h.get_by_label_contains("Hide Rulers").click();
    h.run_steps(3);
    assert!(!h.state().ui.rulers);
    // The page opens no units menu.
    right_click(&mut h, rect.center());
    assert!(h.query_by_label("   Points").is_none());
}

/// The live-corner widget draws on a selected rectangle frame; clicking it draws the diamonds.
#[test]
fn live_corner_widget_and_diamonds_draw() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [100, 100, 300, 250], "content": "graphic"})).unwrap();
    let id = app.session.doc().unwrap().doc.spreads[0].items[0].id;
    app.run("selection.set", json!({"ids": [id.0]})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_pixels_per_point(1.0).wgpu().build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    let screen = |h: &Harness<'_, DesignApp>, x: f64, y: f64| {
        let app = h.state();
        let layout = CanvasLayout::new(&app.session.doc().unwrap().doc, false);
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        xf.to_screen(layout.to_canvas(SpreadRef::Doc(0), Point::new(x, y)))
    };
    let widget = screen(&h, 300.0, 100.0) + vec2(0.0, 11.5);
    let diamond = screen(&h, 100.0, 100.0) + vec2(12.0, 0.0);
    let yellow = |h: &mut Harness<'_, DesignApp>, p: Pos2| -> Option<bool> {
        let img = match h.render() {
            Ok(img) => img,
            Err(e) => {
                eprintln!("skipped: no offscreen renderer ({e})");
                return None;
            }
        };
        let px = img.get_pixel(p.x.round() as u32, p.y.round() as u32).0;
        Some(px[0] > 200 && px[1] > 180 && px[2] < 100)
    };
    let Some(w) = yellow(&mut h, widget) else { return };
    assert!(w, "no widget at {widget:?}");
    assert_eq!(yellow(&mut h, diamond), Some(false));
    h.hover_at(widget);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: widget, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
        h.step();
    }
    h.run_steps(3);
    assert_eq!(yellow(&mut h, diamond), Some(true), "no diamond at {diamond:?}");
    assert_eq!(yellow(&mut h, widget), Some(false));
}

/// A harness whose frames are 1 ms apart, so two clicks make a double-click.
fn fast_harness(app: DesignApp) -> Harness<'static, DesignApp> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(0.001).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn primary(h: &mut Harness<'_, DesignApp>, pos: Pos2, pressed: bool) {
    h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
    h.run_steps(2);
}

fn corner_drag(h: &mut Harness<'_, DesignApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.step();
    primary(h, from, true);
    h.event(egui::Event::PointerMoved(from + (to - from) / 2.0));
    h.run_steps(2);
    h.event(egui::Event::PointerMoved(to));
    h.run_steps(2);
    primary(h, to, false);
}

fn corner_click(h: &mut Harness<'_, DesignApp>, p: Pos2) {
    h.hover_at(p);
    h.step();
    primary(h, p, true);
    primary(h, p, false);
}

/// Where the rulers measure from (canvas coordinates): the document's zero point on the
/// spread in view (its first page, for a page origin).
fn ruler_zero(app: &DesignApp, doc: &Document, layout: &CanvasLayout) -> Option<Point> {
    ruler_pieces(app, doc, layout, doc.zero_point()).first().map(|p| p.2)
}

fn zero_point(h: &Harness<'_, DesignApp>) -> [f64; 2] {
    h.state().session.doc().unwrap().doc.settings.zero_point
}

#[test]
fn the_ruler_corner_drags_resets_and_locks_the_zero_point() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 3})).unwrap();
    let mut h = fast_harness(app);
    let rect = h.state().canvas_rect.unwrap();
    let corner = rect.min - vec2(RULER / 2.0, RULER / 2.0);
    // A click that stays in the corner leaves the zero point.
    corner_click(&mut h, corner);
    assert_eq!(zero_point(&h), [0.0, 0.0]);
    // Dragged out, it lands where released, measured from the spread in view.
    let dest = rect.min + vec2(80.0, 50.0);
    let expected = {
        let app = h.state();
        let st = app.session.active().unwrap();
        let layout = CanvasLayout::new(&st.doc, false);
        let slot = layout.slots[current_slot(app, &layout).unwrap()];
        let sp = slot.to_spread(Xf::new(rect, app.view().unwrap()).to_canvas(dest));
        let b = st.doc.spread(slot.spread).unwrap().bounds();
        [sp.x - b.x0, sp.y - b.y0]
    };
    corner_drag(&mut h, corner, dest);
    let zp = zero_point(&h);
    assert!((zp[0] - expected[0]).abs() < 0.5 && (zp[1] - expected[1]).abs() < 0.5, "{zp:?} vs {expected:?}");
    assert!(zp[0].abs() > 1.0 && zp[1].abs() > 1.0, "moved: {zp:?}");
    // The rulers measure from the point it was dropped on.
    {
        let app = h.state();
        let st = app.session.active().unwrap();
        let layout = CanvasLayout::new(&st.doc, false);
        let o = ruler_zero(app, &st.doc, &layout).unwrap();
        let on_screen = Xf::new(rect, app.view().unwrap()).to_screen(o);
        assert!((on_screen - dest).length() < 0.5, "{on_screen:?} vs {dest:?}");
    }
    // A double-click resets it.
    corner_click(&mut h, corner);
    corner_click(&mut h, corner);
    assert_eq!(zero_point(&h), [0.0, 0.0]);
    // Locked from the corner's context menu, the corner ignores drags and double-clicks.
    right_click(&mut h, corner);
    h.get_by_label("   Lock Zero Point").click();
    h.run_steps(3);
    assert!(h.state().session.doc().unwrap().doc.settings.zero_point_locked);
    corner_drag(&mut h, corner, dest);
    assert_eq!(zero_point(&h), [0.0, 0.0]);
    // Unlocked again from the same menu.
    right_click(&mut h, corner);
    h.get_by_label("   Unlock Zero Point").click();
    h.run_steps(3);
    corner_drag(&mut h, corner, dest);
    assert_ne!(zero_point(&h), [0.0, 0.0]);
    h.state_mut().run("view.lockZeroPoint", json!({"on": true})).unwrap();
    corner_click(&mut h, corner);
    corner_click(&mut h, corner);
    assert_ne!(zero_point(&h), [0.0, 0.0], "a locked zero point isn't reset");
}

#[test]
fn cmd_dragging_the_ruler_corner_makes_a_guide_each_way() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let mut h = harness(app);
    let rect = h.state().canvas_rect.unwrap();
    let corner = rect.min - vec2(RULER / 2.0, RULER / 2.0);
    let dest = rect.min + vec2(120.0, 90.0);
    let (r, at) = {
        let app = h.state();
        let st = app.session.active().unwrap();
        CanvasLayout::new(&st.doc, false).spread_at(Xf::new(rect, app.view().unwrap()).to_canvas(dest)).unwrap()
    };
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
    h.step();
    corner_drag(&mut h, corner, dest);
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.step();
    let doc = h.state().session.doc().unwrap().doc.clone();
    let guides: Vec<_> = doc.spread(r).unwrap().pages.iter().flat_map(|p| p.guides.clone()).collect();
    assert_eq!(guides.len(), 2, "{guides:?}");
    let pos = |o: designcraft_doc::Orientation| guides.iter().find(|g| g.orientation == o).map(|g| g.position).unwrap();
    assert!((pos(designcraft_doc::Orientation::Horizontal) - at.y).abs() < 0.5);
    assert!((pos(designcraft_doc::Orientation::Vertical) - at.x).abs() < 0.5);
    assert!(guides.iter().all(|g| g.spread));
    assert_eq!(doc.settings.zero_point, [0.0, 0.0], "the zero point stays");
}

/// A guide being dragged out of a ruler says where it is, as the rulers measure it (#1282):
/// from the zero point, in the ruler units, and the label follows the pointer while it moves.
#[test]
fn a_dragged_guide_shows_where_it_is() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("view.zeroPoint", json!({"at": [100, 50]})).unwrap();
    let mut h = harness(app);
    let rect = h.state().canvas_rect.unwrap();
    let dest = rect.min + vec2(160.0, 110.0);
    let (xf, at) = {
        let app = h.state();
        let xf = Xf::new(rect, app.view().unwrap());
        let st = app.session.active().unwrap();
        (xf, CanvasLayout::new(&st.doc, false).spread_at(xf.to_canvas(dest)).unwrap().1)
    };
    let units = h.state().session.doc().unwrap().doc.settings.horizontal_units;
    for (orientation, label, want) in
        [(designcraft_doc::Orientation::Vertical, "X", at.x - 100.0), (designcraft_doc::Orientation::Horizontal, "Y", at.y - 50.0)]
    {
        let g = GuideDrag { orientation, from: None, at: Some(dest), mods: egui::Modifiers::NONE };
        let (l, v, _) = guide_measure(h.state(), &xf, &g).unwrap();
        assert_eq!(l, label);
        assert!((v - want).abs() < 1e-6, "{label}: {v} vs {want}");
        let text = guide_readout(h.state(), &xf, &g).unwrap();
        assert_eq!(text, format!("{label}: {}", designcraft_geom::units::format_measure(v, units)));
    }
    // While a guide is actually being dragged out of the top ruler, the readout is there.
    let ruler = pos2(rect.min.x + 200.0, rect.min.y - RULER / 2.0);
    h.hover_at(ruler);
    h.step();
    primary(&mut h, ruler, true);
    h.event(egui::Event::PointerMoved(dest));
    h.run_steps(2);
    let g = h.ctx.data(|d| d.get_temp::<GuideDrag>(egui::Id::unique(GUIDE_DRAG))).expect("dragging");
    assert_eq!(g.orientation, designcraft_doc::Orientation::Horizontal);
    assert!(guide_readout(h.state(), &xf, &g).is_some_and(|t| t.starts_with("Y: ")));
    primary(&mut h, dest, false);
}

/// Over a spread other than the one in the middle of the view, the readout measures that
/// spread, the one the guide is dropped on: it reads the position the guide gets.
#[test]
fn the_guide_readout_measures_the_spread_under_the_pointer() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 5})).unwrap();
    app.run("view.zeroPoint", json!({"at": [100, 50]})).unwrap();
    let h = harness(app);
    let rect = h.state().canvas_rect.unwrap();
    let xf = Xf::new(rect, h.state().view().unwrap());
    let st = h.state().session.active().unwrap();
    let layout = CanvasLayout::new(&st.doc, false);
    let current = current_slot(h.state(), &layout).unwrap();
    // A point inside the last spread, well away from the one in view.
    let last = layout.slots.last().unwrap();
    assert_ne!(layout.slots.iter().position(|s| s.spread == last.spread), Some(current), "the last spread isn't the one in view");
    let inside = Point::new(last.bounds.center().x, last.bounds.y0 + (last.bounds.y1 - last.bounds.y0) * 0.3);
    let (r, sp) = layout.spread_at(inside).unwrap();
    assert_eq!(r, last.spread);
    let g =
        GuideDrag { orientation: designcraft_doc::Orientation::Horizontal, from: None, at: Some(xf.to_screen(inside)), mods: egui::Modifiers::NONE };
    let (label, v, _) = guide_measure(h.state(), &xf, &g).unwrap();
    assert_eq!(label, "Y");
    // The guide would be stored at `sp.y` on that spread, which its ruler reads from the zero point.
    // (To screen and back in f32: a few hundred-thousandths of a point.)
    assert!((v - (sp.y - 50.0)).abs() < 1e-3, "{v} vs {}", sp.y - 50.0);
    // An existing guide of another spread can't be dropped there (it only moves on its own
    // spread): no readout over this one, a readout over its own.
    let first = layout.slots[current].spread;
    let moving = GuideDrag {
        orientation: designcraft_doc::Orientation::Horizontal,
        from: Some((first, 0, 0)),
        at: Some(xf.to_screen(inside)),
        mods: egui::Modifiers::NONE,
    };
    assert!(guide_measure(h.state(), &xf, &moving).is_none(), "an existing guide over another spread");
    let own = layout.slots[current].bounds;
    let over_own = Point::new(own.center().x, own.y0 + (own.y1 - own.y0) * 0.3);
    let moving_here = GuideDrag { at: Some(xf.to_screen(over_own)), ..moving };
    assert!(guide_measure(h.state(), &xf, &moving_here).is_some(), "over its own spread");
}

/// The guide readout stays on the canvas: below right of the pointer in the open, flipped
/// left or up near the right or bottom edge, and inside the canvas in every corner.
#[test]
fn the_guide_readout_stays_inside_the_canvas() {
    let area = Rect::from_min_max(pos2(50.0, 40.0), pos2(850.0, 640.0));
    let size = vec2(80.0, 14.0);
    let inside = |at: Pos2| {
        let r = Rect::from_min_size(at, size).expand(3.0);
        r.min.x >= area.min.x - 1e-3 && r.min.y >= area.min.y - 1e-3 && r.max.x <= area.max.x + 1e-3 && r.max.y <= area.max.y + 1e-3
    };
    assert_eq!(readout_at(pos2(400.0, 300.0), size, area), pos2(412.0, 312.0), "in the open: below right");
    let right = readout_at(pos2(845.0, 300.0), size, area);
    assert!(right.x + size.x < 845.0 && inside(right), "near the right edge: left of the pointer, {right:?}");
    let bottom = readout_at(pos2(400.0, 636.0), size, area);
    assert!(bottom.y + size.y < 636.0 && inside(bottom), "near the bottom: above it, {bottom:?}");
    for p in [area.min, pos2(area.max.x, area.min.y), area.max, pos2(area.min.x, area.max.y), pos2(-500.0, 2000.0)] {
        assert!(inside(readout_at(p, size, area)), "{p:?}");
    }
    // A canvas narrower than the label: pinned to its top left, not past it.
    let tiny = Rect::from_min_max(pos2(0.0, 0.0), pos2(40.0, 10.0));
    assert_eq!(readout_at(pos2(20.0, 5.0), size, tiny), pos2(3.0, 3.0));
}

#[test]
fn rulers_and_xy_fields_measure_from_the_zero_point_on_every_spread() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    // Spread 0: page 1; spread 1: pages 2 and 3.
    app.run("file.new", json!({"pages": 3})).unwrap();
    app.run("view.zeroPoint", json!({"at": [100, 50]})).unwrap();
    app.session.prefs.dimensions_include_stroke = false;
    let mut h = harness(app);
    let rect = h.state().canvas_rect.unwrap();
    for (spread, x0) in [(0usize, 136.0), (1, 736.0)] {
        let app = h.state_mut();
        let id = app.run("frame.create", json!({"spread": spread, "rect": [x0, 70, x0 + 50.0, 120]})).unwrap()["id"].clone();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        // X/Y (Control, Properties and Info panels).
        let r = crate::panels::sel_info(app).unwrap().page_rect;
        assert!((r.x0 - (x0 - 100.0)).abs() < 1e-6 && (r.y0 - 20.0).abs() < 1e-6, "spread {spread}: {r:?}");
        // The rulers, with that spread in view.
        let layout = CanvasLayout::new(&app.session.active().unwrap().doc, false);
        let slot = layout.slots[spread];
        let v = app.view_mut().unwrap();
        let zoom = v.zoom;
        let c = slot.bounds.center();
        v.origin = Point::new(c.x - rect.width() as f64 / 2.0 / zoom, c.y - rect.height() as f64 / 2.0 / zoom);
        h.run_steps(2);
        let app = h.state();
        let doc = app.session.active().unwrap().doc.clone();
        assert_eq!(current_slot(app, &layout), Some(spread));
        let o = ruler_zero(app, &doc, &layout).unwrap();
        assert!((o.x - slot.bounds.x0 - 100.0).abs() < 1e-6 && (o.y - slot.bounds.y0 - 50.0).abs() < 1e-6, "spread {spread}: {o:?}");
    }
}

/// Screen position of spread 0's point (x, y).
fn screen_of(h: &Harness<'_, DesignApp>, x: f64, y: f64) -> Pos2 {
    let app = h.state();
    let st = app.session.active().unwrap();
    let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    xf.to_screen(CanvasLayout::new(&st.doc, st.editing_parents).xf(SpreadRef::Doc(0)) * Point::new(x, y))
}

/// A menu row: a button that isn't a toggle, named by its label, then its shortcut if it has
/// one. Toggle buttons with the same name (the Task Bar's icons) don't match, nor do buttons
/// right of the canvas (the Properties panel's Arrange).
fn menu_row<'h>(h: &'h Harness<'_, DesignApp>, label: &'h str) -> Option<egui_kittest::Node<'h>> {
    let right = f64::from(h.state().canvas_rect.map_or(f32::INFINITY, |r| r.max.x));
    h.query_by(move |n| {
        n.role() == egui::accesskit::Role::Button
            && n.toggled().is_none()
            && n.bounding_box().is_none_or(|b| b.x0 < right)
            && n.label().is_some_and(|l| l == label || l.strip_prefix(label).and_then(|r| r.strip_prefix(' ')).is_some_and(|r| !r.contains(' ')))
    })
}

fn enabled(h: &Harness<'_, DesignApp>, label: &str) -> bool {
    use egui_kittest::kittest::NodeT;
    !menu_row(h, label).unwrap_or_else(|| panic!("no menu row {label}")).accesskit_node().is_disabled()
}

fn spread_items(h: &Harness<'_, DesignApp>) -> Vec<u64> {
    h.state().session.doc().unwrap().doc.spreads[0].items.iter().map(|i| i.id.0).collect()
}

fn selected(h: &Harness<'_, DesignApp>) -> Vec<u64> {
    h.state().session.doc().unwrap().selection.items.iter().map(|i| i.0).collect()
}

#[test]
fn right_clicking_an_object_selects_it_and_its_menu_arranges_it() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let b = app.run("frame.create", json!({"rect": [150, 150, 300, 300]})).unwrap()["id"].as_u64().unwrap();
    let mut h = harness(app);
    assert_eq!(selected(&h), [b]);
    let on_a = screen_of(&h, 60.0, 60.0);
    right_click(&mut h, on_a);
    assert_eq!(selected(&h), [a], "the object under the pointer is selected first");
    // An empty frame's menu: Content, but no Fitting (nothing to fit) and no text frame entries.
    assert!(menu_row(&h, "Content").is_some());
    assert!(menu_row(&h, "Fitting").is_none() && menu_row(&h, "Text Frame Options…").is_none());
    // Paste is available with nothing copied here: the system clipboard may hold another app's text.
    assert!(enabled(&h, "Copy") && enabled(&h, "Paste"));
    menu_row(&h, "Arrange").unwrap().click();
    h.run_steps(3);
    menu_row(&h, "Bring to Front").unwrap().click();
    h.run_steps(3);
    assert_eq!(spread_items(&h), [b, a]);
    assert!(menu_row(&h, "Arrange").is_none(), "choosing an entry closes the menu");
}

#[test]
fn right_clicking_empty_canvas_offers_paste_after_a_copy() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("edit.copy", json!({})).unwrap();
    app.select_tool("rectangleFrame");
    let mut h = harness(app);
    let empty = screen_of(&h, 400.0, 500.0);
    right_click(&mut h, empty);
    assert_eq!(selected(&h), [a], "a right-click on nothing keeps the selection");
    assert!(menu_row(&h, "Arrange").is_none(), "the page menu, not the object menu");
    assert!(enabled(&h, "Paste in Place"));
    assert!(menu_row(&h, "✓ Show/Hide Rulers").is_some(), "toggles show their state");
    // Escape closes the menu and does nothing else: the tool would deselect, the shortcuts
    // would switch to the Selection tool.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(menu_row(&h, "Paste in Place").is_none());
    assert_eq!((h.state().session.tool_id(), selected(&h)), ("rectangleFrame", vec![a]));
    right_click(&mut h, empty);
    menu_row(&h, "Paste").unwrap().click();
    h.run_steps(3);
    assert_eq!(spread_items(&h).len(), 2);
}

#[test]
fn right_clicking_another_object_while_the_menu_is_open_reopens_it_there() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let b = app.run("frame.create", json!({"rect": [36, 600, 200, 760]})).unwrap()["id"].as_u64().unwrap();
    let mut h = harness(app);
    // The menu opens down from A, with the pointer on its first row (no submenu open); B is below it.
    let (on_a, on_b) = (screen_of(&h, 60.0, 60.0), screen_of(&h, 50.0, 700.0));
    right_click(&mut h, on_a);
    assert_eq!(selected(&h), [a]);
    assert!(menu_row(&h, "Arrange").is_some());
    right_click(&mut h, on_b);
    assert_eq!(selected(&h), [b]);
    assert!(menu_row(&h, "Arrange").is_some(), "the menu reopens at the new click");
}

#[test]
fn a_context_menu_in_one_pane_keeps_the_keys_from_the_other() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 100, 100]})).unwrap()["id"].as_u64().unwrap();
    app.select_tool("rectangleFrame");
    app.split = true;
    let mut h = harness(app);
    // The focused pane is the left one; this point is near its right edge, away from the frame.
    let left = h.state().canvas_rect.unwrap();
    right_click(&mut h, pos2(left.max.x - 40.0, left.center().y));
    assert!(menu_row(&h, "Paste in Place").is_some());
    // Escape closes the left pane's menu before the right pane handles the keys.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(menu_row(&h, "Paste in Place").is_none());
    assert_eq!((h.state().session.tool_id(), selected(&h)), ("rectangleFrame", vec![a]));
}

#[test]
fn right_clicking_text_keeps_the_selected_text_or_places_the_caret() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [36, 36, 400, 200], "content": "text"})).unwrap();
    app.select_tool("type");
    app.run("text.insert", json!({"text": "Hello world"})).unwrap();
    let sid = *app.session.doc().unwrap().doc.stories.keys().next().unwrap();
    app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 5})).unwrap();
    let mut h = harness(app);
    let text_sel = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().selection.text.as_ref().map(|t| (t.anchor, t.focus));
    // Inside "Hello": the selection stays.
    let inside = screen_of(&h, 45.0, 44.0);
    right_click(&mut h, inside);
    assert_eq!(text_sel(&h), Some((0, 5)));
    assert!(menu_row(&h, "Paste without Formatting").is_some(), "the text menu");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    // Past the end of the line: the caret goes there.
    let past_end = screen_of(&h, 300.0, 44.0);
    right_click(&mut h, past_end);
    assert_eq!(text_sel(&h), Some((11, 11)));
    assert!(menu_row(&h, "Paste without Formatting").is_some());
}

#[test]
fn rows_hidden_in_the_menu_bar_are_hidden_in_context_menus() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap();
    app.ui.hidden_menu_items.push(crate::menus::menu_key("Object/Arrange", "Bring to Front"));
    let mut h = harness(app);
    let on_frame = screen_of(&h, 60.0, 60.0);
    right_click(&mut h, on_frame);
    menu_row(&h, "Arrange").unwrap().click();
    h.run_steps(3);
    assert!(menu_row(&h, "Bring to Front").is_none());
    assert!(menu_row(&h, "Send to Back").is_some());
    assert!(h.query_by_label("Show All Menu Items").is_some());
}

#[test]
fn a_secondary_drag_neither_moves_marquees_nor_draws() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let mut h = harness(app);
    // A secondary-button drag between spread points.
    let drag = |h: &mut Harness<'_, DesignApp>, from: (f64, f64), to: (f64, f64)| {
        let (from, to) = (screen_of(h, from.0, from.1), screen_of(h, to.0, to.1));
        h.hover_at(from);
        h.step();
        h.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Secondary, pressed: true, modifiers: Default::default() });
        h.step();
        for t in [0.25, 0.5, 0.75, 1.0] {
            h.event(egui::Event::PointerMoved(from + (to - from) * t));
            h.step();
        }
        h.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Secondary, pressed: false, modifiers: Default::default() });
        h.run_steps(3);
    };
    let xf = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.item(designcraft_doc::ItemId(a)).unwrap().xf;
    let before = xf(&h);
    // Selection tool: on the object (no move), and from empty page across it (no marquee).
    drag(&mut h, (100.0, 100.0), (300.0, 300.0));
    assert_eq!(xf(&h), before);
    h.state_mut().run("edit.deselectAll", json!({})).unwrap();
    drag(&mut h, (400.0, 400.0), (20.0, 20.0));
    assert!(selected(&h).is_empty());
    // Rectangle Frame tool: no frame.
    h.state_mut().select_tool("rectangleFrame");
    drag(&mut h, (300.0, 300.0), (500.0, 500.0));
    assert_eq!(spread_items(&h), [a]);
}

/// A text frame holding `text`, the Type tool, and a harness stepping at 60 fps (clicks in a
/// row must land within the double-click delay).
fn typing(text: &str) -> (Harness<'static, DesignApp>, u64) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let r = app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "text": text})).unwrap();
    let id = r["id"].as_u64().unwrap();
    app.run("tool.select", json!({"tool": "type"})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    (h, id)
}

/// The screen point over the first line of frame `id` where a click puts the caret at a byte
/// in `bytes`.
fn screen_at(h: &mut Harness<'_, DesignApp>, id: u64, bytes: std::ops::Range<usize>) -> Pos2 {
    let mut x = 74.0;
    let p = loop {
        assert!(x < 400.0, "no point on the first line hits {bytes:?}");
        h.state_mut().run("text.placeCaret", json!({"frame": id, "point": [x, 80.0]})).unwrap();
        if h.state().session.doc().unwrap().selection.text.is_some_and(|t| bytes.contains(&t.focus)) {
            break Point::new(x, 80.0);
        }
        x += 1.0;
    };
    h.state_mut().run("edit.deselectAll", json!({})).unwrap();
    let app = h.state();
    let c = app.session.layout().to_canvas(SpreadRef::Doc(0), p);
    Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(c)
}

fn click(h: &mut Harness<'_, DesignApp>, p: Pos2) {
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
        h.step();
    }
}

fn selected_range(h: &Harness<'_, DesignApp>) -> std::ops::Range<usize> {
    h.state().session.doc().unwrap().selection.text.map(|t| t.range()).unwrap_or_default()
}

#[test]
fn clicks_in_a_row_select_word_line_paragraph_with_return_and_story() {
    let text = "One two. Three four! Five six.\nSecond para here.";
    let (mut h, id) = typing(text);
    // Inside "Three": double-click selects the word, then line and paragraph with return.
    let p = screen_at(&mut h, id, 10..14);
    h.hover_at(p);
    h.step();
    let mut got = vec![];
    for _ in 0..5 {
        click(&mut h, p);
        let r = selected_range(&h);
        got.push(text[r].to_string());
    }
    assert_eq!(got[1..], ["Three", "One two. Three four! Five six.", "One two. Three four! Five six.\n", text], "{got:?}");
    assert!(got[0].is_empty(), "one click places the caret: {got:?}");
}

#[test]
fn esc_that_closes_a_dialog_keeps_the_type_tool_and_its_text_selection() {
    let (mut h, id) = typing("One two three");
    // A click in the text gives the canvas the keyboard focus, as when typing.
    let p = screen_at(&mut h, id, 5..6);
    h.hover_at(p);
    h.step();
    for _ in 0..2 {
        click(&mut h, p);
    }
    h.run_steps(4);
    let words = selected_range(&h);
    assert_eq!(words, 4..7, "the double-click selects \"two\"");
    let app = h.state_mut();
    let name = app.run("style.character.create", json!({"name": "Key Term"})).unwrap()["name"].as_str().unwrap().to_string();
    app.ui.dialog = Some(crate::dialogs::Dialog::new("characterStyleOptions", json!({"name": name})));
    h.run_steps(3);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().ui.dialog.is_none(), "Esc closes the dialog");
    assert_eq!((h.state().session.tool_id(), selected_range(&h)), ("type", words));
    // With no dialog open, Esc in the text still leaves it for the Selection tool.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert_eq!(h.state().session.tool_id(), "selection");
}

#[test]
fn slow_clicks_do_not_add_up() {
    let (mut h, id) = typing("One two. Three four!");
    let p = screen_at(&mut h, id, 10..14);
    h.hover_at(p);
    h.step();
    for _ in 0..3 {
        click(&mut h, p);
        h.run_steps(30);
    }
    assert!(selected_range(&h).is_empty(), "three clicks half a second apart leave a caret: {:?}", selected_range(&h));
}

/// A viewport wider than the GPU's largest texture: the canvas texture is sampled down, and an
/// edit repaints it whole. A region repainted in view pixels would land in the wrong place,
/// or outside the texture (a wgpu validation error). A view that fits is still patched.
#[test]
fn an_edit_repaints_a_canvas_texture_sampled_down_to_the_gpu_limit_whole() {
    for (width, sampled_down) in [(1440.0, false), (2600.0, true)] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let frame = app.run("frame.create", json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Text"})).unwrap();
        let mut h = Harness::builder().with_size(vec2(width, 900.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        h.run_steps(6);
        let limit = h.ctx.input(|i| i.max_texture_side);
        let size = h.state().canvas.texture.as_ref().map(|t| t.size()).unwrap();
        assert!(size[0] <= limit && size[1] <= limit, "{size:?}");
        let shown = h.state().canvas.shown.unwrap().size;
        assert_eq!(shown.0 > limit as f32, sampled_down, "view {shown:?}, limit {limit}");
        // A valid downsampled texture is final at this GPU limit. It must not keep
        // scheduling background renders merely because it is smaller than view pixels.
        h.run_steps(3);
        assert!(h.state().canvas.pending.is_none(), "an unchanged canvas is reused, width {width}");
        let patches = h.state().canvas.patches;
        // A small edit: one more character in the frame.
        h.state_mut().run("text.insert", json!({"story": frame["story"], "at": 4, "text": "s"})).unwrap();
        h.run_steps(6);
        assert_eq!(h.state().canvas.patches == patches, sampled_down, "width {width}");
        assert_eq!(h.state().canvas.texture.as_ref().map(|t| t.size()), Some(size));
    }
}

/// ⌥E then I types "í" on a Mac: the input method composes the dead key's accent, which
/// it does only for a window that turned it on. The canvas never did, so ⌥E typed "e".
#[test]
fn dead_keys_compose_through_the_input_method() {
    let (mut h, id) = typing("T");
    h.state_mut().run("text.placeCaret", json!({"frame": id, "point": [390.0, 80.0]})).unwrap();
    h.run_steps(2);
    assert!(h.output().platform_output.ime.is_some(), "a text caret turns the input method on");
    h.event(egui::Event::Ime(egui::ImeEvent::Preedit { text: "´".into(), active_range_chars: None }));
    h.run_steps(2);
    let text = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.stories.values().map(|s| s.text.clone()).collect::<String>();
    assert_eq!(text(&h), "T´", "the accent being composed shows in the story");
    assert!(h.state().session.tool_composing(), "and is marked as composed, not typed");
    h.event(egui::Event::Ime(egui::ImeEvent::Preedit { text: String::new(), active_range_chars: None }));
    h.event(egui::Event::Ime(egui::ImeEvent::Commit("í".into())));
    h.run_steps(2);
    assert_eq!(text(&h), "Tí");
    assert!(!h.state().session.tool_composing());
    // Moving the caret mid-composition keeps the accent as typed and tells the input method
    // to drop its copy.
    h.event(egui::Event::Ime(egui::ImeEvent::Preedit { text: "˜".into(), active_range_chars: None }));
    h.run_steps(2);
    h.state_mut().run("text.move", json!({"dir": "left"})).unwrap();
    h.step();
    assert_eq!(text(&h), "Tí˜");
    assert!(!h.state().session.tool_composing());
    assert!(h.output().platform_output.ime.as_ref().is_some_and(|i| i.should_interrupt_composition));
    assert!(h.state_mut().take_ime_discard());
    // Another tool takes no text: the input method is off.
    h.state_mut().run("tool.select", json!({"tool": "selection"})).unwrap();
    h.run_steps(2);
    assert!(h.output().platform_output.ime.is_none());
}

#[test]
fn tools_show_their_own_cursors_over_the_page() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let mut h = harness(app);
    let p = h.state().canvas_rect.unwrap().center();
    let mut shown = |tool: &str| {
        h.state_mut().run("tool.select", json!({"tool": tool})).unwrap();
        h.hover_at(p);
        h.run_steps(2);
        let out = &h.output().platform_output;
        (out.cursor_icon, out.cursor_image.clone())
    };
    let (pen, direct, text, selection) = (shown("pen"), shown("directSelection"), shown("type"), shown("selection"));
    let ppp = h.ctx.pixels_per_point();
    let images = &mut h.state_mut().cursors;
    match crate::cursors::mode() {
        crate::cursors::Mode::Os => {
            assert_eq!(pen.1, images.get(Cursor::Pen, ppp), "Pen");
            assert_eq!(direct.1, images.get(Cursor::ArrowHollow, ppp), "Direct Selection");
            assert_eq!(selection.1, images.get(Cursor::Arrow, ppp), "Selection");
            assert_eq!(text, (egui::CursorIcon::Text, None), "the I-beam stays a system cursor");
        }
        crate::cursors::Mode::Painted => {
            assert_eq!((pen.0, direct.0, selection.0), (egui::CursorIcon::None, egui::CursorIcon::None, egui::CursorIcon::None));
        }
        crate::cursors::Mode::System => assert_eq!(pen, (egui::CursorIcon::Crosshair, None)),
    }
}

#[test]
fn document_grid_is_drawn_where_points_snap_to_it() {
    use designcraft_tools::{SnapView, ToolContext};
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let doc = app.session.active().unwrap().doc.clone();
    let layout = CanvasLayout::new(&doc, false);
    let sr = SpreadRef::Doc(0);
    // Where the grid snaps a point in the first page (72 pt / 8 subdivisions = 9 pt).
    let page = doc.spreads[0].pages[0].bounds();
    let sel = Selection::default();
    let cx = ToolContext {
        doc: &doc,
        selection: &sel,
        cache: &app.session.cache,
        layout: &layout,
        zoom: 1.0,
        layer: doc.layers[0].id,
        snap: SnapView { snap_to_document_grid: true, zone_px: 20.0, ..SnapView::OFF },
        unit: Unit::Points,
        stroke_in_front: false,
    };
    let (snapped, _) = designcraft_tools::snap::snap_point(&cx, sr, Point::new(page.x0 + 31.3, page.y0 + 47.9));
    assert_eq!((snapped.x - page.x0, snapped.y - page.y0), (27.0, 45.0));
    // Render 40 × 40 pt around it at 4 px/pt.
    let scale = 4.0;
    let corner = layout.xf(sr) * Point::new(snapped.x - 20.0, snapped.y - 20.0);
    let view = Affine::scale(scale) * Affine::translate((-corner.x, -corner.y));
    let placed: Vec<_> = layout.slots.iter().map(|s| designcraft_render::Placed { spread: s.spread, xf: s.xf }).collect();
    let render = |app: &DesignApp| {
        let opts = screen_options(app, &layout, false, 1.0);
        designcraft_render::Renderer::new().render(&doc, &app.session.cache, &placed, 160, 160, view, &opts)
    };
    // The snapped point is at pixel (80, 80); halfway to the next line is 18 px further.
    let darkest = |img: &designcraft_render::Rendered, xs: [u32; 2], ys: [u32; 2]| {
        (xs[0]..xs[1]).flat_map(|x| (ys[0]..ys[1]).map(move |y| (x, y))).map(|(x, y)| img.pixel(x, y)[0]).min().unwrap()
    };
    let off = render(&app);
    assert_eq!(darkest(&off, [78, 82], [60, 64]), 255, "no grid while it is hidden");
    app.run("view.documentGrid", json!({})).unwrap();
    let on = render(&app);
    assert!(darkest(&on, [79, 81], [60, 64]) < 250, "a vertical line at the snapped x");
    assert!(darkest(&on, [60, 64], [79, 81]) < 250, "a horizontal line at the snapped y");
    assert_eq!(darkest(&on, [96, 100], [96, 100]), 255, "nothing between lines");
    // Preview hides it.
    app.ui.screen_mode = ScreenMode::Preview;
    assert!(screen_options(&app, &layout, true, 1.0).document_grid.is_empty());
}

/// egui moves keyboard focus on a plain arrow key or Tab, and Space/Return then clicks the
/// focused widget (e.g. the status bar's spread buttons) while typing still reaches the text.
#[test]
fn arrow_keys_and_tab_keep_keyboard_focus_on_the_canvas() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 3})).unwrap();
    let f = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Hello"})).unwrap();
    app.select_tool("type");
    let mut h = harness(app);
    let p = {
        let app = h.state();
        let st = app.session.active().unwrap();
        let layout = CanvasLayout::new(&st.doc, st.editing_parents);
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        xf.to_screen(layout.xf(SpreadRef::Doc(0)) * Point::new(100.0, 60.0))
    };
    h.hover_at(p);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
        h.step();
    }
    h.run_steps(2);
    let focus = h.ctx.memory(|m| m.focused());
    assert!(focus.is_some(), "a click on the canvas takes keyboard focus");
    let view = *h.state().view().unwrap();
    for key in [egui::Key::ArrowDown, egui::Key::Space, egui::Key::ArrowRight, egui::Key::Enter, egui::Key::Tab, egui::Key::Escape] {
        h.key_press(key);
        if key == egui::Key::Space {
            h.event(egui::Event::Text(" ".into()));
        }
        h.run_steps(2);
        assert_eq!(h.ctx.memory(|m| m.focused()), focus, "{key:?} keeps keyboard focus on the canvas");
    }
    let v = *h.state().view().unwrap();
    assert_eq!((v.origin, v.zoom), (view.origin, view.zoom), "the view stays where it was");
    let text = h.state_mut().session.execute("story.get", &json!({"story": f["story"]})).unwrap()["text"].clone();
    assert_eq!(text, "Hello \n\t", "Space, Return and Tab reach the text");
}

#[test]
fn rulers_open_no_units_menu_without_a_document() {
    let mut h = harness(DesignApp::new(designcraft_engine::Session::new(), crate::Services::default()));
    let Some(rect) = h.state().canvas_rect else { return };
    right_click(&mut h, pos2(rect.center().x, rect.min.y - RULER / 2.0));
    assert!(h.query_by_label("   Points").is_none());
}

/// One frame of the whole app with `events`; returns what it asks of the system IME.
fn ime_frame(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Option<egui::output::IMEOutput> {
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
    out.platform_output.ime
}

/// An edit patches the canvas texture in place only while that texture is at full resolution:
/// past the GPU's texture limit `upload` shrinks it, and a full-resolution patch written into
/// it is a renderer validation error.
#[test]
fn edits_patch_the_canvas_only_at_full_resolution() {
    for (max_side, patches) in [(None, true), (Some(1024), false)] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let frame = |app: &mut DesignApp| {
            // Wider than the limit (egui's font atlas needs at least 1024).
            let mut raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1900.0, 900.0))), ..Default::default() };
            if let Some(m) = max_side {
                raw.max_texture_side = Some(m);
            }
            let mut out = ctx.run_ui(raw, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        app.run("file.new", json!({})).unwrap();
        for _ in 0..3 {
            frame(&mut app);
        }
        app.run("frame.create", json!({"rect": [36, 36, 60, 60]})).unwrap();
        frame(&mut app);
        let size = app.canvas.texture.as_ref().unwrap().size();
        assert!(max_side.is_none_or(|m| size[0] <= m && size[1] <= m), "{size:?}");
        assert_eq!(app.canvas.patches > 0, patches, "limit {max_side:?}");
    }
}

/// The Type tool's caret at the end of a new text frame holding `text`, the app's first frames
/// drawn; returns the story id.
fn typing_app(app: &mut DesignApp, ctx: &egui::Context, text: &str) -> Value {
    app.run("file.new", json!({})).unwrap();
    let f = app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": text})).unwrap();
    let sid = f["story"].clone();
    app.session.execute("text.select", &json!({"story": sid, "anchor": text.len(), "focus": text.len()})).unwrap();
    app.select_tool("type");
    for _ in 0..4 {
        ime_frame(app, ctx, vec![]);
    }
    sid
}

fn story_text(app: &mut DesignApp, sid: &Value) -> String {
    app.session.execute("story.get", &json!({"story": sid})).unwrap()["text"].as_str().unwrap().to_string()
}

/// Spread point `p` on screen.
fn on_screen(app: &DesignApp, p: Point) -> Pos2 {
    let st = app.session.active().unwrap();
    let c = CanvasLayout::new(&st.doc, st.editing_parents).to_canvas(SpreadRef::Doc(0), p);
    Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(c)
}

fn preedit(t: &str, c: usize) -> egui::Event {
    egui::Event::Ime(egui::ImeEvent::Preedit { text: t.into(), active_range_chars: Some(c..c) })
}

/// Extract `(surrounding text, cursor, anchor)` from an IME output.
fn ime_payload(ime: &egui::output::IMEOutput) -> Option<(&str, usize, usize)> {
    ime.surrounding_text.available().map(|p| (p.text.as_str(), p.cursor, p.anchor))
}

/// Set an engine text selection for a test fixture.
fn text_selection(app: &mut DesignApp, sid: &Value, anchor: usize, focus: usize) {
    app.session.execute("text.select", &json!({"story": sid, "anchor": anchor, "focus": focus})).unwrap();
}

struct NativeClickFixture {
    app: DesignApp,
    ctx: egui::Context,
    target: Value,
    old: Value,
    at: Pos2,
    focus: usize,
}

/// Build the old and target stories, measure an interior target caret, and leave the old story selected before the click.
fn native_gui_click_fixture() -> NativeClickFixture {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let old = typing_app(&mut app, &ctx, "Old story");
    let frame = app.run("frame.create", json!({"rect": [36, 280, 300, 400], "content": "text", "text": "Target story"})).unwrap();
    let target = frame["story"].clone();
    // Measure an interior first-word caret through the engine, rather than guessing a
    // font-dependent screen pixel or clicking the frame's resize border.
    let (point, focus) = (50..180)
        .find_map(|x| {
            app.run("text.placeCaret", json!({"frame": frame["id"], "point": [x, 290]})).unwrap();
            let sel = app.session.doc().unwrap().selection.text.unwrap();
            assert_eq!(sel.story.0, target.as_u64().unwrap());
            (1..5).contains(&sel.focus).then_some((Point::new(f64::from(x), 290.0), sel.focus))
        })
        .expect("an interior point in the target's first word");
    text_selection(&mut app, &old, "Old story".len(), "Old story".len());
    for _ in 0..4 {
        ime_frame(&mut app, &ctx, vec![]);
    }
    let at = on_screen(&app, point);
    assert!(app.canvas_rect.unwrap().contains(at));
    ime_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
    assert_eq!(app.session.doc().unwrap().selection.text.unwrap().story.0, old.as_u64().unwrap());
    NativeClickFixture { app, ctx, target, old, at, focus }
}

/// Build a primary pointer press or release event at a position.
fn native_gui_button(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
}

/// The 37-byte Devanagari line the native run had in the previous story.
const NATIVE_GUI_OLD_DEVANAGARI: &str = "नमस्ते दुनिया";

/// The click fixture with the non-empty selection in the previous story that the native
/// Windows run had (37-byte Devanagari, 0..37). A non-empty selection shows the contextual
/// task bar; keep it present but clear of the target click.
fn native_gui_selected_old_click_fixture() -> NativeClickFixture {
    let mut f = native_gui_click_fixture();
    text_selection(&mut f.app, &f.old, 0, "Old story".len());
    f.app.run("text.insert", json!({"text": NATIVE_GUI_OLD_DEVANAGARI})).unwrap();
    text_selection(&mut f.app, &f.old, 0, NATIVE_GUI_OLD_DEVANAGARI.len());
    for _ in 0..4 {
        ime_frame(&mut f.app, &f.ctx, vec![]);
    }
    let sel = f.app.session.doc().unwrap().selection.text.unwrap();
    assert_eq!((sel.story.0, sel.anchor, sel.focus), (f.old.as_u64().unwrap(), 0, NATIVE_GUI_OLD_DEVANAGARI.len()));
    assert_eq!(NATIVE_GUI_OLD_DEVANAGARI.len(), 37, "the native fixture's Devanagari line was 37 UTF-8 bytes");
    assert!(!sel.is_caret(), "the previous story's selection is non-empty");
    assert!(!context_menu_open(&f.ctx), "no context menu owns this frame's keys");
    assert!(f.app.ui.dialog.is_none() && f.app.ui.palette.is_none(), "no modal owns the keyboard");
    assert!(!f.ctx.text_edit_focused(), "no text field owns the keyboard");
    assert_eq!(f.app.pane, f.app.focus_pane, "the canvas pane takes the keyboard");
    let task_bar = egui::Id::unique(("task_bar", f.ctx.viewport_id(), f.app.pane));
    let bar = f.ctx.memory(|m| m.area_rect(task_bar)).expect("the old selection shows the floating task bar");
    assert!(!bar.contains(f.at), "the floating task bar does not cover the target click at {bar:?}");
    f
}

/// Run one raw event batch, optionally force a discarded pass, and return the pointer edges observed by each pass.
fn native_gui_input(f: &mut NativeClickFixture, events: Vec<egui::Event>, discard: bool) -> Vec<(bool, bool, bool)> {
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
    let mut edges = Vec::new();
    let mut out = f.ctx.run_ui(raw, |ui| {
        edges.push(ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.press_origin().is_some())));
        f.app.logic(ui.ctx());
        f.app.ui(ui);
        if discard && edges.len() == 1 {
            ui.ctx().request_discard("native same-batch click exactly-once regression");
        }
    });
    if discard {
        assert_eq!(out.platform_output.num_completed_passes, 2, "the requested discarded pass actually ran");
    }
    out.textures_delta.clear();
    edges
}

/// Assert that the primary button, canvas press state and tool gesture are all released.
fn native_gui_released(f: &NativeClickFixture) {
    assert!(!f.ctx.input(|i| i.pointer.primary_down()), "the native primary button is up");
    let down = f.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(("canvas_pointer_down", f.app.pane)))).unwrap_or(false);
    assert!(!down, "the canvas retained a released pointer");
    assert!(!f.app.session.tool_busy(), "a released click must not leave a tool gesture");
}

/// Create an independently measured second-story caret farther apart than the double-click distance.
fn native_gui_second_click_target(f: &mut NativeClickFixture) -> (Value, Pos2, usize) {
    let frame = f.app.run("frame.create", json!({"rect": [330, 280, 575, 400], "content": "text", "text": "Second story"})).unwrap();
    let story = frame["story"].clone();
    let (point, focus) = (345..465)
        .find_map(|x| {
            f.app.run("text.placeCaret", json!({"frame": frame["id"], "point": [x, 290]})).unwrap();
            let sel = f.app.session.doc().unwrap().selection.text.unwrap();
            assert_eq!(sel.story.0, story.as_u64().unwrap());
            (1..5).contains(&sel.focus).then_some((Point::new(f64::from(x), 290.0), sel.focus))
        })
        .expect("an independently measured interior caret in the second story");
    text_selection(&mut f.app, &f.old, "Old story".len(), "Old story".len());
    for _ in 0..4 {
        ime_frame(&mut f.app, &f.ctx, vec![]);
    }
    let at = on_screen(&f.app, point);
    assert!(f.app.canvas_rect.unwrap().contains(f.at));
    assert!(f.app.canvas_rect.unwrap().contains(at));
    let max_click_dist = f.ctx.options(|o| o.input_options.max_click_dist);
    assert!((at - f.at).length() > max_click_dist, "the two target presses must not form a double click");
    (story, at, focus)
}

/// Two completed clicks in one batch use the last press once, emit one caret/release pair, add no history, and type at the final caret.
#[test]
fn native_gui_two_completed_clicks_use_last_primary_press_once() {
    for reverse in [false, true] {
        for discard in [false, true] {
            let mut f = native_gui_click_fixture();
            let (second, second_at, second_focus) = native_gui_second_click_target(&mut f);
            let (first_at, last_at, final_story, final_focus) =
                if reverse { (second_at, f.at, f.target.clone(), f.focus) } else { (f.at, second_at, second.clone(), second_focus) };
            let initial = f.app.session.doc().unwrap().selection.text.unwrap();
            assert_eq!((initial.story.0, initial.anchor, initial.focus), (f.old.as_u64().unwrap(), "Old story".len(), "Old story".len()));
            let undo = f.app.session.doc().unwrap().history.undo.len();
            let journal = f.app.session.journal.len();
            let edges = native_gui_input(
                &mut f,
                vec![
                    egui::Event::PointerMoved(first_at),
                    native_gui_button(first_at, true),
                    native_gui_button(first_at, false),
                    egui::Event::PointerMoved(last_at),
                    native_gui_button(last_at, true),
                    native_gui_button(last_at, false),
                ],
                discard,
            );
            assert_eq!(edges[0], (true, true, false), "both completed clicks leave no held origin");
            assert_eq!(edges.iter().filter(|e| e.0).count(), 1, "discard must not replay either press");
            let sel = f.app.session.doc().unwrap().selection.text.unwrap();
            assert_eq!(
                (sel.story.0, sel.anchor, sel.focus),
                (final_story.as_u64().unwrap(), final_focus, final_focus),
                "reverse={reverse}, discard={discard}"
            );
            let commands = &f.app.session.journal[journal..];
            assert_eq!(
                commands.iter().filter(|(id, _)| id == "text.placeCaret").count(),
                1,
                "one aggregate canvas selection, reverse={reverse}, discard={discard}"
            );
            assert_eq!(
                commands.iter().filter(|(id, _)| id == "text.release").count(),
                1,
                "one corresponding release, reverse={reverse}, discard={discard}"
            );
            let clicks = f.ctx.data(|d| d.get_temp::<Clicks>(egui::Id::unique(("canvas_clicks", f.app.pane)))).expect("one recovered canvas press");
            assert_eq!(clicks.n, 1, "separated clicks and discarded passes must not select a word");
            assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo, "pointer selection adds no document history");
            native_gui_released(&f);
            native_gui_input(&mut f, vec![egui::Event::Text("!".into())], false);
            let mut target_text = "Target story".to_owned();
            let mut second_text = "Second story".to_owned();
            if reverse {
                target_text.insert(final_focus, '!');
            } else {
                second_text.insert(final_focus, '!');
            }
            assert_eq!(story_text(&mut f.app, &f.target), target_text);
            assert_eq!(story_text(&mut f.app, &second), second_text);
            assert_eq!(story_text(&mut f.app, &f.old), "Old story");
            assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo + 1, "typing at the final caret is one edit");
            native_gui_released(&f);
        }
    }
}

/// A non-finite final press rejects the click instead of reviving an earlier valid press; later motion cannot drag.
#[test]
fn native_gui_nonfinite_final_press_cannot_reuse_an_earlier_click() {
    for discard in [false, true] {
        let mut f = native_gui_click_fixture();
        let at = f.at;
        let initial = f.app.session.doc().unwrap().selection.text;
        let undo = f.app.session.doc().unwrap().history.undo.len();
        let journal = f.app.session.journal.len();
        let edges = native_gui_input(
            &mut f,
            vec![
                egui::Event::PointerMoved(at),
                native_gui_button(at, true),
                native_gui_button(at, false),
                native_gui_button(pos2(f32::NAN, at.y), true),
                egui::Event::PointerMoved(at),
                native_gui_button(at, false),
            ],
            discard,
        );
        assert_eq!(edges[0], (true, true, false));
        assert_eq!(edges.iter().filter(|e| e.0).count(), 1, "discard must not replay malformed input");
        assert_eq!(
            f.app.session.doc().unwrap().selection.text,
            initial,
            "discard={discard}: reject the last press instead of reviving an earlier valid one"
        );
        assert!(f.app.session.journal[journal..].iter().all(|(id, _)| id != "text.placeCaret" && id != "text.release"));
        assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo);
        assert_eq!(story_text(&mut f.app, &f.target), "Target story");
        assert_eq!(story_text(&mut f.app, &f.old), "Old story");
        native_gui_released(&f);
        native_gui_input(&mut f, vec![egui::Event::PointerMoved(at + vec2(90.0, 0.0))], false);
        assert_eq!(f.app.session.doc().unwrap().selection.text, initial, "motion after the invalid released press cannot drag");
        native_gui_released(&f);
    }
}

/// A same-batch click switches story exactly once; typing, undo and redo affect only the target story, including discarded-pass runs.
#[test]
fn native_gui_same_batch_click_switches_story_once_before_typing() {
    for discard in [false, true] {
        let mut f = native_gui_click_fixture();
        let undo = f.app.session.doc().unwrap().history.undo.len();
        let at = f.at;
        let edges = native_gui_input(&mut f, vec![egui::Event::PointerMoved(at), native_gui_button(at, true), native_gui_button(at, false)], discard);
        assert_eq!(edges[0], (true, true, false), "the real input consumed both edges and cleared the origin");
        assert_eq!(edges.iter().filter(|e| e.0).count(), 1, "a discarded pass must not replay the press");
        let selection = f.app.session.doc().unwrap().selection.text.unwrap();
        assert_eq!((selection.story.0, selection.anchor, selection.focus), (f.target.as_u64().unwrap(), f.focus, f.focus), "discard={discard}");
        let clicks = f.ctx.data(|d| d.get_temp::<Clicks>(egui::Id::unique(("canvas_clicks", f.app.pane)))).expect("one actual canvas click");
        assert_eq!(clicks.n, 1, "discard={discard}: replay must not turn a caret into word selection");
        native_gui_released(&f);
        assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo, "a click is not a document edit");
        let end = |pressed| egui::Event::Key { key: egui::Key::End, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
        native_gui_input(&mut f, vec![end(true), end(false)], false);
        let caret = f.app.session.doc().unwrap().selection.text.unwrap();
        assert_eq!((caret.story.0, caret.focus), (f.target.as_u64().unwrap(), "Target story".len()));
        native_gui_input(&mut f, vec![egui::Event::Text("!".into())], false);
        assert_eq!(story_text(&mut f.app, &f.target), "Target story!");
        assert_eq!(story_text(&mut f.app, &f.old), "Old story");
        assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo + 1, "one typed edit, including discarded-pass control");
        f.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(story_text(&mut f.app, &f.target), "Target story");
        assert_eq!(story_text(&mut f.app, &f.old), "Old story");
        f.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(story_text(&mut f.app, &f.target), "Target story!");
        assert_eq!(story_text(&mut f.app, &f.old), "Old story");
        native_gui_released(&f);
    }
}

/// The native Windows failure: with a non-empty selection in another story, a click that
/// switches stories followed by End in the same job left the new story's caret at the click
/// point. End in a later job worked. Drive the actual app UI and actual pointer/key events in
/// one batch, in split batches, and with End while the press is still held; discard a pass to
/// cover replay. The End press must reach the type tool once and win over the click, wherever
/// the pointer release falls, and typing must go there once.
#[test]
fn native_gui_click_then_end_in_one_batch_keeps_the_target_caret_at_the_story_end() {
    let names = ["same batch, click events first", "click batch then End batch", "End while the press is held"];
    for (layout, name) in names.into_iter().enumerate() {
        for discard in [false, true] {
            let mut f = native_gui_selected_old_click_fixture();
            let (at, target, old) = (f.at, f.target.clone(), f.old.clone());
            let undo = f.app.session.doc().unwrap().history.undo.len();
            let journal = f.app.session.journal.len();
            let moved = || egui::Event::PointerMoved(at);
            let down = || native_gui_button(at, true);
            let up = || native_gui_button(at, false);
            let end = |pressed| egui::Event::Key { key: egui::Key::End, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
            let batches: Vec<Vec<egui::Event>> = match layout {
                0 => vec![vec![moved(), down(), up(), end(true), end(false)]],
                1 => vec![vec![moved(), down(), up()], vec![end(true), end(false)]],
                _ => vec![vec![moved(), down()], vec![end(true)], vec![up(), end(false)]],
            };
            for events in batches {
                native_gui_input(&mut f, events, discard);
            }
            let selection = f.app.session.doc().unwrap().selection.text.unwrap();
            assert_eq!(
                (selection.story.0, selection.anchor, selection.focus),
                (target.as_u64().unwrap(), "Target story".len(), "Target story".len()),
                "{name}, discard={discard}: End must win after the click, wherever the release falls"
            );
            let commands = &f.app.session.journal[journal..];
            assert_eq!(
                commands.iter().filter(|(id, _)| id == "text.move").count(),
                1,
                "{name}, discard={discard}: the End press reached the type tool once"
            );
            let clicks = f.ctx.data(|d| d.get_temp::<Clicks>(egui::Id::unique(("canvas_clicks", f.app.pane)))).expect("one actual canvas click");
            assert_eq!(clicks.n, 1, "{name}, discard={discard}: a discarded pass must not replay the press as a word selection");
            native_gui_released(&f);
            native_gui_input(&mut f, vec![egui::Event::Text("!".into())], discard);
            assert_eq!(story_text(&mut f.app, &target), "Target story!", "{name}, discard={discard}: typing lands at the End caret once");
            assert_eq!(story_text(&mut f.app, &old), NATIVE_GUI_OLD_DEVANAGARI);
            assert_eq!(
                f.app.session.journal[journal..].iter().filter(|(id, _)| id == "text.insert").count(),
                1,
                "{name}, discard={discard}: the text event is not replayed"
            );
            assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo + 1, "{name}, discard={discard}: one edit");
        }
    }
}

/// Pointer ownership survives a split press/move/release; a real drag selects, release ends it, and later motion cannot continue it.
#[test]
fn native_gui_split_click_and_drag_release_keep_pointer_ownership() {
    for drag in [false, true] {
        let mut f = native_gui_click_fixture();
        let at = f.at;
        native_gui_input(&mut f, vec![native_gui_button(at, true)], false);
        let sel = f.app.session.doc().unwrap().selection.text.unwrap();
        assert_eq!((sel.story.0, sel.focus), (f.target.as_u64().unwrap(), f.focus));
        assert!(f.ctx.input(|i| i.pointer.primary_down()));
        assert!(f.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(("canvas_pointer_down", f.app.pane)))).unwrap());
        if drag {
            let moved = at + vec2(60.0, 0.0);
            native_gui_input(&mut f, vec![egui::Event::PointerMoved(moved)], false);
            let sel = f.app.session.doc().unwrap().selection.text.unwrap();
            assert_eq!(sel.story.0, f.target.as_u64().unwrap());
            assert!(!sel.is_caret(), "a real held drag selects text");
        }
        let release = if drag { pos2(-20.0, at.y) } else { at };
        native_gui_input(&mut f, vec![egui::Event::PointerMoved(release), native_gui_button(release, false)], false);
        native_gui_released(&f);
        let selection = f.app.session.doc().unwrap().selection.text;
        native_gui_input(&mut f, vec![egui::Event::PointerMoved(at + vec2(90.0, 0.0))], false);
        assert_eq!(f.app.session.doc().unwrap().selection.text, selection, "motion after release cannot continue the old drag");
        assert_eq!(story_text(&mut f.app, &f.target), "Target story");
        assert_eq!(story_text(&mut f.app, &f.old), "Old story");
    }
}

/// An unowned outside press, and a press that dismisses an open menu, cannot move the selection or edit either story.
#[test]
fn native_gui_outside_press_and_open_menu_do_not_click_through() {
    for menu in [false, true] {
        let mut f = native_gui_click_fixture();
        let at = f.at;
        let selection = f.app.session.doc().unwrap().selection.text;
        if menu {
            let blank = f.app.canvas_rect.unwrap().max - vec2(30.0, 30.0);
            for pressed in [true, false] {
                let event = egui::Event::PointerButton { pos: blank, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() };
                native_gui_input(&mut f, vec![egui::Event::PointerMoved(blank), event], false);
            }
            assert!(context_menu_open(&f.ctx), "the actual secondary click opened the canvas menu");
            assert_eq!(f.app.session.doc().unwrap().selection.text, selection);
            native_gui_input(&mut f, vec![egui::Event::PointerMoved(at), native_gui_button(at, true), native_gui_button(at, false)], false);
        } else {
            let outside = pos2(-20.0, -20.0);
            assert!(!f.app.canvas_rect.unwrap().contains(outside));
            native_gui_input(
                &mut f,
                vec![
                    egui::Event::PointerMoved(outside),
                    native_gui_button(outside, true),
                    egui::Event::PointerMoved(at),
                    native_gui_button(at, false),
                ],
                false,
            );
        }
        assert_eq!(f.app.session.doc().unwrap().selection.text, selection, "menu={menu}: an unowned press cannot move the old caret");
        assert_eq!(story_text(&mut f.app, &f.target), "Target story");
        assert_eq!(story_text(&mut f.app, &f.old), "Old story");
        native_gui_released(&f);
    }
}

/// Font inventory distinguishes exact bundled faces, missing styles and missing families without accepting installed or injected substitutes.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_gui_font_inventory_keeps_exact_regular_and_missing_style_diagnostics() {
    if designcraft_fonts::CRAFT_FONTS.is_empty() {
        assert!(std::env::var_os("CRAFT_FONTS_REQUIRED").is_none(), "required canonical fonts were not embedded");
        return; // The supported optional-input build has no central document fonts.
    }
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    for (family, text) in [("Noto Sans Hebrew", "שלום"), ("Noto Sans Devanagari", "नमस्ते")] {
        assert!(designcraft_fonts::CRAFT_FONTS.iter().any(|f| f.family == family && f.style == "Regular"), "canonical {family} Regular absent");
        for style in ["Regular", "Unavailable Native GUI Style"] {
            let frame = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": text})).unwrap();
            app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": text.len()})).unwrap();
            app.run("type.char", json!({"fontFamily": family, "fontStyle": style})).unwrap();
        }
    }
    let frame = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": "Missing"})).unwrap();
    app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 7})).unwrap();
    app.run("type.char", json!({"fontFamily": "Unavailable Native GUI Family", "fontStyle": "Regular"})).unwrap();
    let inventory = app.run("font.list", json!({})).unwrap();
    let rows = inventory.as_array().unwrap();
    assert_eq!(rows.len(), 5, "{inventory}");
    for family in ["Noto Sans Hebrew", "Noto Sans Devanagari"] {
        for style in ["Regular", "Unavailable Native GUI Style"] {
            let row = rows.iter().find(|r| r["family"] == family && r["style"] == style).unwrap();
            assert_eq!(row["missing"], false, "{row}");
            assert_eq!(row["styleMissing"], style != "Regular", "{row}");
            assert_eq!(row["matchStatus"], if style == "Regular" { "exact" } else { "styleSubstitute" }, "{row}");
            assert_eq!(row["resolvedFamily"], family, "{row}");
            assert_eq!(row["resolvedStyle"], "Regular", "{row}");
            assert_eq!(row["source"], "bundled", "an installed or injected face cannot satisfy this production contract: {row}");
        }
    }
    let missing = rows.iter().find(|r| r["family"] == "Unavailable Native GUI Family").unwrap();
    assert_eq!(missing["missing"], true, "{missing}");
    assert_eq!(missing["matchStatus"], "missing", "{missing}");
    assert!(missing["source"].is_null(), "{missing}");
}

#[test]
#[allow(deprecated)] // `ImeEvent::Enabled` / `Disabled`: egui no longer sends them, other integrations may.
fn ime_events_compose_in_the_story_with_the_candidate_window_at_the_caret() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let sid = typing_app(&mut app, &ctx, "曲:");
    // At a text caret the IME is allowed, its window on the caret as the canvas draws it.
    let ime = ime_frame(&mut app, &ctx, vec![]).expect("IME allowed at a text caret");
    let (top, bottom) = {
        let st = app.session.active().unwrap();
        let cs = app.session.cache.get(&st.doc, designcraft_doc::StoryId(sid.as_u64().unwrap()), None);
        let (fi, x, bl, asc, desc) = compose::caret(&cs, "曲:".len()).unwrap();
        let fid = cs.frames[fi].frame;
        let (a, _) = item_canvas_xf(&st.doc, &CanvasLayout::new(&st.doc, st.editing_parents), fid).unwrap();
        let m = a * st.doc.text_xf(st.doc.item(fid).unwrap());
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        (xf.to_screen(m * Point::new(x, bl - asc)), xf.to_screen(m * Point::new(x, bl + desc)))
    };
    assert!(ime.rect.expand(1.0).contains(top) && ime.rect.expand(1.0).contains(bottom), "{:?} vs {top:?}–{bottom:?}", ime.rect);
    assert_eq!(ime.cursor_rect, ime.rect);
    assert!(!ime.should_interrupt_composition);
    assert!(app.canvas_ime(), "the web build keeps whole words typed at the caret");
    // "ni" → に → 日, with the events winit sends.
    ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Enabled), preedit("ni", 2)]);
    assert_eq!(story_text(&mut app, &sid), "曲:ni", "marked text shows in place");
    assert!(app.session.tool_composing());
    // Keys, plain text, the clipboard and Undo (the native menu's ⌘Z) wait for the IME.
    let steps = app.session.doc().unwrap().history.undo.len();
    let key = |key, pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
    ime_frame(
        &mut app,
        &ctx,
        vec![key(egui::Key::Backspace, true), key(egui::Key::Backspace, false), egui::Event::Text("x".into()), egui::Event::Paste("y".into())],
    );
    crate::menus::activate(&mut app, "edit.undo", &Value::Null);
    assert!(!crate::menus::enabled(&app, "edit.undo"));
    assert_eq!(story_text(&mut app, &sid), "曲:ni");
    let ime2 = ime_frame(&mut app, &ctx, vec![preedit("に", 1)]).expect("IME allowed while composing");
    assert_eq!(story_text(&mut app, &sid), "曲:に");
    assert!(ime2.rect.center().x > ime.rect.center().x, "the window follows the IME's cursor: {:?} {:?}", ime2.rect, ime.rect);
    ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into())), egui::Event::Ime(egui::ImeEvent::Disabled)]);
    assert_eq!(story_text(&mut app, &sid), "曲:日");
    assert!(!app.session.tool_composing());
    assert_eq!(app.session.doc().unwrap().history.undo.len(), steps + 1, "the commit is one undo step");
    assert_eq!(app.session.journal.last(), Some(&("text.insert".to_string(), json!({"text": "日"}))));
    // After the commit, typing and Undo work again.
    ime_frame(&mut app, &ctx, vec![egui::Event::Text("!".into())]);
    assert_eq!(story_text(&mut app, &sid), "曲:日!");
    crate::menus::activate(&mut app, "edit.undo", &Value::Null);
    crate::menus::activate(&mut app, "edit.undo", &Value::Null);
    assert_eq!(story_text(&mut app, &sid), "曲:");
    // Without a text caret the IME is off: single-key tool shortcuts keep working.
    app.select_tool("selection");
    assert!(ime_frame(&mut app, &ctx, vec![]).is_none());
    assert!(!app.canvas_ime());
}

/// Both panes of a split window see the same keyboard events: only the pane last clicked
/// types them.
#[test]
fn a_split_window_types_once() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let sid = typing_app(&mut app, &ctx, "ab");
    app.run("window.split", json!({"on": true})).unwrap();
    for _ in 0..3 {
        ime_frame(&mut app, &ctx, vec![]);
    }
    assert!(app.other_pane.as_ref().and_then(|o| o.1).is_some(), "both panes are drawn");
    ime_frame(&mut app, &ctx, vec![egui::Event::Text("c".into())]);
    assert_eq!(story_text(&mut app, &sid), "abc");
    let key = |pressed| egui::Event::Key { key: egui::Key::Backspace, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
    ime_frame(&mut app, &ctx, vec![key(true), key(false)]);
    assert_eq!(story_text(&mut app, &sid), "ab");
    ime_frame(&mut app, &ctx, vec![egui::Event::Paste("de".into())]);
    assert_eq!(story_text(&mut app, &sid), "abde");
    ime_frame(&mut app, &ctx, vec![preedit("に", 1)]);
    assert_eq!(story_text(&mut app, &sid), "abdeに");
    ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into()))]);
    assert_eq!(story_text(&mut app, &sid), "abde日");
}

/// Surrounding text is committed text only, preserves reversed selection direction, and a commit keeps owner/generation while reporting `InputMethod`.
#[test]
fn ime_reports_the_committed_story_and_excludes_the_pre_edit() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let sid = typing_app(&mut app, &ctx, "a😀b");
    let len = "a😀b".len();
    text_selection(&mut app, &sid, 0, len);
    let ime = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
    assert_eq!(ime_payload(&ime), Some(("a😀b", len, 0)), "reversed selection keeps its direction");
    let owner = (ime.owner, ime.context_generation);
    // Marked text previews in the document but is not committed surrounding text.
    let ime = ime_frame(&mut app, &ctx, vec![preedit("に", 1)]).expect("a text caret");
    assert_eq!(ime_payload(&ime), Some(("a😀b", len, 0)));
    assert_eq!(ime.owner, owner.0);
    let ime = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into()))]).expect("a text caret");
    assert_eq!(ime_payload(&ime), Some(("日", 3, 3)));
    assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod);
    assert_eq!(story_text(&mut app, &sid), "日");
    assert_eq!(ime.owner, owner.0);
    assert_eq!(ime.context_generation, owner.1, "a commit is not a new context");
    let ime = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
    assert_eq!(ime_payload(&ime), Some(("日", 3, 3)), "a later pass reports the same committed text");
    assert_eq!((ime.owner, ime.context_generation), owner, "an unchanged pass keeps the context");
}

/// An empty story provides `("",0,0)`; long text is bounded and an out-of-window selection is explicitly unavailable.
#[test]
fn ime_empty_story_is_available_and_wide_text_is_bounded() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    typing_app(&mut app, &ctx, "");
    let ime = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
    assert_eq!(ime_payload(&ime), Some(("", 0, 0)), "an empty document is available, not unavailable");
    assert!(!app.session.tool_composing());

    let long = "a".repeat(5000);
    let sid = typing_app(&mut app, &ctx, &long);
    let ime = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
    let (text, _, _) = ime_payload(&ime).expect("a long document still has surrounding text");
    assert!(text.len() <= egui::output::MAX_IME_SURROUNDING_TEXT_BYTES, "{}", text.len());
    text_selection(&mut app, &sid, 0, 5000);
    let ime = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
    assert!(ime.surrounding_text.available().is_none(), "a selection beyond the bound is explicitly unavailable");
}

/// Caret movement preserves owner; changing story or document changes owner and generation.
#[test]
fn ime_owner_follows_the_story_and_the_document() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    typing_app(&mut app, &ctx, "ab");
    let first = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
    app.session.execute("text.move", &json!({"dir": "left"})).unwrap();
    let moved = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
    assert_eq!((moved.owner, moved.context_generation), (first.owner, first.context_generation), "a caret move keeps the context");

    let other = app.run("frame.create", json!({"rect": [400, 36, 200, 100], "content": "text", "text": "xy"})).unwrap();
    let other_sid = other["story"].clone();
    text_selection(&mut app, &other_sid, 2, 2);
    let second = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
    assert_ne!(second.owner, first.owner, "another story is another owner");
    assert_ne!(second.context_generation, first.context_generation);

    app.run("file.new", json!({})).unwrap();
    let doc = app.run("frame.create", json!({"rect": [36, 36, 200, 100], "content": "text", "text": "z"})).unwrap();
    text_selection(&mut app, &doc["story"], 1, 1);
    let replaced = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
    assert_ne!(replaced.owner, second.owner, "a new document is another owner");
    assert_eq!(ime_payload(&replaced), Some(("z", 1, 1)));
}

/// Pure commits report `InputMethod`; typing before or after a commit in one pass makes the cause `Other`.
#[test]
fn ime_cause_distinguishes_commits_from_application_edits() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    typing_app(&mut app, &ctx, "a");
    let committed = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into()))]).expect("a text caret");
    assert_eq!(committed.change_cause, egui::output::ImeChangeCause::InputMethod);
    let typed = ime_frame(&mut app, &ctx, vec![egui::Event::Text("!".into())]).expect("a text caret");
    assert_eq!(typed.change_cause, egui::output::ImeChangeCause::Other, "ordinary typing is an application edit");
    assert_eq!(ime_payload(&typed), Some(("a日!", 5, 5)));

    // An application edit before the commit in one pass breaks the receipt chain.
    let mut before = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    typing_app(&mut before, &ctx, "a");
    let mixed = ime_frame(&mut before, &ctx, vec![egui::Event::Text("x".into()), egui::Event::Ime(egui::ImeEvent::Commit("日".into()))])
        .expect("a text caret");
    assert_eq!(mixed.change_cause, egui::output::ImeChangeCause::Other);
    assert_eq!(ime_payload(&mixed), Some(("ax日", 5, 5)));

    // An application edit after the commit in one pass also dominates.
    let mut after = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    typing_app(&mut after, &ctx, "a");
    let mixed = ime_frame(&mut after, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into())), egui::Event::Text("!".into())])
        .expect("a text caret");
    assert_eq!(mixed.change_cause, egui::output::ImeChangeCause::Other);
    assert_eq!(ime_payload(&mixed), Some(("a日!", 5, 5)));
}

/// The settled multipass result rebuilds the current surrounding text while retaining the accepted commit cause.
#[test]
fn ime_cause_and_context_survive_a_discarded_pass() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    typing_app(&mut app, &ctx, "a");
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
        events: vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into()))],
        ..Default::default()
    };
    let mut passes = 0;
    let mut out = ctx.run_ui(raw, |ui| {
        passes += 1;
        if passes == 1 {
            ui.ctx().request_discard("IME multipass regression");
        }
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
    assert_eq!(out.platform_output.num_completed_passes, 2, "the frame really was multipass");
    let ime = out.platform_output.ime.expect("a text caret");
    assert_eq!(ime_payload(&ime), Some(("a日", 4, 4)), "the second pass rebuilds the current text");
    assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod, "the discarded pass keeps the accepted commit's cause");
}

/// Autocorrected commit text is an application edit with cause `Other`.
#[test]
fn ime_autocorrect_is_not_an_input_method_cause() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    app.session.prefs.autocorrect = true;
    let sid = typing_app(&mut app, &ctx, "teh");
    let ime = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit(" ".into()))]).expect("a text caret");
    assert_eq!(story_text(&mut app, &sid), "the ");
    assert_eq!(ime.change_cause, egui::output::ImeChangeCause::Other);
}

/// The selection stays selected and directed; byte partials leave whole characters; the deletion is one undo step.
#[test]
fn ime_delete_surrounding_excludes_the_selection_and_rounds_bytes_inward() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let sid = typing_app(&mut app, &ctx, "aé😀b");
    // Select é, which keeps its bytes and direction; 'a' before and 😀 after go.
    text_selection(&mut app, &sid, 1, 3);
    ime_frame(&mut app, &ctx, vec![]);
    let steps = app.session.doc().unwrap().history.undo.len();
    let ime = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 2, after_bytes: 4 })])
        .expect("a text caret");
    assert_eq!(story_text(&mut app, &sid), "éb");
    let sel = app.session.doc().unwrap().selection.text.unwrap();
    assert_eq!((sel.anchor, sel.focus), (0, 2), "the selected text stays selected and directed");
    assert_eq!(ime_payload(&ime), Some(("éb", 2, 0)));
    assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), steps + 1, "one deletion is one undo step");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(story_text(&mut app, &sid), "aé😀b");
    ime_frame(&mut app, &ctx, vec![]);
    // The same deletion with a reversed selection keeps anchor > focus.
    text_selection(&mut app, &sid, 3, 1);
    ime_frame(&mut app, &ctx, vec![]);
    let ime = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 2, after_bytes: 4 })])
        .expect("a text caret");
    assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod);
    let sel = app.session.doc().unwrap().selection.text.unwrap();
    assert_eq!(story_text(&mut app, &sid), "éb");
    assert_eq!((sel.anchor, sel.focus), (2, 0), "reversed selection stays reversed");

    // One byte of a two-byte character leaves the whole character.
    let partial = typing_app(&mut app, &ctx, "aé");
    text_selection(&mut app, &partial, 1, 1);
    ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 0, after_bytes: 1 })]);
    assert_eq!(story_text(&mut app, &partial), "aé");
    // A preceding count that lands inside the four-byte emoji leaves it whole too.
    let partial = typing_app(&mut app, &ctx, "a😀");
    ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 2, after_bytes: 0 })]);
    assert_eq!(story_text(&mut app, &partial), "a😀");
}

/// A 200-character request deletes the whole 100-character story.
#[test]
fn ime_delete_surrounding_has_no_sixty_four_character_cap() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let sid = typing_app(&mut app, &ctx, &"x".repeat(100));
    ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurrounding { before_chars: 200, after_chars: 0 })]);
    assert_eq!(story_text(&mut app, &sid), "");
}

/// Delete plus commit in one IME batch produces one undoable edit and reports the final surrounding text.
#[test]
fn ime_delete_and_commit_are_one_undo_step() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let sid = typing_app(&mut app, &ctx, "ab");
    text_selection(&mut app, &sid, 1, 2);
    ime_frame(&mut app, &ctx, vec![]);
    let steps = app.session.doc().unwrap().history.undo.len();
    let ime = ime_frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::Ime(egui::ImeEvent::DeleteSurrounding { before_chars: 1, after_chars: 0 }),
            egui::Event::Ime(egui::ImeEvent::Commit("X".into())),
        ],
    )
    .expect("a text caret");
    assert_eq!(story_text(&mut app, &sid), "X");
    assert_eq!(ime_payload(&ime), Some(("X", 1, 1)));
    assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), steps + 1, "the IME batch is one undo step");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(story_text(&mut app, &sid), "ab");
}

/// The input method's Commit for a composition a click ended earlier in the same event batch
/// must not type the marked text again at the clicked caret.
#[test]
fn a_commit_after_a_click_in_the_same_batch_is_not_typed_twice() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let sid = typing_app(&mut app, &ctx, "ab");
    ime_frame(&mut app, &ctx, vec![preedit("い", 1)]);
    assert_eq!(story_text(&mut app, &sid), "abい");
    let p = on_screen(&app, Point::new(150.0, 120.0));
    let press = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    ime_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p), press(true), egui::Event::Ime(egui::ImeEvent::Commit("い".into()))])
        .expect("still at a text caret");
    let typed = story_text(&mut app, &sid);
    assert_eq!(typed.matches('い').count(), 1, "the click typed the marked text once: {typed:?}");
    assert!(typed.starts_with("ab"), "{typed:?}");
    assert!(!app.session.tool_composing());
    assert!(app.take_ime_discard(), "the system IME is told to drop its copy");
    ime_frame(&mut app, &ctx, vec![press(false)]);
}

/// A delete-surrounding and its commit from the same ended composition must not edit the
/// clicked caret's text either.
#[test]
fn a_delete_and_commit_after_a_click_in_the_same_batch_are_dropped() {
    for delete in [
        egui::ImeEvent::DeleteSurrounding { before_chars: 1, after_chars: 0 },
        egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 1, after_bytes: 0 },
    ] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let sid = typing_app(&mut app, &ctx, "ab");
        ime_frame(&mut app, &ctx, vec![preedit("い", 1)]);
        let p = on_screen(&app, Point::new(150.0, 120.0));
        let press = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        ime_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(p), press(true), egui::Event::Ime(delete), egui::Event::Ime(egui::ImeEvent::Commit("い".into()))],
        )
        .expect("still at a text caret");
        let typed = story_text(&mut app, &sid);
        assert_eq!(typed.matches('い').count(), 1, "the late commit is dropped: {typed:?}");
        assert!(typed.contains("ab"), "the click's text was not deleted: {typed:?}");
        assert!(app.take_ime_discard());
        ime_frame(&mut app, &ctx, vec![press(false)]);
    }
}

/// A byte-count delete that reaches past the bounded IME window is still exactly the requested
/// input method mutation: the engine reports the ranges from the whole story.
#[test]
fn a_delete_beyond_the_surrounding_window_is_still_an_input_method_cause() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let sid = typing_app(&mut app, &ctx, &"x".repeat(5000));
    let ime = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 4500, after_bytes: 0 })])
        .expect("a text caret");
    assert_eq!(story_text(&mut app, &sid).len(), 500);
    assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod);
    assert_eq!(ime_payload(&ime).map(|(text, cursor, anchor)| (text.len(), cursor, anchor)), Some((500, 500, 500)));
}

/// The pass's IME observation comes after the later UI sections: a click into the Story
/// Editor's field takes the keyboard from the canvas in the same pass, so the composition
/// ends there and the canvas reports no IME.
#[test]
fn a_later_field_taking_the_keyboard_ends_the_composition_in_that_pass() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let f = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "ab"})).unwrap();
    let sid = f["story"].clone();
    app.select_tool("type");
    app.run("text.select", json!({"story": sid, "anchor": 2, "focus": 2})).unwrap();
    app.run("app.storyEditor", json!({"story": sid})).unwrap();
    let mut h = crate::test_window::open(app, egui::vec2(1440.0, 900.0));
    h.event(preedit("い", 1));
    h.run_steps(2);
    assert!(h.state().app.session.tool_composing());
    let canvas_owner = h.output().platform_output.ime.as_ref().expect("the canvas owns the composition").owner;
    let field = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).next().expect("the Story Editor's text field");
    field.click();
    h.run_steps(2);
    assert!(!h.state().app.session.tool_composing(), "the composition ended where the field took the keyboard");
    assert!(h.state_mut().app.take_ime_discard());
    assert!(h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).next().unwrap().is_focused(), "the field has the keyboard");
    let ime = h.output().platform_output.ime.as_ref().expect("the focused Story Editor publishes its IME context");
    assert_ne!(ime.owner, canvas_owner, "the field owns the IME, not the canvas");
    let typed = story_text(&mut h.state_mut().app, &sid);
    assert!(typed.contains('い'), "the marked text stays as typed: {typed:?}");
    h.event(egui::Event::Ime(egui::ImeEvent::Commit("X".into())));
    h.run_steps(2);
    let edited = story_text(&mut h.state_mut().app, &sid);
    assert_eq!(edited.matches('X').count(), 1, "the field's next commit is applied once: {edited:?}");
    assert_eq!(edited.matches('い').count(), 1, "the old canvas composition is not replayed: {edited:?}");
}

#[test]
fn clicking_away_mid_composition_keeps_the_text_and_interrupts_the_ime() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let sid = typing_app(&mut app, &ctx, "");
    ime_frame(&mut app, &ctx, vec![preedit("しょうこ", 4)]);
    assert!(app.session.tool_composing());
    // A click in the frame (macOS and Windows send nothing for it): the marked text stays as
    // typed and the IME is told to drop its composition, once.
    let p = on_screen(&app, Point::new(150.0, 120.0));
    let press = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    let ime = ime_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p), press(true)]).expect("still at a text caret");
    assert!(ime.should_interrupt_composition);
    ime_frame(&mut app, &ctx, vec![press(false)]);
    assert!(!app.session.tool_composing());
    assert_eq!(story_text(&mut app, &sid), "しょうこ");
    assert!(app.take_ime_discard(), "the host tells the system IME to drop its marked text");
    assert!(!ime_frame(&mut app, &ctx, vec![]).expect("still at a text caret").should_interrupt_composition);
    assert!(!app.take_ime_discard());
    // A bare line break from the IME confirms a composition: the marked text, no new paragraph.
    ime_frame(&mut app, &ctx, vec![preedit("がく", 2)]);
    ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("\n".into()))]);
    assert!(!app.session.tool_composing());
    let typed = story_text(&mut app, &sid);
    assert!(typed.contains("がく") && typed.contains("しょうこ") && !typed.contains('\n'), "{typed:?}");
}

#[test]
fn baseline_grid_lines_are_the_ones_on_the_page() {
    // A start far above the page used to be walked line by line, without end.
    for start in [-1e9, -1e17] {
        let mut far = baseline_grid_lines(start, 12.0, 792.0);
        assert_eq!((far.next(), far.count()), (Some(8.0), 65));
    }
    let lines = |start, increment, height| baseline_grid_lines(start, increment, height).collect::<Vec<_>>();
    assert_eq!(lines(36.0, 12.0, 72.0), [36.0, 48.0, 60.0], "the bottom edge is not a line");
    assert_eq!(lines(-30.0, 12.0, 30.0), [6.0, 18.0], "a start above the page keeps its rhythm");
    assert_eq!(lines(f64::NAN, 12.0, 792.0), [0.0; 0]);
    assert_eq!(baseline_grid_lines(0.0, 12.0, f64::INFINITY).count(), MAX_BASELINES);
}

#[test]
fn enter_or_tab_in_a_panel_field_leave_the_selected_text_alone() {
    for key in [egui::Key::Enter, egui::Key::Tab] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let f = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Hello"})).unwrap();
        app.select_tool("type");
        app.run("text.select", json!({"story": f["story"], "anchor": 1, "focus": 4})).unwrap();
        assert!(app.session.wants_text());
        let mut h = harness(app);
        let size = h.get_all_by_role(egui::accesskit::Role::TextInput).find(|n| n.value().as_deref() == Some("12 pt")).unwrap();
        size.focus();
        h.run_steps(1);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run_steps(1);
        h.event(egui::Event::Text("18".into()));
        h.run_steps(1);
        h.key_press(key);
        h.run_steps(3);
        let text = h.state_mut().session.execute("story.get", &json!({"story": f["story"]})).unwrap()["text"].clone();
        assert_eq!(text, "Hello", "{key:?} confirms the field and leaves the story alone");
        let size = crate::panels::text_attrs(h.state_mut()).unwrap()["chars"]["size"].clone();
        assert_eq!(size, 18.0, "{key:?} applies the typed size");
    }
}

#[test]
fn trailing_spaces_show_dots_before_the_paragraph_mark() {
    let text = "ab  \ncd  ";
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let rect = DRect::new(0.0, 0.0, 300.0, 100.0);
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), rect, lid, text, Default::default()).unwrap();
    let cs = compose::compose_story(&d, sid, &compose::ComposeOptions::default());
    let lines: Vec<_> = cs.frames.iter().flat_map(|f| &f.lines).collect();
    assert_eq!(lines.len(), 2);
    for (l, end_mark) in lines.iter().zip(["¶", "#"]) {
        let marks = hidden_marks(text, l);
        let dots: Vec<f64> = marks.iter().filter(|m| m.1 == "·").map(|m| m.0).collect();
        assert_eq!(dots.len(), 2, "a dot for each trailing space: {marks:?}");
        let &(para_x, mark, _) = marks.last().unwrap();
        assert_eq!(mark, end_mark);
        // The dots sit past the last letter, and the mark comes after both spaces.
        let space = l.glyphs.iter().rev().find(|g| g.len > 0).unwrap();
        assert!(dots.iter().all(|&x| x > l.end_x && x < para_x), "{dots:?} then {mark} at {para_x}");
        assert!(para_x >= space.x + space.adv, "{mark} at {para_x} overlaps the last space");
    }
}
