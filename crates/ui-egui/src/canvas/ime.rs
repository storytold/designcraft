use super::input::{keyboard_pane, modal_open};
use super::{CommittedText, DesignApp, IMESurroundingText, ImeMutation, ImeMutationStamp, ImeObservation, ImeTarget, ImeTextContext, Rect, Xf};

/// Has the composition these events belong to already ended outside the IME in this batch?
///
/// A pointer press and a document switch end the composition and type its marked text where the
/// caret then is, but the system IME still sends the Commit (or a delete) for the copy it holds.
/// Applying those at the new caret would type the text twice, put it in another story or document,
/// or delete text there, so they are dropped and the IME is told to drop its copy.
fn ime_composition_ended(app: &DesignApp) -> bool {
    app.ime_marked.is_some() && !app.session.tool_composing()
}

/// Did the composition move to another owner (another story, cell or document state) since it
/// began? A late commit then belongs to the old one, not the caret that is there now.
fn ime_composition_moved(app: &DesignApp) -> bool {
    let Some(recorded) = app.ime_target else { return false };
    app.session.ime_text_context().is_some_and(|context| ime_target(&context) != recorded)
}

/// Drop events for a composition that already ended or moved, keeping what was typed.
fn drop_stale_ime_events(app: &mut DesignApp) {
    app.ime_marked = None;
    app.ime_target = None;
    app.pending_ime_delete = None;
    app.ime_discard = true;
    // The host must interrupt the system IME's copy in this pass too, as for a click away.
    app.ime_interrupt = true;
}

/// One input method event for the text caret: marked text previews in the story, a commit types.
pub(super) fn ime_event(app: &mut DesignApp, ctx: &egui::Context, e: egui::ImeEvent, vi: designcraft_engine::ViewInfo) {
    let r = match e {
        egui::ImeEvent::Preedit { text, active_range_chars } => {
            if text.is_empty() {
                app.ime_marked = None;
                app.ime_target = None;
            } else {
                // The composition's owner at its first marked text: a commit may only land there.
                if app.ime_marked.is_none() {
                    app.ime_target = app.session.ime_text_context().map(|c| ime_target(&c));
                }
                app.ime_marked = Some(text.clone());
            }
            app.session.tool_preedit(&text, active_range_chars, vi).map_err(|e| e.to_string())
        }
        // A composition that already ended or moved in this batch owns nothing any more.
        egui::ImeEvent::Commit(_) | egui::ImeEvent::DeleteSurrounding { .. } | egui::ImeEvent::DeleteSurroundingBytes { .. }
            if app.ime_interrupt || ime_composition_ended(app) || ime_composition_moved(app) =>
        {
            // Dropping a stale delete clears the marked target. Keep the pass's interrupt latch
            // authoritative so the commit following that delete cannot become a fresh insertion.
            drop_stale_ime_events(app);
            Ok(())
        }
        // A bare line break confirms the composition (Enter with some IMEs): the marked text, not
        // a new paragraph.
        egui::ImeEvent::Commit(t) if t == "\n" || t == "\r" => {
            finish_pending_ime_delete(app, ctx);
            keep_marked_text(app);
            Ok(())
        }
        egui::ImeEvent::Commit(t) => ime_commit(app, ctx, &t, vi),
        // The web text agent's corrections and mobile Backspace: characters around the caret go.
        // These are held for the Commit in the same batch so both mutations are one undo step.
        egui::ImeEvent::DeleteSurrounding { before_chars, after_chars } => {
            app.pending_ime_delete = Some((before_chars, after_chars, false));
            Ok(())
        }
        egui::ImeEvent::DeleteSurroundingBytes { before_bytes, after_bytes } => {
            app.pending_ime_delete = Some((before_bytes, after_bytes, true));
            Ok(())
        }
        // `Enabled` and `Disabled` carry nothing: the IME is on while the observation sets `ime`.
        _ => Ok(()),
    };
    if let Err(e) = r {
        app.status(e);
    }
    app.after_engine();
}

/// The bounded committed snapshot and stable target just before an IME mutation.
struct ImeBefore {
    target: ImeTarget,
    stamp: ImeMutationStamp,
    snapshot: IMESurroundingText,
    document_uid: u64,
    document_epoch: u64,
    story: designcraft_doc::StoryId,
    cell: Option<designcraft_doc::CellAddr>,
    anchor: usize,
    focus: usize,
}

/// Build a stable owner from the document uid, story and cell identity, plus the document epoch.
fn ime_target(context: &ImeTextContext<'_>) -> ImeTarget {
    let owner = egui::Id::unique(("designcraft-ime", context.document_uid, context.story.0, context.cell.map(|c| (c.table, c.row, c.col))));
    ImeTarget::new(owner, context.document_epoch)
}

/// Capture the bounded committed surrounding text, mutation stamp, target and selection identity before an IME edit.
fn capture_ime_before(app: &DesignApp) -> Option<ImeBefore> {
    let context = app.session.ime_text_context()?;
    let snapshot = IMESurroundingText::from_text(context.text, context.focus, context.anchor, None);
    let stamp = ImeMutationStamp::from_surrounding_text(snapshot.clone());
    Some(ImeBefore {
        target: ime_target(&context),
        stamp,
        snapshot,
        document_uid: context.document_uid,
        document_epoch: context.document_epoch,
        story: context.story,
        cell: context.cell,
        anchor: context.anchor,
        focus: context.focus,
    })
}

/// Fold a pending delete-surrounding plus commit into one engine operation, clear marked/target state and record the change.
fn ime_commit(app: &mut DesignApp, ctx: &egui::Context, text: &str, vi: designcraft_engine::ViewInfo) -> Result<(), String> {
    let pending = app.pending_ime_delete.take();
    let before = capture_ime_before(app);
    let r = if let Some((before_bytes, after_bytes, bytes)) = pending {
        app.session.ime_commit_after_delete(text, vi, before_bytes, after_bytes, bytes)
    } else {
        app.session.ime_commit(text, vi)
    };
    app.ime_marked = None;
    app.ime_target = None;
    let r = r.map_err(|e| e.to_string());
    if let Ok(mutation) = &r {
        note_ime_change(app, ctx, before, mutation.as_ref(), pending);
    }
    r.map(|_| ())
}

/// Apply a pending IME delete unless its composition ended or moved; record the mutation and drop stale marked text.
pub(super) fn finish_pending_ime_delete(app: &mut DesignApp, ctx: &egui::Context) {
    let Some((before_bytes, after_bytes, bytes)) = app.pending_ime_delete.take() else { return };
    if ime_composition_ended(app) || ime_composition_moved(app) {
        // The composition ended earlier in this batch: its delete belongs to the copy the system
        // IME still holds, not to the caret that is there now.
        drop_stale_ime_events(app);
        return;
    }
    let before = capture_ime_before(app);
    match app.session.ime_delete_surrounding(before_bytes, after_bytes, bytes) {
        Ok(mutation) => {
            // The delete drops the composition (its marked text is not typed): the IME's copy goes.
            if !app.session.tool_composing() && app.ime_marked.take().is_some() {
                app.ime_discard = true;
            }
            if !app.session.tool_composing() {
                app.ime_target = None;
            }
            note_ime_change(app, ctx, before, mutation.as_ref(), Some((before_bytes, after_bytes, bytes)));
        }
        Err(e) => app.status(e.to_string()),
    }
    app.after_engine();
}

/// Record the origin of a successful IME mutation.
///
/// A receipt is earned only when the engine reported the exact mutation it applied and the bounded
/// committed snapshot agrees with it. Smart quotes, autocorrect, tracked changes and any other
/// application transformation therefore report `Other`, as do failed, stale and no-op mutations.
fn note_ime_change(
    app: &mut DesignApp,
    ctx: &egui::Context,
    before: Option<ImeBefore>,
    mutation: Option<&ImeMutation>,
    delete: Option<(usize, usize, bool)>,
) {
    let Some(before) = before else { return };
    let Some(after) = app.session.ime_text_context() else {
        let viewport = ctx.viewport_id();
        app.ime_tracker.note_application_change(viewport, ctx.cumulative_frame_nr_for(viewport), before.target);
        return;
    };
    if ime_target(&after) != before.target {
        return;
    }
    let after_snapshot = IMESurroundingText::from_text(after.text, after.focus, after.anchor, None);
    if &after_snapshot == before.stamp.surrounding_text() {
        return;
    }
    let after_stamp = ImeMutationStamp::from_surrounding_text(after_snapshot.clone());
    let viewport = ctx.viewport_id();
    let frame_nr = ctx.cumulative_frame_nr_for(viewport);
    if mutation.is_some_and(|m| ime_change_is_exact(&before, &after_snapshot, m, delete)) {
        app.ime_tracker.note_input_method_apply(viewport, frame_nr, before.target, before.stamp, after_stamp);
    } else {
        app.ime_tracker.note_application_change(viewport, frame_nr, before.target);
    }
}

/// Does the bounded post-mutation snapshot agree with the mutation the engine applied?
///
/// The expected result is built from the reported ranges and typed text — absolute byte offsets in
/// the whole story, not the truncated outgoing window — so a delete that reaches past the window
/// still matches. Every visible byte of the after window must agree with it, but a mutation the
/// after window does not overlap at all (a delete longer than the window) rests on the engine's
/// report alone, which is exact because the engine computed it from the whole story. A rewrite,
/// tracked changes or a replaced story is not a receipt.
fn ime_change_is_exact(before: &ImeBefore, after: &IMESurroundingText, m: &ImeMutation, delete: Option<(usize, usize, bool)>) -> bool {
    // The same target, selection and text the receipt was captured around.
    if m.document_uid != before.document_uid
        || m.document_epoch != before.document_epoch
        || m.story != before.story
        || m.cell != before.cell
        || m.anchor_before != before.anchor
        || m.focus_before != before.focus
    {
        return false;
    }
    let (Some(before_payload), Some(after_payload)) = (before.snapshot.available(), after.available()) else { return false };
    let before_text = before_payload.text.as_str();
    let after_text = after_payload.text.as_str();
    let selection = m.anchor_before.min(m.focus_before)..m.anchor_before.max(m.focus_before);
    // The reported deletion adjoins the selection, is inside the text, and exists only for a
    // delete-surrounding event. Its exact extent is the engine's: it computes the ranges from the
    // whole story with the requested counts.
    if m.deleted_before.end != selection.start
        || m.deleted_after.start != selection.end
        || m.deleted_before.start > selection.start
        || m.deleted_after.end > m.text_len_before
    {
        return false;
    }
    if delete.is_none() && (!m.deleted_before.is_empty() || !m.deleted_after.is_empty()) {
        return false;
    }
    // The story changed by exactly the removed bytes, the replaced selection and the typed text:
    // tracked changes and an inserted paragraph return (a table anchor) break this identity.
    let replaced = if m.inserted.is_empty() { 0 } else { selection.len() };
    let expected_len = m
        .text_len_before
        .saturating_sub(m.deleted_before.len())
        .saturating_sub(m.deleted_after.len())
        .saturating_sub(replaced)
        .saturating_add(m.inserted.len());
    if m.text_len_after != expected_len {
        return false;
    }
    // A commit collapses the caret after the typed text; a delete keeps the selection, shifted.
    let (want_anchor, want_focus) = if m.inserted.is_empty() {
        (m.anchor_before.saturating_sub(m.deleted_before.len()), m.focus_before.saturating_sub(m.deleted_before.len()))
    } else {
        let at = m.deleted_before.start + m.inserted.len();
        (at, at)
    };
    if m.anchor_after != want_anchor || m.focus_after != want_focus {
        return false;
    }
    // The text around the caret, rebuilt from the reported mutation. A snapshot's window starts
    // before its selection's start by the selection's own offset in that window.
    let before_win = selection.start.saturating_sub(before_payload.cursor.min(before_payload.anchor));
    let after_selection = m.anchor_after.min(m.focus_after)..m.anchor_after.max(m.focus_after);
    let after_win = after_selection.start.saturating_sub(after_payload.cursor.min(after_payload.anchor));
    if after_win.saturating_add(after_payload.cursor) != m.focus_after || after_win.saturating_add(after_payload.anchor) != m.anchor_after {
        return false;
    }
    let before_end = before_win.saturating_add(before_text.len());
    let after_end = after_win.saturating_add(after_text.len());
    if before_end > m.text_len_before || after_end > m.text_len_after {
        return false;
    }
    let kept = if m.inserted.is_empty() { selection.len() } else { 0 };
    let inserted_post = m.deleted_before.start + kept;
    let tail_post = inserted_post + m.inserted.len();
    let compared = (|| -> Option<usize> {
        let mut n = 0usize;
        n += ime_compare_kept(before_text, before_win..before_end, 0..m.deleted_before.start, 0, after_text, after_win..after_end)?;
        if kept > 0 {
            n += ime_compare_kept(before_text, before_win..before_end, selection.clone(), m.deleted_before.start, after_text, after_win..after_end)?;
        }
        n += ime_compare_window(after_text, after_win..after_end, &(inserted_post..tail_post), &m.inserted)?;
        n += ime_compare_kept(
            before_text,
            before_win..before_end,
            m.deleted_after.end..m.text_len_before,
            tail_post,
            after_text,
            after_win..after_end,
        )?;
        Some(n)
    })();
    compared.is_some()
}

/// Compare one unchanged piece of the story: the pre-mutation bytes `pre` (absolute) appear at
/// post-mutation offset `post_start`. Returns the number of bytes compared, or `None` on a
/// mismatch. Bytes outside the bounded before window cannot be checked and are skipped.
fn ime_compare_kept(
    before_text: &str,
    before_win: std::ops::Range<usize>,
    pre: std::ops::Range<usize>,
    post_start: usize,
    after_text: &str,
    after_win: std::ops::Range<usize>,
) -> Option<usize> {
    let from = pre.start.max(before_win.start);
    let to = pre.end.min(before_win.end);
    if from >= to {
        return Some(0);
    }
    let expected = before_text.get(from - before_win.start..to - before_win.start)?;
    let post = post_start + (from - pre.start)..post_start + (to - pre.start);
    ime_compare_window(after_text, after_win, &post, expected)
}

/// Compare `expected`, which begins at absolute post-mutation offset `post.start`, with what the
/// after window shows of it. Returns the number of bytes compared; `None` when they differ.
fn ime_compare_window(after_text: &str, after_win: std::ops::Range<usize>, post: &std::ops::Range<usize>, expected: &str) -> Option<usize> {
    let from = post.start.max(after_win.start);
    let to = post.end.min(after_win.end);
    if from >= to {
        return Some(0);
    }
    let actual = after_text.get(from - after_win.start..to - after_win.start)?;
    let skip = from - post.start;
    let want = expected.get(skip..skip + (to - from))?;
    (actual == want).then_some(to - from)
}

/// End a composition outside the IME (a click away, the canvas losing the keyboard): the marked
/// text stays as typed, and the system IME is told to drop its copy.
pub(crate) fn keep_marked_text(app: &mut DesignApp) {
    if app.ime_marked.take().is_some() {
        app.ime_discard = true;
    }
    app.ime_target = None;
    if app.session.tool_composing() {
        if let Err(e) = app.session.end_composition() {
            app.status(e.to_string());
        }
        app.after_engine();
    }
}

/// What the canvas found for this viewport pass, observed once at the end of the pass.
pub(crate) struct CanvasIme {
    /// The viewport it was found in: the second window's canvas runs inside the root's pass and
    /// must not consume the root's observation.
    viewport: egui::ViewportId,
    /// The caret's screen rectangle, when the canvas owns the text caret this pass.
    caret: Option<Rect>,
}

/// While the text caret is in the canvas, let the system IME compose (egui-winit allows it only in
/// frames that set `ime`; the web build focuses its hidden text input then) and keep the candidate
/// window at the caret, through zoom, scrolling, view rotation and vertical text.
///
/// The caret is only recorded here: the observation waits for the end of the viewport's pass, so
/// a command or a field that takes the keyboard in a later section of the UI is included.
pub(super) fn ime_canvas_pass(app: &mut DesignApp, ctx: &egui::Context, xf: &Xf) {
    if !keyboard_pane(app) {
        // Both panes see the same viewport and events. The focused pane owns this viewport's one
        // observation; observing for the other would reset the context every split-window frame.
        return;
    }
    // A text field, dialog or Quick Apply has the keyboard: egui's own text editing serves the IME.
    let ours = app.session.wants_text() && !ctx.text_edit_focused() && !modal_open(app);
    let caret = if ours { app.session.tool_ime_caret(app.view_info()) } else { None };
    let caret = caret.map(|(a, b)| Rect::from_two_pos(xf.to_screen(a), xf.to_screen(b)).expand(1.0));
    app.canvas_ime = caret.is_some();
    app.ime_canvas = Some(CanvasIme { viewport: ctx.viewport_id(), caret });
}

/// Observe the IME once for this viewport pass, after every command and focus change it applied.
pub(crate) fn ime_observe(app: &mut DesignApp, ctx: &egui::Context) {
    let viewport = ctx.viewport_id();
    if app.ime_canvas.as_ref().is_none_or(|canvas| canvas.viewport != viewport) {
        return;
    }
    let Some(canvas) = app.ime_canvas.take() else { return };
    // A text field, dialog or Quick Apply that took the keyboard later in the pass owns the IME.
    let ours = canvas.caret.is_some() && !ctx.text_edit_focused() && !modal_open(app);
    let Some(r) = canvas.caret.filter(|_| ours) else {
        // The IME goes away with its marked text: keep that text as typed, like a click away.
        keep_marked_text(app);
        app.ime_tracker.observe(viewport, None);
        return;
    };
    // The composition ended outside the IME (a click, a command): the IME drops what it has marked.
    let interrupt = app.ime_interrupt || (app.ime_marked.is_some() && !app.session.tool_composing());
    app.ime_interrupt = false;
    if interrupt {
        app.ime_marked = None;
        app.ime_target = None;
        app.ime_discard = true;
    }
    app.canvas_ime = true;
    // One observation per viewport pass, after this pass's events and commands. The committed
    // text excludes any active pre-edit, and the tracker copies only the bounded IME window.
    let update = match app.session.ime_text_context() {
        Some(context) => {
            let observation = ImeObservation::new(ctx.cumulative_frame_nr_for(viewport), ctx.cumulative_pass_nr_for(viewport), ime_target(&context))
                .purpose(egui::IMEPurpose::Normal)
                .committed(Some(CommittedText::new(context.text, context.focus, context.anchor)));
            app.ime_tracker.observe(viewport, Some(observation))
        }
        None => app.ime_tracker.observe(viewport, None),
    };
    let Some(update) = update else { return };
    ctx.output_mut(|o| {
        o.ime = Some(egui::output::IMEOutput {
            owner: update.owner,
            context_generation: update.context_generation,
            purpose: update.purpose,
            rect: r,
            cursor_rect: r,
            should_interrupt_composition: interrupt,
            reset_request: None,
            change_cause: update.change_cause,
            surrounding_text: update.surrounding_text,
        });
    });
}
