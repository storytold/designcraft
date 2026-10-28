use super::*;

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
        self.thumbnails.retain_open(self.session.documents().iter().any(|d| d.uid == self.thumbnails.document_uid()));
        self.canvas.previews.retain_open(self.session.documents().iter().any(|d| d.uid == self.canvas.previews.document_uid()));
        if let Some((canvas, _)) = &mut self.other_pane {
            canvas.previews.retain_open(self.session.documents().iter().any(|d| d.uid == canvas.previews.document_uid()));
        }
        if self.session.active().is_none() {
            self.canvas.drag.clear();
            self.canvas.previews.clear();
            if let Some((canvas, _)) = &mut self.other_pane {
                canvas.drag.clear();
                canvas.previews.clear();
            }
        }
        self.update_screenshot_epoch(ctx);
        self.drain_control(ctx);
        updates_ui::tick(self, ctx);
        if self.fonts_ready && !self.host_order_pending() {
            self.drain_inbox();
            self.open_os_documents();
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
        // Crash recovery: unsaved documents are written to the recovery folder every 30 s.
        #[cfg(not(target_arch = "wasm32"))]
        if self.session.recovery_dir.is_some() && now - self.last_recovery > (self.session.prefs.recovery_minutes * 60.0).max(5.0) {
            self.last_recovery = now;
            if self.session.documents().iter().any(|d| d.is_dirty())
                && let Err(e) = self.session.execute("file.recovery.save", &json!({}))
            {
                self.status(crate::i18n::tr(&self.ui.language, "Couldn't write recovery data: {error}").replace("{error}", &e.to_string()));
            }
        }
        let dt = now - self.last_time;
        if dt > 0.0 {
            self.perf.fps = self.perf.fps * 0.9 + (1.0 / dt).min(240.0) * 0.1;
        }
        self.last_time = now;
        self.issue_screenshots(ctx);
    }

    /// The document area: the start screen, or the document tabs over one or two canvas panes.
    pub(crate) fn canvas_area(&mut self, ui: &mut egui::Ui, presenting: bool) {
        let t = theme::Tokens::get(ui.ctx());
        if self.session.active().is_none() {
            chrome::start_screen(self, ui);
            return;
        }
        if !presenting {
            chrome::doc_tabs(self, ui);
        }
        let split_layout = (self.split && !presenting)
            .then(|| {
                craft_ui::layout::split_rect(
                    ui.available_rect_before_wrap(),
                    craft_ui::layout::SplitAxis::Horizontal,
                    craft_ui::layout::SplitSize::Ratio(0.5),
                    2.0,
                    0.0,
                )
            })
            .flatten();
        if let Some(layout) = split_layout {
            // Two panes with a divider; each has its own view (zoom, scroll) and render cache.
            ui.painter().rect_filled(layout.divider, 0.0, t.border);
            // Resolve the clicked pane before either canvas handles this frame's keys.
            if let Some(p) = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten()) {
                if layout.first.contains(p) {
                    self.focus_pane = 0;
                } else if layout.second.contains(p) {
                    self.focus_pane = 1;
                }
            }
            for (k, half) in [layout.first, layout.second].into_iter().enumerate() {
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
