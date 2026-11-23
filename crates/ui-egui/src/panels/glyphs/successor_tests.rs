//! Successor, on the installed framework, of the native glyph grid width test: the same steps and checks with the app's framework flag
//! on, so the panel draws the framework glyph grid.

use super::*;

/// Successor of `the_grid_leaves_room_for_the_scroll_bar` on the installed framework: the framework glyph grid, in a panel of each width, stops short of the scroll bar and leaves no column out.
#[test]
fn the_framework_grid_leaves_room_for_the_scroll_bar() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    let ctx = crate::test_window::context();
    crate::theme::install_fonts(&ctx, "");
    crate::theme::apply(&ctx, &Tokens::for_brightness(crate::theme::Brightness::MediumDark));
    let scroll = ctx.global_style().spacing.scroll;
    let bar = scroll.bar_inner_margin + scroll.bar_width + scroll.bar_outer_margin;
    for width in [236.0, 256.0, 300.0, 333.0] {
        let mut panel = egui::Rect::NOTHING;
        let mut grid = None;
        for _ in 0..2 {
            // The app raises the texture limit for the grid (see `test_window`).
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 700.0))),
                max_texture_side: Some(8192),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                ui.allocate_ui(vec2(width, 650.0), |ui| {
                    ui.set_width(width);
                    panel = ui.max_rect();
                    show(&mut app, ui);
                });
            });
            out.textures_delta.clear();
            // The grid is the one textured mesh that isn't text.
            grid = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => Some(m.calc_bounds()),
                _ => None,
            });
        }
        let grid = grid.expect("the glyph grid is drawn");
        assert!(grid.right() <= panel.right() - bar + 0.5, "{width}: the grid {grid:?} runs under the scroll bar of {panel:?}");
        assert!(grid.right() > panel.right() - bar - CELL, "{width}: no column is left out ({grid:?})");
    }
}
