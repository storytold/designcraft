//! Linked graphics whose files changed or went missing, found off the UI thread for the canvas's
//! link badges.
//!
//! Every few seconds, when the window gains focus again and soon after the document changes, the
//! active document's linked graphics are handed to a worker thread, which asks the engine how each
//! file compares with the copy in the document ([`designcraft_engine::cmd::link_state`]). That
//! reads only metadata, except once for a file of the copy's size whose time changed since it was
//! last compared. The frame never waits for it: the badges show the last answer.
//!
//! When the window gains focus again with Preferences › File Handling › Update Modified Links
//! When DesignCraft Comes Back to the Front on (off by default, as in the reference app), the
//! modified graphics found are updated in one step. A graphic opened with Edit Original is
//! updated either way, as soon as a check sees its file changed while the window is in front.
//!
//! The browser can't look at files: nothing is watched there.

// The browser build has no worker and never checks.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use designcraft_engine::cmd::LinkState;
use serde_json::json;

use crate::DesignApp;

/// The preference's label (Preferences › File Handling › Links).
pub const AUTO_UPDATE_LINKS: &str = "Update Modified Links When DesignCraft Comes Back to the Front";

/// Seconds between checks while nothing else asks for one.
pub const CHECK_EVERY: f64 = 3.0;
/// The soonest a document change starts another check (typing changes it on every key).
const AFTER_EDIT: f64 = 0.25;

/// One check: the linked graphics of a document, with the copy each is compared against.
pub(crate) struct Job {
    /// The document and its revision.
    token: (u64, u64),
    document: Option<PathBuf>,
    links: Vec<(u64, String, Arc<Vec<u8>>)>,
    /// Update the modified graphics found (focus regained with the preference on).
    update: bool,
}

/// What a check found.
pub(crate) struct Done {
    token: (u64, u64),
    states: Vec<(u64, LinkState)>,
    update: bool,
}

/// The worker body: compare each linked file with its copy.
fn run(job: Job) -> Done {
    let document = job.document.as_deref();
    let states = job.links.iter().map(|(id, link, data)| (*id, designcraft_engine::cmd::link_state(Some(link), document, data, true))).collect();
    Done { token: job.token, states, update: job.update }
}

#[cfg(not(target_arch = "wasm32"))]
struct Worker {
    tx: std::sync::mpsc::Sender<Job>,
    rx: std::sync::mpsc::Receiver<Done>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Worker {
    fn spawn(ctx: Option<egui::Context>) -> Option<Self> {
        let (tx, jobs) = std::sync::mpsc::channel::<Job>();
        let (done_tx, rx) = std::sync::mpsc::channel::<Done>();
        std::thread::Builder::new()
            .name("designcraft-links".into())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    if done_tx.send(run(job)).is_err() {
                        break;
                    }
                    if let Some(ctx) = &ctx {
                        ctx.request_repaint();
                    }
                }
            })
            .ok()?;
        Some(Worker { tx, rx })
    }
}

/// What the watch remembers between frames.
#[derive(Default)]
pub(crate) struct State {
    /// The document the states are of.
    doc: u64,
    /// Linked graphics of that document by asset id, as the last check found them.
    states: HashMap<u64, LinkState>,
    /// When the last check started (egui time, seconds), and what it looked at.
    last: Option<f64>,
    token: Option<(u64, u64)>,
    /// A check is running.
    busy: bool,
    /// Whether the window had focus on the last frame (`None` before the host said).
    focused: Option<bool>,
    /// Focus came back while a check was running: check again (and update, if `update`) next.
    again: bool,
    update: bool,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<Worker>,
    /// Check on the calling thread instead of the worker (tests).
    pub(crate) inline: bool,
}

impl State {
    /// How the last check found asset `asset` of document `doc`.
    pub fn state(&self, doc: u64, asset: u64) -> Option<LinkState> {
        (self.doc == doc).then(|| self.states.get(&asset).copied()).flatten()
    }

    /// Whether some graphic of document `doc` is modified or missing (a badge to draw).
    pub fn flagged(&self, doc: u64) -> bool {
        self.doc == doc && self.states.values().any(|s| matches!(s, LinkState::Modified | LinkState::Missing))
    }

    /// Records `state` for `asset` (it was just updated), until the next check.
    pub(crate) fn set(&mut self, doc: u64, asset: u64, state: LinkState) {
        if self.doc == doc {
            self.states.insert(asset, state);
        }
    }
}

/// Once per frame: start a check when one is due, and take in the last one's results.
pub(crate) fn on_frame(app: &mut DesignApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (now, focused) = ctx.input(|i| (i.time, i.viewport().focused));
        if app.link_watch.worker.is_none() && !app.link_watch.inline {
            app.link_watch.worker = Worker::spawn(Some(ctx.clone()));
        }
        tick(app, now, focused);
        // Look again in a while even when nothing else repaints.
        if !app.link_watch.states.is_empty() || app.link_watch.busy {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(CHECK_EVERY));
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, ctx);
}

/// [`on_frame`] at time `now` with the window's focus as the host reports it.
pub(crate) fn tick(app: &mut DesignApp, now: f64, focused: Option<bool>) {
    receive(app);
    let was = std::mem::replace(&mut app.link_watch.focused, focused);
    let regained = focused == Some(true) && was == Some(false);
    if regained {
        app.link_watch.again = true;
        app.link_watch.update |= app.session.prefs.auto_update_links;
    }
    let Some(st) = app.session.active() else {
        app.link_watch.states.clear();
        return;
    };
    // No check during a gesture: what it found would wait for the gesture to end anyway.
    if st.interaction.is_some() {
        return;
    }
    let token = (st.uid, st.revision);
    let w = &app.link_watch;
    let since = w.last.map_or(f64::INFINITY, |t| now - t);
    let due = w.again || since >= CHECK_EVERY || (w.token != Some(token) && since >= AFTER_EDIT) || since < 0.0;
    // A check that never came back (its thread gone) doesn't stop the next ones for long.
    if w.busy && since > 10.0 * CHECK_EVERY {
        app.link_watch.busy = false;
    }
    let w = &app.link_watch;
    if !due || w.busy {
        return;
    }
    let job = job(app, token);
    let w = &mut app.link_watch;
    w.last = Some(now);
    w.token = Some(token);
    w.again = false;
    w.update = false;
    submit(app, job);
}

/// The active document's linked graphics, for a check.
fn job(app: &DesignApp, token: (u64, u64)) -> Job {
    let mut links = Vec::new();
    let mut document = None;
    if let Some(st) = app.session.active() {
        document = st.path.as_deref().map(PathBuf::from);
        let used = st.doc.used_assets();
        for a in st.doc.assets.values().filter(|a| used.contains(&a.id)) {
            if let Some(link) = &a.link {
                links.push((a.id.0, link.clone(), a.data.clone()));
            }
        }
    }
    Job { token, document, links, update: app.link_watch.update }
}

fn submit(app: &mut DesignApp, job: Job) {
    #[cfg(not(target_arch = "wasm32"))]
    let job = match &app.link_watch.worker {
        Some(w) if !app.link_watch.inline => match w.tx.send(job) {
            Ok(()) => {
                app.link_watch.busy = true;
                return;
            }
            // The worker is gone (its thread ended): the next frame starts another, and this
            // check runs here.
            Err(std::sync::mpsc::SendError(job)) => {
                app.link_watch.worker = None;
                job
            }
        },
        _ => job,
    };
    // Tests (and a host whose worker couldn't start or has gone) check here.
    let done = run(job);
    apply(app, done);
}

/// Take in a finished check, if there is one.
fn receive(app: &mut DesignApp) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut last = None;
        if let Some(w) = &app.link_watch.worker {
            while let Ok(d) = w.rx.try_recv() {
                last = Some(d);
            }
        }
        if let Some(done) = last {
            app.link_watch.busy = false;
            apply(app, done);
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = app;
}

/// A check's results: the badges, then the updates they call for.
fn apply(app: &mut DesignApp, done: Done) {
    // A check of a document that is no longer active is stale.
    if app.session.active().map(|st| st.uid) != Some(done.token.0) {
        return;
    }
    let w = &mut app.link_watch;
    w.doc = done.token.0;
    w.states = done.states.iter().copied().collect();
    let modified: Vec<u64> = done.states.iter().filter(|(_, s)| *s == LinkState::Modified).map(|(id, _)| *id).collect();
    if modified.is_empty() {
        return;
    }
    // An update made during a gesture would be undone by its next preview, which starts again
    // from the document as the gesture found it: check again (and update) once it ends.
    if interacting(app) {
        app.link_watch.again = true;
        app.link_watch.update |= done.update;
        return;
    }
    if done.update {
        update(app, &modified);
    } else if app.link_watch.focused != Some(false) {
        // Files opened with Edit Original are updated once they changed, the window in front.
        crate::edit_original::refresh_edited(app);
    }
}

/// A canvas gesture (or another interaction) is under way in the active document.
pub(crate) fn interacting(app: &DesignApp) -> bool {
    app.session.active().is_some_and(|st| st.interaction.is_some())
}

/// Update `assets` in one step (`links.update {assets}`).
fn update(app: &mut DesignApp, assets: &[u64]) {
    match app.run("links.update", json!({ "assets": assets })) {
        Ok(_) => {
            let doc = app.link_watch.doc;
            for id in assets {
                app.link_watch.set(doc, *id, LinkState::Ok);
            }
        }
        Err(e) => app.status(e),
    }
}

/// A badge was clicked: update its graphic (one undo step) → whether it was updated.
pub(crate) fn update_one(app: &mut DesignApp, asset: u64) -> bool {
    match app.run("links.update", json!({ "asset": asset })) {
        Ok(r) => {
            let doc = app.session.active().map_or(0, |st| st.uid);
            app.link_watch.set(doc, asset, LinkState::Ok);
            r["updated"].as_u64().is_some_and(|n| n > 0)
        }
        Err(e) => {
            app.status(e);
            false
        }
    }
}

#[cfg(test)]
mod tests;
