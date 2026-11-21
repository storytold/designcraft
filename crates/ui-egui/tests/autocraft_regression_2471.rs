//! A press on a resize handle of the selected frame resizes it, even where a ruler guide runs
//! through that handle (#2471).

use designcraft_geom::Point;
use designcraft_tools::CanvasLayout;
use designcraft_ui_egui::DesignApp;
use designcraft_ui_egui::canvas::Xf;
use egui::{Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

const FRAME_ID_KEY: &str = "id";

fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 4.0).build_ui_state(
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
fn a_selected_frames_handle_wins_over_a_guide_through_it() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), designcraft_ui_egui::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("guide.add", json!({"orientation": "vertical", "position": 100})).unwrap();
    // The frame's left edge is on the guide.
    let id = app.run("frame.create", json!({"rect": [100, 100, 300, 200]})).unwrap()[FRAME_ID_KEY].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    app.session.set_tool("selection");
    let mut h = harness(app);

    let left = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds().x0;
    let guide = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.spreads[0].pages[0].guides[0].position;
    assert_eq!(left(&h), 100.0);

    // The left middle handle, on screen.
    let handle = {
        let app = h.state();
        let st = app.session.active().unwrap();
        let layout = CanvasLayout::new(&st.doc, st.editing_parents);
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        xf.to_screen(layout.slots[0].xf * Point::new(100.0, 150.0))
    };
    drag(&mut h, handle, handle + vec2(60.0, 0.0));

    assert_eq!(guide(&h), 100.0, "the guide stays where it was");
    assert!(left(&h) > 105.0, "the handle resized the frame: left edge at {}", left(&h));
    assert!(h.state().session.active().unwrap().selection.guides.is_empty(), "the guide was not selected");
}
