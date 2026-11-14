//! The thread indicator between two linked text frames runs from the out port square of the
//! first frame to the in port square of the next (#2538).

use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, Services};
use egui::epaint::Shape;
use serde_json::json;

fn flatten<'a>(shape: &'a Shape, out: &mut Vec<&'a Shape>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
        other => out.push(other),
    }
}

/// Run a few frames of the whole window; the shapes of the last one.
fn paint(app: &mut DesignApp) -> Vec<Shape> {
    let ctx = egui::Context::default();
    let mut shapes = Vec::new();
    for _ in 0..3 {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
        shapes = out.shapes.into_iter().map(|c| c.shape).collect();
    }
    shapes
}

#[test]
fn thread_line_joins_the_port_squares() {
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.run("file.new", json!({})).unwrap();
    let mut ids = Vec::new();
    for rect in [[36, 36, 200, 300], [250, 36, 400, 300]] {
        let r = app.run("frame.create", json!({"rect": rect, "content": "text", "text": "hello", "caret": false})).unwrap();
        ids.push(r["id"].as_u64().unwrap());
    }
    app.run("text.thread", json!({"from": ids[0], "to": ids[1]})).unwrap();
    app.run("selection.set", json!({"ids": ids})).unwrap();
    app.ui.text_threads = true;

    let shapes = paint(&mut app);
    let mut flat = Vec::new();
    shapes.iter().for_each(|s| flatten(s, &mut flat));

    // The port squares: white, 8.5 px.
    let ports: Vec<egui::Pos2> = flat
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(r) if r.fill == egui::Color32::WHITE && (r.rect.width() - 8.5).abs() < 0.01 && (r.rect.height() - 8.5).abs() < 0.01 => {
                Some(r.rect.center())
            }
            _ => None,
        })
        .collect();
    assert!(ports.len() >= 4, "two linked frames show four ports, found {}", ports.len());

    let at_port = |p: egui::Pos2| ports.iter().any(|c| (c.x - p.x).abs() < 0.01 && (c.y - p.y).abs() < 0.01);
    let joined = flat.iter().any(|s| match s {
        Shape::LineSegment { points, .. } => at_port(points[0]) && at_port(points[1]),
        _ => false,
    });
    assert!(joined, "the thread line starts and ends at port squares; ports {ports:?}");
}
