use super::*;

#[test]
fn uncovered_page_is_paper_instead_of_a_black_shadow_and_absent_pages_stay_pasteboard() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"width": 64, "height": 64, "pages": 1, "facingPages": true})).unwrap();
    let st = app.session.active().unwrap();
    let layout = CanvasLayout::new(&st.doc, false);
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(160.0, 100.0));
    let xf = Xf::new(screen, &View { origin: Point::new(-80.0, -18.0), zoom: 1.0, fitted: true, rotation: 0 });
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        paint_page_previews(&mut app.canvas.previews, &ui.painter_at(screen), &xf, &st.doc, &layout, screen);
    });
    output.textures_delta.clear();
    let pages: Vec<_> =
        layout.slots.iter().flat_map(|slot| st.doc.spread(slot.spread).unwrap().pages.iter().map(move |p| slot.xf * p.bounds().center())).collect();
    assert_eq!(pages.len(), 1);
    let point = xf.to_screen(pages[0]);
    let rects: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(rects.len(), 2, "only the actual page gets a shadow and paper");
    assert_eq!(rects.iter().rfind(|r| r.rect.contains(point)).unwrap().fill, Color32::WHITE);
    let absent = xf.to_screen(Point::new(pages[0].x - 64.0, pages[0].y));
    assert!(!rects.iter().any(|r| r.rect.contains(absent)), "the missing facing page must remain pasteboard");
}
