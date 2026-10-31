//! The loaded place cursor's image count, drawn beside the thumbnail's badge.

use super::{Color32, DesignApp, Pos2, Rect, pos2, vec2};

/// Several images loaded: how many, beside the thumbnail's image `badge` ("(6)"), navy with a
/// white rim so it reads over any picture.
pub(super) fn draw_loaded_count(app: &DesignApp, painter: &egui::Painter, badge: Rect) {
    let loaded = app.session.loaded.is_some().then(|| app.session.loaded_queue.len().saturating_add(1));
    let Some(label) = loaded_count_label(loaded) else { return };
    let at: Pos2 = pos2(badge.max.x + 6.0, badge.center().y);
    let font = crate::theme::semibold(15.0);
    for d in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0)] {
        painter.text(at + d, egui::Align2::LEFT_CENTER, &label, font.clone(), Color32::WHITE);
    }
    painter.text(at, egui::Align2::LEFT_CENTER, label, font, Color32::from_rgb(25, 35, 100));
}

/// The count the loaded place cursor shows, as InDesign's "(6)": only when more than one image
/// is loaded.
pub(super) fn loaded_count_label(loaded: Option<usize>) -> Option<String> {
    loaded.filter(|n| *n > 1).map(|n| format!("({n})"))
}

#[cfg(test)]
mod tests {
    /// The loaded cursor counts the images as InDesign's "(6)", and shows no count for the last
    /// one or with nothing loaded.
    #[test]
    fn the_loaded_cursor_counts_several_images() {
        assert_eq!(super::loaded_count_label(Some(6)).as_deref(), Some("(6)"));
        assert_eq!(super::loaded_count_label(Some(1)), None);
        assert_eq!(super::loaded_count_label(None), None);
    }
}
