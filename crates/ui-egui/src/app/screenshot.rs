use super::{
    ControlResponse, DesignApp, SCREENSHOT_MAX_AGGREGATE_BYTES, SCREENSHOT_MAX_IMAGE_BYTES, SCREENSHOT_MAX_REQUESTS, ScreenshotGeometry, Sender,
    ShotRequest, json, now_ms,
};

impl DesignApp {
    /// Advance the screenshot geometry epoch whenever size, DPI, zoom or surface change.
    pub(super) fn update_screenshot_epoch(&mut self, ctx: &egui::Context) {
        let Some(mut geometry) = screenshot_geometry(ctx) else { return };
        geometry.document = self.active_document_epoch();
        if self.screenshot_geometry != Some(geometry) {
            self.screenshot_geometry = Some(geometry);
            self.screenshot_epoch = self.screenshot_epoch.wrapping_add(1);
        }
    }

    /// Admit one `ui.screenshot` request against the app's count and byte budgets.
    ///
    /// Admission happens before any capture is issued: the checked, current viewport dimensions
    /// reserve their RGBA bytes, and queued plus completed images are counted together.
    pub(crate) fn admit_screenshot(&mut self, path: Option<String>, reply: Sender<ControlResponse>) -> Result<u64, serde_json::Value> {
        let Some(mut geometry) = self.screenshot_geometry else {
            return Err(json!({"ok": false, "error": "the window geometry is not known yet"}));
        };
        // The cached geometry may predate a document switch handled earlier in this same logic
        // tick. Admit for the current document, not for the one the cache last saw.
        geometry.document = self.active_document_epoch();
        if self.pending_shots.len() >= SCREENSHOT_MAX_REQUESTS {
            return Err(json!({"ok": false, "error": format!("at most {SCREENSHOT_MAX_REQUESTS} screenshot requests may be outstanding")}));
        }
        let reserved_bytes = geometry
            .width
            .checked_mul(geometry.height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| json!({"ok": false, "error": "the screenshot dimensions overflow the addressable size"}))?;
        if reserved_bytes > SCREENSHOT_MAX_IMAGE_BYTES {
            return Err(json!({
                "ok": false,
                "error": format!(
                    "a screenshot of {}×{} exceeds the {SCREENSHOT_MAX_IMAGE_BYTES}-byte image limit",
                    geometry.width, geometry.height
                )
            }));
        }
        let used = self.pending_shots.iter().try_fold(0usize, |total, shot| total.checked_add(shot.reserved_bytes));
        let over_budget = used.and_then(|used| used.checked_add(reserved_bytes)).is_none_or(|total| total > SCREENSHOT_MAX_AGGREGATE_BYTES);
        if over_budget {
            return Err(json!({"ok": false, "error": format!("the {SCREENSHOT_MAX_AGGREGATE_BYTES}-byte screenshot budget is exhausted")}));
        }
        self.shot_token += 1;
        let token = self.shot_token;
        let now = now_ms();
        self.queued_shots.push((token, now + 120.0, 0));
        self.pending_shots.push(ShotRequest {
            token,
            path,
            reply,
            deadline: now + 8000.0,
            viewport: egui::ViewportId::ROOT,
            epoch: self.screenshot_epoch,
            geometry,
            reserved_bytes,
            slot: None,
        });
        Ok(token)
    }

    /// The current document state (`0` when none is open). Revert preserves the UID but
    /// gives the replacement state a new epoch, invalidating old captures and input.
    pub(super) fn active_document_epoch(&self) -> u64 {
        self.session.active().map_or(0, |document| document.identity_epoch)
    }

    /// Reject one admitted request: remove its admission and queued token and cancel any live
    /// capture slot before replying with the error.
    fn reject_screenshot(&mut self, index: usize, message: &str) {
        let shot = self.pending_shots.remove(index);
        self.queued_shots.retain(|(token, ..)| *token != shot.token);
        if let Some(slot) = &shot.slot {
            slot.cancel();
        }
        let _ = shot.reply.send(json!({"ok": false, "error": message}));
    }

    /// Settle and issue queued captures. Called only at the end of a settled `run_ui` pass, so
    /// `frames` counts actual layout passes; hidden logic ticks never satisfy the three-frame
    /// settling policy on their own.
    pub(super) fn issue_screenshots(&mut self, ctx: &egui::Context) {
        let now = now_ms();
        let mut due = Vec::new();
        self.queued_shots.retain_mut(|(token, at, frames)| {
            *frames += 1;
            if now >= *at && *frames >= 3 {
                due.push(*token);
                false
            } else {
                true
            }
        });
        for token in due {
            let Some(index) = self.pending_shots.iter().position(|shot| shot.token == token) else { continue };
            if self.pending_shots[index].geometry.document != self.active_document_epoch() {
                self.reject_screenshot(index, "the active document changed before the screenshot was issued");
                continue;
            }
            let Some(current) = self.screenshot_geometry else {
                let shot = self.pending_shots.remove(index);
                let _ = shot.reply.send(json!({"ok": false, "error": "the window geometry is not known yet"}));
                continue;
            };
            // Re-check the current dimensions before issuing: a request admitted before a resize,
            // DPI or zoom change must renew its checked reservation (or fail, never overrun it).
            if self.pending_shots[index].epoch != self.screenshot_epoch {
                let bytes = current.width.checked_mul(current.height).and_then(|pixels| pixels.checked_mul(4));
                let others = self
                    .pending_shots
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != index)
                    .try_fold(0usize, |total, (_, shot)| total.checked_add(shot.reserved_bytes));
                let reserved = bytes.zip(others).and_then(|(bytes, others)| others.checked_add(bytes).map(|total| (bytes, total)));
                let fits = reserved.is_some_and(|(bytes, total)| bytes <= SCREENSHOT_MAX_IMAGE_BYTES && total <= SCREENSHOT_MAX_AGGREGATE_BYTES);
                let Some((bytes, _)) = reserved else {
                    let shot = self.pending_shots.remove(index);
                    let _ = shot.reply.send(json!({
                        "ok": false,
                        "error": "the window changed and the screenshot no longer fits the admitted budget"
                    }));
                    continue;
                };
                if !fits {
                    let shot = self.pending_shots.remove(index);
                    let _ = shot.reply.send(json!({
                        "ok": false,
                        "error": "the window changed and the screenshot no longer fits the admitted budget"
                    }));
                    continue;
                }
                let shot = &mut self.pending_shots[index];
                shot.epoch = self.screenshot_epoch;
                shot.geometry = current;
                shot.reserved_bytes = bytes;
            }
            let shot = &mut self.pending_shots[index];
            let (slot, callback) = craft_ui::capture::CaptureSlot::new(ctx, shot.viewport, SCREENSHOT_MAX_IMAGE_BYTES);
            shot.slot = Some(slot);
            // The callback holds only a weak slot: it never captures the app, writes a file, runs a
            // command or wakes the context itself (the slot's live request does that).
            ctx.send_viewport_cmd_to(shot.viewport, egui::ViewportCommand::Screenshot(callback));
        }
        if !self.queued_shots.is_empty() || !self.pending_shots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        // The screenshot command itself forces one hidden paint; the settling frames before it do
        // not, so ask for them while the window may be hidden. The 0.37 web backend does not yet
        // honor this command (fork follow-up): hidden-tab captures time out there rather than
        // pretending to settle.
        if !self.queued_shots.is_empty() && ctx.input(|i| i.viewport().visible()) != Some(true) {
            #[cfg(not(target_arch = "wasm32"))]
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        }
    }

    /// Take completed captures out of their slots and write files only on this (UI) thread.
    pub(super) fn collect_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_shots.is_empty() {
            return;
        }
        let now = now_ms();
        let current_document = self.active_document_epoch();
        let mut i = 0;
        while i < self.pending_shots.len() {
            // Check before taking the callback: a request whose document changed must not write,
            // retry, or wait for its slot, even when no frame has completed yet.
            if self.pending_shots.get(i).is_some_and(|shot| shot.geometry.document != current_document) {
                self.reject_screenshot(i, "the active document changed while the screenshot was pending");
                continue;
            }
            let taken = self.pending_shots.get(i).and_then(|shot| shot.slot.as_ref()).and_then(|slot| slot.take());
            let Some(result) = taken else {
                if now >= self.pending_shots[i].deadline {
                    let shot = self.pending_shots.remove(i);
                    self.queued_shots.retain(|(token, ..)| *token != shot.token);
                    if let Some(slot) = &shot.slot {
                        slot.cancel();
                    }
                    let _ = shot.reply.send(json!({"ok": false, "error": "no frame was presented (screen locked or window hidden); use ui.render"}));
                } else {
                    i += 1;
                }
                continue;
            };
            let (epoch_changed, within_reserved) = {
                let shot = &self.pending_shots[i];
                let within = match &result {
                    Ok(image) => {
                        let [width, height] = image.size;
                        width <= shot.geometry.width && height <= shot.geometry.height
                    }
                    Err(_) => true,
                };
                (shot.epoch != self.screenshot_epoch, within)
            };
            if !epoch_changed && within_reserved {
                let shot = self.pending_shots.remove(i);
                self.queued_shots.retain(|(token, ..)| *token != shot.token);
                let value = match result {
                    Ok(image) => crate::control::save_screenshot(self, image.as_ref(), shot.path.as_deref()),
                    Err(e) => json!({"ok": false, "error": e.to_string()}),
                };
                let _ = shot.reply.send(value);
                continue;
            }
            let (retry, retry_token) = {
                let shot = &self.pending_shots[i];
                (now < shot.deadline, shot.token)
            };
            if let Some(slot) = self.pending_shots[i].slot.take() {
                slot.cancel();
            }
            if !retry {
                let shot = self.pending_shots.remove(i);
                self.queued_shots.retain(|(token, ..)| *token != shot.token);
                let message = match result {
                    Err(e) => e.to_string(),
                    Ok(_) if epoch_changed => "the window changed while the screenshot was captured".to_string(),
                    Ok(_) => "the screenshot is larger than the admitted viewport".to_string(),
                };
                let _ = shot.reply.send(json!({"ok": false, "error": message}));
                continue;
            }
            // Reject the stale pixels and retry within the same deadline with a fresh epoch.
            self.queued_shots.push((retry_token, now + 120.0, 0));
            i += 1;
        }
        if !self.queued_shots.is_empty() || !self.pending_shots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
}

/// The physical capture surface of the root viewport, from its logical rect and DPI.
fn screenshot_geometry(ctx: &egui::Context) -> Option<ScreenshotGeometry> {
    let zoom = ctx.zoom_factor();
    let fallback = ctx.pixels_per_point();
    let (rect, native, fullscreen) = ctx.input(|i| {
        let viewport = i.viewport();
        // Web (eframe's web backend) and egui_kittest set `screen_rect` but not `inner_rect`;
        // both describe the same logical content rect for this viewport.
        (viewport.inner_rect.or(i.raw.screen_rect), viewport.native_pixels_per_point, viewport.fullscreen)
    });
    let rect = rect?;
    let pixels_per_point = match native {
        // `native_pixels_per_point` is before egui's zoom
        // (`Context::pixels_per_point() == zoom_factor * native_pixels_per_point`), so apply the
        // zoom exactly once. The fallback already includes it.
        Some(native) => native * zoom,
        None => fallback,
    };
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return None;
    }
    let width = (rect.width() * pixels_per_point).ceil();
    let height = (rect.height() * pixels_per_point).ceil();
    if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 {
        return None;
    }
    Some(ScreenshotGeometry {
        width: width as usize,
        height: height as usize,
        document: 0,
        signature: [
            rect.width().to_bits(),
            rect.height().to_bits(),
            pixels_per_point.to_bits(),
            u32::from(fullscreen.unwrap_or(false)),
            zoom.to_bits(),
            // Reserved for a surface-generation counter once a backend exposes one.
            0,
        ],
    })
}
