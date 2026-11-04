//! Modal windows (dialogs, About) that the user moves by dragging their title row.
//!
//! The modal stays anchored at the centre of the window and is shifted by an offset kept in
//! egui's temporary memory. A modal that was not shown in the previous pass, or whose window
//! changed size, starts centred. An undragged modal keeps egui's constraint to the window; a
//! dragged one may go partly off-screen, but its title row stays reachable.

use egui::{Align2, Context, CursorIcon, Id, Modal, Rect, Sense, Ui, Vec2, vec2};

/// How much of the title row's width stays inside the window.
const KEEP_VISIBLE: f32 = 100.0;

#[derive(Clone, Copy, Default)]
struct Placement {
    offset: Vec2,
    /// The pass the modal was last shown in: a gap means it was closed and opened again.
    pass: u64,
    /// The window content size the offset belongs to.
    content: Vec2,
    /// From the top of the modal to the bottom of its title row.
    title_height: f32,
}

/// A modal centred in the window and shifted by the user's drag.
///
/// `area` is the modal's area id. `instance` names what it shows (for example the dialog kind),
/// so a different dialog in the same host starts centred.
pub(crate) fn modal(ctx: &Context, area: Id, instance: Id) -> Modal {
    let pass = ctx.cumulative_pass_nr();
    let size = ctx.memory(|m| m.area_rect(area)).map(|r| r.size());
    let content = ctx.content_rect();
    let offset = ctx.data_mut(|d| {
        let p = d.get_temp_mut_or_default::<Placement>(instance);
        if p.pass.saturating_add(1) < pass || p.content != content.size() {
            *p = Placement { content: content.size(), ..Placement::default() };
        }
        p.pass = pass;
        if let Some(size) = size
            && p.offset != Vec2::ZERO
        {
            p.offset = clamp(p.offset, size, p.title_height, content);
        }
        p.offset
    });
    // Dragged: `clamp` keeps the title row reachable instead of egui keeping the whole modal inside.
    let constrained = offset == Vec2::ZERO;
    Modal::new(area).area(Modal::default_area(area).anchor(Align2::CENTER_CENTER, offset).constrain(constrained))
}

/// Makes `rect`, the modal's title row, drag the modal opened with [`modal`] for `instance`.
///
/// Widgets registered after this call sit on top of the drag area and keep their own input.
pub(crate) fn title_drag(ui: &Ui, instance: Id, rect: Rect) {
    let resp = ui.interact(rect, instance.with("title_drag"), Sense::drag());
    let delta = if resp.dragged() { resp.drag_delta() } else { Vec2::ZERO };
    let top = ui.ctx().memory(|m| m.area_rect(ui.layer_id().id)).map_or(rect.top(), |r| r.top());
    ui.ctx().data_mut(|d| {
        let p = d.get_temp_mut_or_default::<Placement>(instance);
        p.offset += delta;
        p.title_height = (rect.bottom() - top).max(0.0);
    });
    if resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
}

/// Limits `offset` so the title row of a modal of `size` stays inside `content` vertically and
/// [`KEEP_VISIBLE`] of its width stays inside horizontally. A modal taller than the window keeps
/// its top edge inside.
fn clamp(offset: Vec2, size: Vec2, title_height: f32, content: Rect) -> Vec2 {
    let centred = Align2::CENTER_CENTER.align_size_within_rect(size, content);
    let keep = KEEP_VISIBLE.min(size.x);
    let x_lo = content.left() + keep - size.x - centred.left();
    let x_hi = content.right() - keep - centred.left();
    let y_lo = content.top() - centred.top();
    let y_hi = content.bottom() - title_height.min(size.y) - centred.top();
    vec2(offset.x.min(x_hi).max(x_lo), offset.y.min(y_hi).max(y_lo))
}
