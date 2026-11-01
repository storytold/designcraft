#[cfg(not(target_arch = "wasm32"))]
use super::PendingDrop;
use super::{DesignApp, InputGeneration, Value, menus};

impl DesignApp {
    /// Keep pending synthetic input and screenshot settlement repainting while the window may be
    /// hidden.
    ///
    /// `visible() == None` means unknown and is treated like hidden. The command holds for one
    /// frame and is sent once per tick until a settled UI pass has taken the injected step or
    /// issued the queued capture.
    pub(super) fn request_hidden_paint_for_pending_input(&self, ctx: &egui::Context) {
        let pending = self.host_order_pending() || !self.queued_shots.is_empty();
        if !pending || ctx.input(|i| i.viewport().visible()) == Some(true) {
            return;
        }
        // The 0.37 web backend ignores `RequestPaintWhileHidden` until the fork carries it. Do not
        // pretend a hidden-tab capture or gesture will run there; the request times out instead.
        #[cfg(not(target_arch = "wasm32"))]
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
    }

    /// Inject synthetic events (one pointer event per host iteration), retaining control
    /// provenance until a settled UI pass has actually taken the batch.
    pub fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if !self.injected_settle_hook {
            let pending = self.injected_pending.clone();
            let settled = self.injected_settled.clone();
            ctx.on_end_pass(
                "injected input settlement",
                std::sync::Arc::new(move |ui| {
                    if ui.ctx().viewport_id() == egui::ViewportId::ROOT
                        && pending.load(std::sync::atomic::Ordering::Relaxed)
                        && !ui.ctx().will_discard()
                    {
                        settled.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }),
            );
            self.injected_settle_hook = true;
        }
        // A settled pass that did not go through `DesignApp::ui` (a harness consuming raw input
        // directly) still consumed the batch; acknowledge it before considering the next step.
        if self.injected_settled.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.synthetic_injected = false;
            self.injected_events.clear();
            self.injected_pending.store(false, std::sync::atomic::Ordering::Relaxed);
            self.injected_private = false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pointer = raw.events.iter().rev().find_map(|event| match event {
                egui::Event::PointerMoved(pos) | egui::Event::PointerButton { pos, .. } => Some(*pos),
                _ => None,
            });
            let generation = self.input_generation();
            for file in std::mem::take(&mut raw.dropped_files) {
                let path = file.path().to_string_lossy().to_string();
                if !path.is_empty() {
                    self.pending_drops.push(PendingDrop { path, pointer, generation });
                }
            }
        }
        self.discard_stale_injected_batch(raw);
        // A batch acknowledged by the previous pass keeps its private provenance through that
        // pass's end hook (the clipboard guard), and is cleared at the next host iteration.
        if !self.synthetic_injected {
            self.injected_private = false;
        }
        let private = self.injected_private || !self.control_synthetic.is_empty();
        self.control_input_frame.store(private, std::sync::atomic::Ordering::Relaxed);
        // While the previous batch is still unapplied (it lives in eframe's pending raw input),
        // inject nothing more: one pointer step per UI pass, not per hidden logic tick.
        if self.synthetic_injected {
            return;
        }
        let private = !self.control_synthetic.is_empty();
        let events = if private { &mut self.control_synthetic } else { &mut self.synthetic };
        let Some(first) = events.first() else { return };
        let n = match first {
            egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } => 1,
            _ => events.iter().position(|e| matches!(e, egui::Event::Key { pressed: false, .. })).map_or(events.len(), |i| i + 1),
        };
        let batch: Vec<egui::Event> = events.drain(..n).collect();
        self.injected_generation = self.input_generation();
        self.injected_private = private;
        self.injected_events = batch.clone();
        self.injected_pending.store(true, std::sync::atomic::Ordering::Relaxed);
        raw.events.extend(batch);
        // Held until a settled UI pass runs with the fonts installed; a hidden logic tick, a
        // discarded multipass first pass or a font-gated early return must not acknowledge it.
        self.synthetic_injected = true;
    }

    /// Notify after enqueuing a native menu event, before waking the host. The returned callback
    /// owns its state so the event handler never borrows the app.
    pub fn native_action_notifier(&self, ctx: egui::Context) -> Box<dyn Fn() + Send + Sync> {
        let pending = self.native_events_pending.clone();
        Box::new(move || {
            pending.store(true, std::sync::atomic::Ordering::Release);
            ctx.request_repaint();
        })
    }

    /// Poll from the first real UI pass, under the host's normal UI/watchdog guard. Resolve menu
    /// choices before refreshing their IDs, then execute them once synthetic input has settled.
    /// Event arrival already blocks logic deliveries, so a deferred action keeps its ordering
    /// without executing native dialogs or menu commands from a hidden logic-only tick.
    pub fn poll_native_actions(&mut self, ctx: &egui::Context, mut next: impl FnMut() -> Option<(String, Value)>) {
        // Clear before draining: a notification arriving during the drain stays set for the
        // next pass, even when its channel send races the final empty receive.
        self.native_events_pending.swap(false, std::sync::atomic::Ordering::AcqRel);
        while let Some((id, params)) = next() {
            self.native_actions.push_back((id, params));
        }
        self.apply_native_actions(ctx);
    }

    /// True while an injected batch, control synthetic event or ordinary synthetic event remains queued.
    fn synthetic_input_pending(&self) -> bool {
        self.synthetic_injected || !self.control_synthetic.is_empty() || !self.synthetic.is_empty()
    }

    /// True when synthetic input, a queued native action or a pending native-event notification remains.
    pub(super) fn host_order_pending(&self) -> bool {
        self.synthetic_input_pending() || !self.native_actions.is_empty() || self.native_events_pending.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Execute queued native actions only after synthetic input settles, and request another repaint while host work remains.
    fn apply_native_actions(&mut self, ctx: &egui::Context) {
        while !self.synthetic_input_pending() {
            let Some((id, params)) = self.native_actions.pop_front() else { break };
            menus::activate_native(self, ctx, &id, &params);
        }
        if self.host_order_pending() {
            ctx.request_repaint();
        }
    }

    /// The current state an injected batch belongs to.
    pub(crate) fn input_generation(&self) -> InputGeneration {
        InputGeneration {
            document: self.active_document_epoch(),
            dialog: self.ui.dialog.as_ref().map(|dialog| egui::Id::unique(&dialog.id)),
            focus: self.egui_ctx.as_ref().and_then(|ctx| ctx.memory(|memory| memory.focused())),
        }
    }

    /// Drop a batch whose document, dialog or focus changed before a UI pass could take it.
    /// The batch is still in the host's pending raw input; remove exactly the events we appended
    /// (contiguously, in order) so a subsequent forced paint cannot replay stale input.
    fn discard_stale_injected_batch(&mut self, raw: &mut egui::RawInput) {
        if !self.synthetic_injected || self.input_generation() == self.injected_generation {
            return;
        }
        let events = std::mem::take(&mut self.injected_events);
        if !events.is_empty()
            && let Some(start) = raw.events.windows(events.len()).rposition(|window| window == events.as_slice())
        {
            raw.events.drain(start..start + events.len());
        }
        self.synthetic_injected = false;
        self.injected_pending.store(false, std::sync::atomic::Ordering::Relaxed);
        self.injected_private = false;
        self.control_input_frame.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// A font-gated first pass saw the events before any widget did. Put the same batch back at
    /// the front of its queue (with the original provenance) so a later UI pass consumes it.
    pub(super) fn requeue_injected_batch(&mut self) {
        if !std::mem::take(&mut self.synthetic_injected) {
            return;
        }
        let events = std::mem::take(&mut self.injected_events);
        if !events.is_empty() {
            if self.injected_private {
                self.control_synthetic.splice(0..0, events);
            } else {
                self.synthetic.splice(0..0, events);
            }
        }
        self.injected_pending.store(false, std::sync::atomic::Ordering::Relaxed);
        self.injected_private = false;
        self.control_input_frame.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
