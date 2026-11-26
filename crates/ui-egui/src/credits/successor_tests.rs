//! Successors, on the framework Contributors table, of the three native credits table tests the wave 4 cleanup removed: every
//! column can be dragged at its right edge and stays there, a changed fit replaces the dragged widths, and scrolling sideways keeps
//! them. They draw `credits::contributors_framework` in the Table layout over an installed framework (`table_layout_kit`).

use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::kittest::Queryable as _;

use crate::dialogs::table_layout_kit as kit;

fn view(_app: &mut crate::DesignApp, ui: &mut egui::Ui) {
    let _ = super::contributors_framework(ui, "en");
}

/// The Contributors table open in a window of `width`, the Table layout chosen.
fn table(width: f32) -> egui_kittest::Harness<'static, crate::DesignApp> {
    let app = kit::app(1, false);
    let mut h = kit::panel_harness(app, vec2(width, 500.0), view);
    kit::click(&mut h, &[Role::Button], "Table", 0);
    kit::steps(&mut h, 4);
    h
}

/// The header cells of the table, left to right.
fn headers(h: &egui_kittest::Harness<'static, crate::DesignApp>) -> Vec<egui::Rect> {
    let mut cells: Vec<egui::Rect> = h.query_all_by_role(Role::ColumnHeader).map(|n| n.rect()).collect();
    cells.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    cells
}

/// Drags a column's right edge by `by`.
fn drag_edge(h: &mut egui_kittest::Harness<'static, crate::DesignApp>, cell: egui::Rect, by: f32) {
    let start = egui::pos2(cell.right() - 1.0, cell.center().y);
    kit::drag(h, start, start + vec2(by, 0.0));
}

/// Successor of `every_column_can_be_dragged_at_its_right_edge_and_stays_there`: dragging a column's right edge 30 points makes that
/// column 30 wider and the width stays after more frames. The framework table keeps the other fixed columns as they were and lets
/// its one fill column (Contributor, the first) take what is left, so it is 30 narrower when another column grows. (The drag is
/// outward: the table's number columns open at or near the least width the framework lets a column be dragged to, two control
/// heights, so a drag 30 points inward stops there.)
#[test]
fn every_framework_column_can_be_dragged_at_its_right_edge_and_stays_there() {
    let columns = headers(&table(1100.0)).len();
    assert!(columns >= 3, "the table has its columns: {columns}");
    for dragged in 0..columns {
        let mut h = table(1100.0);
        let opened = headers(&h);
        drag_edge(&mut h, opened[dragged], 30.0);
        kit::steps(&mut h, 4);
        let after = headers(&h);
        assert_eq!(after.len(), opened.len());
        let width = |cells: &[egui::Rect], at: usize| cells[at].width();
        assert!(
            (width(&after, dragged) - (width(&opened, dragged) + 30.0)).abs() < 2.0,
            "dragging column {dragged}: {:?} was {:?}",
            after[dragged],
            opened[dragged]
        );
        // The fixed columns stay; the fill column (the first) takes the room the drag made.
        for column in (1..columns).filter(|c| *c != dragged) {
            assert!(
                (width(&after, column) - width(&opened, column)).abs() < 2.0,
                "dragging column {dragged}: column {column} is {:?}, was {:?}",
                after[column],
                opened[column]
            );
        }
        if dragged != 0 {
            assert!((width(&after, 0) - (width(&opened, 0) - 30.0)).abs() < 2.0, "the fill column gives way: {:?} was {:?}", after[0], opened[0]);
        }
    }
}

/// Successor of `a_changed_fit_replaces_the_column_widths`: the framework table keeps a dragged width (it does not drop it when the
/// fit changes, as the native table did): in a narrower window the dragged column is as wide as the drag left it, and a changed
/// sort keeps it too, with every column still in the table.
#[test]
fn the_framework_table_keeps_a_dragged_width_in_a_narrower_window() {
    let mut h = table(1100.0);
    let opened = headers(&h);
    drag_edge(&mut h, opened[0], -30.0);
    kit::steps(&mut h, 3);
    let dragged = headers(&h)[0].width();
    assert!((dragged - (opened[0].width() - 30.0)).abs() < 2.0, "the drag took: {dragged} was {}", opened[0].width());
    kit::resize(&mut h, vec2(700.0, 500.0));
    let narrower = headers(&h);
    assert_eq!(narrower.len(), opened.len(), "every column is still in the table");
    assert!((narrower[0].width() - dragged).abs() < 2.0, "a narrower window keeps the dragged width: {narrower:?}");
    kit::click(&mut h, &[Role::Button], "Reverse the order", 0);
    let after = headers(&h);
    assert_eq!(after.len(), opened.len());
    assert!((after[0].width() - dragged).abs() < 2.0, "so does a changed sort: {after:?}");
}

/// Successor of `scrolling_sideways_keeps_dragged_widths`: a column dragged 100 points wider keeps its width while the table is
/// scrolled sideways.
#[test]
fn scrolling_the_framework_table_sideways_keeps_dragged_widths() {
    let mut h = table(700.0);
    let opened = headers(&h);
    drag_edge(&mut h, opened[0], 100.0);
    kit::steps(&mut h, 3);
    let dragged = headers(&h);
    assert!((dragged[0].width() - opened[0].width() - 100.0).abs() < 3.0, "the drag took: {dragged:?}");
    for delta in [-13.3, -27.77, 41.1, 0.6] {
        kit::event(&mut h, egui::Event::PointerMoved(dragged[0].center()));
        kit::event(
            &mut h,
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(delta, 0.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
                source: egui::MouseWheelSource::Unknown,
            },
        );
        kit::steps(&mut h, 3);
        let scrolled = headers(&h);
        assert!((scrolled[0].width() - dragged[0].width()).abs() < 2.0, "scrolled {delta}: {scrolled:?}");
    }
}
