use super::clock::now_ms;
#[cfg(test)]
use super::services::Inbox;
use super::services::Services;
use super::state::{CanvasCache, ImportRequest, ImportedFile, Perf, ScreenMode, UiState, View, second_window_viewport};
use super::*;

mod clipboard;
mod commands;
mod control;
mod files;
mod frame;
mod ime;
mod input;
mod keys;
mod quit;
mod screenshot;
mod view;

#[cfg(test)]
pub(crate) use files::browser_import_tests;
pub use files::opens_as_document;

impl DesignApp {
    /// Start the background font scan, establish the monotonic timing origin, and initialize document, view, input, screenshot, dialog and control state.
    pub fn new(session: Session, services: Services) -> Self {
        // The font menus and the first file opened need the installed fonts: catalog them now.
        designcraft_fonts::FontDb::global().scan_in_background();
        // The app owns the monotonic origin for recovery timing and frame pacing; `now_ms` itself
        // is process-global so screenshot deadlines stay comparable across hosts.
        let now = now_ms() / 1000.0;
        DesignApp {
            pending_pdf: None,
            session,
            ui: UiState::default(),
            services,
            views: HashMap::new(),
            page_history: HashMap::new(),
            canvas: CanvasCache::new(),
            canvas_rect: None,
            split: false,
            second_window: false,
            pane: 0,
            focus_pane: 0,
            other_pane: None,
            power_zoom: None,
            color_applied: false,
            perf: Perf::default(),
            thumbnails: Default::default(),
            synthetic: vec![],
            control_synthetic: vec![],
            native_actions: Default::default(),
            native_events_pending: Default::default(),
            control_input_frame: Default::default(),
            clipboard_guard_installed: false,
            story_editor: None,
            control_rx: None,
            pending_shots: vec![],
            queued_shots: vec![],
            shot_token: 0,
            screenshot_geometry: None,
            screenshot_epoch: 0,
            synthetic_injected: false,
            injected_events: Vec::new(),
            injected_private: false,
            injected_generation: InputGeneration::default(),
            injected_pending: Default::default(),
            injected_settled: Default::default(),
            injected_settle_hook: false,
            #[cfg(not(target_arch = "wasm32"))]
            pending_drops: Vec::new(),
            #[cfg(test)]
            logic_tick_ticks: 0,
            #[cfg(test)]
            logic_frame_ticks: 0,
            #[cfg(test)]
            input_ticks: 0,
            styled: false,
            fonts_lang: String::new(),
            fonts_system: false,
            restyle: false,
            fonts_ready: false,
            os_documents: Vec::new(),
            integrated_titlebar: false,
            custom_titlebar: false,
            window_maximized: false,
            pending_window_commands: vec![],
            native_menu: false,
            native_shortcuts: Default::default(),
            keymap: crate::keys::new_keymap(craft_keymap::Overrides::new()),
            key_dispatcher: craft_keymap::egui::Dispatcher::new(),
            shortcut_dialog: None,
            shortcut_answers: Default::default(),
            last_time: now,
            last_recovery: now,
            egui_ctx: None,
            in_control: false,
            object_clip_text: None,
            ime_marked: None,
            ime_discard: false,
            canvas_ime: false,
            ime_tracker: egui::ime_context::ImeContextTracker::new(),
            pending_ime_delete: None,
            ime_target: None,
            ime_interrupt: false,
            ime_canvas: None,
            quit_requested: false,
            native_close_approved: false,
            close_requested_pending: false,
            quit_without_asking: false,
            quitting: None,
            cursors: cursors::Images::default(),
            live_preview: LivePreview::default(),
            updates_rt: updates_ui::Runtime::default(),
        }
    }

    pub fn status(&mut self, s: impl Into<String>) {
        self.ui.status = s.into();
    }

    /// "<feature>: <error>" in the status bar: the feature name is translated, the engine's own
    /// error text stays as the engine wrote it.
    pub fn status_error(&mut self, feature: &str, error: impl std::fmt::Display) {
        self.ui.status = format!("{}: {error}", crate::i18n::tr(&self.ui.language, feature));
    }

    /// First frame only: load the saved colour profiles and make the saved Color Settings active.
    fn apply_saved_color_settings(&mut self) {
        if self.color_applied {
            return;
        }
        self.color_applied = true;
        for path in self.ui.color_profiles.clone() {
            if let Err(e) = self.session.execute("color.loadProfile", &json!({"path": path})) {
                log::warn!("Color Settings: {e}");
                self.status_error("Color Settings", e);
            }
        }
        if let Some(cs) = self.ui.color_settings.clone() {
            let known = cs.with_known_profiles();
            for (saved, used) in [(&cs.rgb, &known.rgb), (&cs.cmyk, &known.cmyk)] {
                if saved != used {
                    let e = format!("saved profile {saved} is not available; using {used}");
                    log::warn!("Color Settings: {e}");
                    self.status_error("Color Settings", e);
                }
            }
            let _ = designcraft_color::cms::set_active(&known);
        }
    }

    /// Queue and deadline work, run once per app callback.
    ///
    /// `logic_tick` runs on every call (a first pass, a later discarded pass, or a hidden
    /// `run_logic` tick) and consumes queues and deadlines once. Work that is genuinely once per
    /// presented layout frame lives in [`Self::ui_setup`] / [`Self::ui_frame_end`], which only a
    /// real `run_ui` pass calls. A hidden `run_logic` tick also reports pass index zero, so the
    /// pass index alone cannot identify a UI pass.
    pub fn logic(&mut self, ctx: &egui::Context) {
        #[cfg(test)]
        {
            self.logic_tick_ticks += 1;
        }
        self.logic_tick(ctx);
    }

    /// Lay out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        #[cfg(test)]
        {
            self.input_ticks += 1;
        }
        let ctx = ui.ctx().clone();
        self.canvas_ime = false;
        self.ime_canvas = None;
        self.ime_interrupt = false;
        // Fonts/theme must be installed from the real UI pass, before the font gate below: doing
        // this only from a hidden logic tick would make the first shown pass wait forever.
        if ctx.current_pass_index() == 0 {
            self.ui_setup(&ctx);
        }
        if !self.fonts_ready {
            // The first pass installs fonts and returns before any widget sees this frame's input.
            // Put an injected step back rather than acknowledge it as consumed.
            self.requeue_injected_batch();
            ctx.request_repaint();
            return;
        }
        self.settle_live_preview();
        // Input consumers run once, in the live first UI pass, before layout. Hidden logic ticks
        // must not replay shortcuts or dropped files from stale input.
        if ctx.current_pass_index() == 0 {
            #[cfg(not(target_arch = "wasm32"))]
            {
                // Sample the generation once for the whole batch: placing the first valid file
                // must not suppress later files that belong to the same captured batch.
                let current = self.input_generation();
                for drop in std::mem::take(&mut self.pending_drops) {
                    if drop.generation != current {
                        continue;
                    }
                    let _ = self.open_dropped_at(&drop.path, drop.pointer);
                }
            }
            menus::shortcuts(self, &ctx);
        }
        let t0 = now_ms();
        let t = theme::Tokens::get(&ctx);
        let presenting = self.ui.screen_mode == ScreenMode::Presentation;
        if presenting || self.ui.hidden_panels != 0 {
            panel_docking::cancel_gesture(&ctx);
        }
        canvas::begin_frame(&ctx);
        let dbg = std::env::var_os("DESIGNCRAFT_DEBUG_LAYOUT").is_some();
        if dbg {
            eprintln!("root start {:?}", ui.available_rect_before_wrap());
        }
        if !presenting {
            chrome::app_bar(self, ui);
            if dbg {
                eprintln!("after app bar {:?}", ui.available_rect_before_wrap());
            }
            if self.ui.control_bar && self.session.active().is_some() && self.ui.hidden_panels == 0 {
                chrome::control_bar(self, ui);
            }
            chrome::status_bar(self, ui);
            if dbg {
                eprintln!("after status {:?}", ui.available_rect_before_wrap());
            }
            if self.ui.hidden_panels != 1 {
                toolbar::show(self, ui);
            }
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.pasteboard)).show(ui, |ui| {
            // The panel layout holds the canvas as its protected `canvas` panel.
            if presenting || self.ui.hidden_panels != 0 {
                self.canvas_area(ui, presenting);
            } else {
                panel_docking::show(self, ui);
            }
        });
        // New Window: pane 1 in a window of its own.
        if self.second_window
            && !presenting
            && let Some(title) = self.session.active().map(|d| format!("{} — 2", d.doc.title))
        {
            let fill = t.pasteboard;
            let ctx = ui.ctx().clone();
            ctx.show_viewport_immediate(
                second_window_viewport(),
                egui::ViewportBuilder::default().with_title(title).with_inner_size([960.0, 720.0]),
                |ui, _class| {
                    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(fill)).show(ui, |ui| {
                        self.switch_pane(1);
                        if self.view().is_none() {
                            self.switch_pane(0);
                            let v = self.view().copied();
                            self.switch_pane(1);
                            if let (Some(mut v), Some(m)) = (v, self.view_mut()) {
                                v.fitted = false;
                                *m = v;
                            }
                        }
                        if ui.input(|i| i.pointer.any_pressed()) {
                            self.focus_pane = 1;
                        }
                        canvas::show(self, ui);
                    });
                    if ui.input(|i| i.viewport().close_requested()) {
                        self.end_second_ime_viewport();
                        self.second_window = false;
                        self.focus_pane = 0;
                    }
                },
            );
            self.switch_pane(self.focus_pane);
        } else if self.second_window {
            // The child viewport does not run this pass (Presentation, or no document for its
            // title): its pass counter stops advancing, so treat it as gone and let the tracker
            // start a fresh context when the window is shown again.
            self.end_second_ime_viewport();
        }
        // Tab, Shift+Tab and Presentation hide the floating panels with the docked ones.
        if !presenting && self.ui.hidden_panels == 0 {
            dock::float_only_windows(self, &ctx);
        }
        story_editor::show(self, &ctx);
        dialogs::show(self, &ctx);
        about::show(self, &ctx);
        updates_ui::show(self, &ctx);
        menus::palette(self, &ctx);
        // The IME is observed once, here: after the canvas, the floating panels, the story editor,
        // dialogs and Quick Apply have all applied this pass's commands and focus changes.
        canvas::ime_observe(self, &ctx);
        if self.custom_titlebar {
            chrome::resize_borders(self, ui);
        }
        for url in std::mem::take(&mut self.ui.pending_urls) {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        if !ctx.will_discard() {
            // A settled UI pass has taken the injected step (a discarded pass has not). Keep
            // `injected_private` until the next raw-input hook, so this pass's end-pass clipboard
            // guard still sees the control provenance.
            self.synthetic_injected = false;
            self.injected_events.clear();
            self.injected_pending.store(false, std::sync::atomic::Ordering::Relaxed);
            self.injected_settled.store(false, std::sync::atomic::Ordering::Relaxed);
            self.ui_frame_end(&ctx);
        }
        self.perf.frame_ms = now_ms() - t0;
    }
}
