//! One blink phase for the text caret: the story, table cell and footnote carets share it.

use designcraft_doc::TextSel;

const BLINK_HZ: f64 = 1.6;

fn start_id() -> egui::Id {
    egui::Id::unique("canvas_caret_blink_start")
}

fn caret_id() -> egui::Id {
    egui::Id::unique("canvas_caret_blink_caret")
}

pub(super) fn reset(ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    ctx.data_mut(|d| d.insert_temp(start_id(), now));
}

pub(super) fn visible(ctx: &egui::Context, caret: TextSel) -> bool {
    let now = ctx.input(|i| i.time);

    let start = ctx.data_mut(|d| match (d.get_temp::<f64>(start_id()), d.get_temp::<TextSel>(caret_id())) {
        (Some(start), Some(last)) if last == caret => start,
        _ => {
            d.insert_temp(start_id(), now);
            d.insert_temp(caret_id(), caret);
            now
        }
    });

    let phase = (now - start) * BLINK_HZ;
    let wait = until_flip(phase);

    if wait.is_finite() {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
    }
    phase as i64 % 2 == 0
}

pub(super) fn until_flip(phase: f64) -> f64 {
    (1.0 - phase.rem_euclid(1.0)) / BLINK_HZ
}
