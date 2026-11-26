#[cfg(not(target_arch = "wasm32"))]
use super::json;
use super::{DesignApp, canvas, chrome, new_document, now_ms, theme, updates_ui};

impl DesignApp {
    /// Install the UI fonts on the first frame, and again when the language or the system fonts change.
    fn update_fonts(&mut self, ctx: &egui::Context) {
        if !self.styled {
            // The UI scale is a preference (applied above); egui's own Cmd+Plus/Minus/0 would rescale the
            // whole window instead of the canvas, only to be reset next frame.
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
            self.fonts_system = theme::install_fonts(ctx, &self.ui.language);
            self.fonts_lang = self.ui.language.clone();
            self.styled = true;
            self.restyle = true;
        } else {
            if self.fonts_lang != self.ui.language || (!self.fonts_system && theme::system_fonts_ready()) {
                self.fonts_system = theme::install_fonts(ctx, &self.ui.language);
                self.fonts_lang = self.ui.language.clone();
            }
            self.fonts_ready = true;
        }
    }

    /// Work whose identity is a consumed queue item, revision or deadline.
    pub(super) fn logic_tick(&mut self, ctx: &egui::Context) {
        self.update_screenshot_epoch(ctx);
        self.drain_control(ctx);
        updates_ui::tick(self, ctx);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.dialog_wake.set(ctx.clone());
        // Host-ordered work (an injected batch, a native menu action) holds the deliveries back for the rest of the frame
        // it was seen in, not only for the pass: they resume with the next host iteration. A frame can run a second pass
        // (the framework asks for one when a measured layout changed), and that pass must not open a file right after a
        // native action ran in the first one.
        let frame = ctx.cumulative_frame_nr();
        if self.host_order_pending() {
            self.host_order_frame = Some(frame);
        } else if self.host_order_frame == Some(frame) {
            ctx.request_repaint();
        }
        if self.fonts_ready && !self.host_order_pending() && self.host_order_frame != Some(frame) {
            #[cfg(not(target_arch = "wasm32"))]
            self.handle_file_dialog_answers();
            self.drain_inbox();
            self.open_os_documents();
            #[cfg(not(target_arch = "wasm32"))]
            self.poll_opening(ctx);
            self.poll_link_checks(ctx);
        }
        if self.host_order_pending() {
            ctx.request_repaint();
        }
        self.collect_screenshots(ctx);
        // Ending a stale preview is state cleanup after a control/menu action, not a frame effect:
        // it stays here so a hidden tick still closes a preview whose dialog has gone.
        if !self.host_order_pending() {
            new_document::end_stale_preview(self);
        }
        self.request_hidden_paint_for_pending_input(ctx);
        self.handle_quit(ctx);
        self.flush_window_commands(ctx);
    }

    /// The first pass of a real `run_ui`: fonts, theme, scale and window state, before any widget
    /// is laid out. It must run before the font gate in [`Self::ui`], or the app would wait for
    /// fonts that this function is what installs.
    pub(crate) fn ui_setup(&mut self, ctx: &egui::Context) {
        #[cfg(test)]
        {
            self.logic_frame_ticks += 1;
        }
        if self.egui_ctx.is_none() {
            self.egui_ctx = Some(ctx.clone());
        }
        crate::keys::note_platform(ctx);
        self.sync_keymap();
        self.window_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        self.apply_saved_color_settings();
        let scale = self.ui.ui_scale.clamp(0.5, 3.0);
        if (ctx.zoom_factor() - scale).abs() > 1e-3 {
            ctx.set_zoom_factor(scale);
        }
        self.update_fonts(ctx);
        if !self.fonts_system {
            // The font scan runs in the background: look again shortly.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if self.restyle {
            theme::apply(ctx, &theme::Tokens::for_brightness(self.ui.brightness));
            self.restyle = false;
        }
    }

    /// The end of a settled `run_ui` pass: pacing, recovery and screenshot settlement. Recovery
    /// uses the monotonic clock, so a wall-clock/NTP jump cannot postpone or advance it.
    pub(super) fn ui_frame_end(&mut self, ctx: &egui::Context) {
        let now = now_ms() / 1000.0;
        // Crash recovery: unsaved documents are written to the recovery folder every 30 s, on a
        // background thread — writing a catalogue with gigabytes of placed files took minutes, and
        // on the interface thread the window stopped answering for all of it.
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.session.recovery_dir.is_some() && now - self.last_recovery > (self.session.prefs.recovery_minutes * 60.0).max(5.0) {
                self.last_recovery = now;
                if self.session.documents().iter().any(|d| d.is_dirty()) {
                    match self.session.execute("file.recovery.save", &json!({"background": true})) {
                        Ok(v) if v["queued"].as_u64().unwrap_or(0) > 0 => self.recovery_writing = true,
                        Ok(_) => {}
                        Err(e) => self
                            .status(crate::i18n::tr(&self.ui.language, "Couldn't write recovery data: {error}").replace("{error}", &e.to_string())),
                    }
                }
            }
            if self.recovery_writing {
                self.poll_recovery();
                // Keep the frames coming while the writer works; it has no interface to ask.
                if self.recovery_writing {
                    ctx.request_repaint_after(std::time::Duration::from_millis(150));
                }
            }
        }
        let dt = now - self.last_time;
        if dt > 0.0 {
            self.perf.fps = self.perf.fps * 0.9 + (1.0 / dt).min(240.0) * 0.1;
        }
        self.last_time = now;
        self.issue_screenshots(ctx);
    }

    /// Take what the background recovery writer finished: its checkpoints, and what failed.
    #[cfg(not(target_arch = "wasm32"))]
    fn poll_recovery(&mut self) {
        let Ok(v) = self.session.execute("file.recovery.progress", &json!({})) else { return };
        self.recovery_writing = v["busy"].as_bool().unwrap_or(false);
        let errors: Vec<String> = v["errors"].as_array().into_iter().flatten().filter_map(|e| e.as_str().map(str::to_string)).collect();
        for e in errors {
            let message = crate::i18n::tr(&self.ui.language, "Couldn't write recovery data: {error}").replace("{error}", &e);
            self.status(message);
        }
    }

    /// The document area: the start screen, or the document tabs over one or two canvas panes.
    pub(crate) fn canvas_area(&mut self, ui: &mut egui::Ui, presenting: bool) {
        // The dock's group gives the area its size, so a sizing or measuring run of its body only takes the space: the
        // tabs, the panes, the pane a press focuses and the canvases belong to the run that is drawn (see `canvas::show`).
        if ui.is_sizing_pass() || craft_ui_widgets::runtime::measure::active(ui.ctx()) {
            ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
            return;
        }
        let t = theme::Tokens::get(ui.ctx());
        if self.session.active().is_none() {
            chrome::start_screen(self, ui);
            return;
        }
        if !presenting {
            chrome::doc_tabs(self, ui);
        }
        let split_layout = (self.split && !presenting).then(|| split_panes(ui.available_rect_before_wrap())).flatten();
        if let Some((first, divider, second)) = split_layout {
            // Two panes with a divider; each has its own view (zoom, scroll) and render cache.
            ui.painter().rect_filled(divider, 0.0, t.border);
            // Resolve the clicked pane before either canvas handles this frame's keys.
            if let Some(p) = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten()) {
                if first.contains(p) {
                    self.focus_pane = 0;
                } else if second.contains(p) {
                    self.focus_pane = 1;
                }
            }
            for (k, half) in [first, second].into_iter().enumerate() {
                self.switch_pane(k as u8);
                // A new pane starts at the other pane's view.
                if k == 1 && self.view().is_none() {
                    self.switch_pane(0);
                    let v = self.view().copied();
                    self.switch_pane(1);
                    if let (Some(mut v), Some(m)) = (v, self.view_mut()) {
                        v.fitted = false;
                        *m = v;
                    }
                }
                ui.scope_builder(egui::UiBuilder::new().max_rect(half), |ui| canvas::show(self, ui));
            }
            self.switch_pane(self.focus_pane);
        } else {
            self.switch_pane(0);
            // With a second window, the main one takes the focus back when clicked.
            if !self.second_window || (ui.input(|i| i.pointer.any_pressed()) && ui.ui_contains_pointer()) {
                self.focus_pane = 0;
            }
            canvas::show(self, ui);
        }
    }
}

/// The split view's two side-by-side panes of equal width and the divider between them, which takes
/// [`SPLIT_DIVIDER`] points of the width (all of it in a narrower rect). `None` for a rect with a non-finite or
/// inverted extent.
fn split_panes(rect: egui::Rect) -> Option<(egui::Rect, egui::Rect, egui::Rect)> {
    if !rect.is_finite() || rect.width() < 0.0 || rect.height() < 0.0 {
        return None;
    }
    let divider = SPLIT_DIVIDER.min(rect.width());
    let edge = rect.min.x + (rect.width() - divider) * 0.5;
    let first = egui::Rect::from_min_max(rect.min, egui::pos2(edge, rect.max.y));
    let line = egui::Rect::from_min_max(egui::pos2(edge, rect.min.y), egui::pos2(edge + divider, rect.max.y));
    let second = egui::Rect::from_min_max(egui::pos2(line.max.x, rect.min.y), rect.max);
    Some((first, line, second))
}

/// The width of the line between the split view's panes, in points.
const SPLIT_DIVIDER: f32 = 2.0;

#[cfg(test)]
mod split_tests {
    use super::split_panes;
    use egui::{Rect, pos2};

    /// The panes share the width the divider leaves, the divider sits between them, and the three cover the rect.
    #[test]
    fn the_split_view_gives_each_pane_half_of_what_the_divider_leaves() {
        let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(412.0, 300.0));
        let (first, divider, second) = split_panes(rect).unwrap();
        assert_eq!((first.width(), divider.width(), second.width()), (200.0, 2.0, 200.0));
        assert_eq!((first.min, first.max.x, divider.min.x, divider.max.x, second.max), (rect.min, 210.0, 210.0, 212.0, rect.max));
        assert!([first, divider, second].iter().all(|pane| pane.min.y == rect.min.y && pane.max.y == rect.max.y));
        // A rect narrower than the divider is all divider; an inverted one has no split.
        let (first, divider, second) = split_panes(Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 5.0))).unwrap();
        assert_eq!((first.width(), divider.width(), second.width()), (0.0, 1.0, 0.0));
        assert!(split_panes(Rect::from_min_max(pos2(5.0, 0.0), pos2(1.0, 5.0))).is_none());
    }
}
