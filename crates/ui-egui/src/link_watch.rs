//! Linked graphics whose files changed or went missing, for the canvas's link badges.
//!
//! The statuses come from the Links panel's background checks ([`crate::links_worker`]): each
//! frame the active document is handed to the same worker, which asks the file system how each
//! linked file compares with the copy in the document, off the UI thread, soon after the document
//! changes and every few seconds for changes made elsewhere. The badges show the last answer, so a
//! share that doesn't answer never holds up a frame, and a document's files are asked once whether
//! the panel is shown or not.
//!
//! When the window gains focus again its files are asked again at once. With Preferences › File
//! Handling › Update Modified Links When DesignCraft Comes Back to the Front on (off by default,
//! as in the reference app), the modified graphics that check finds are updated in one step. A
//! graphic opened with Edit Original is updated either way, as soon as a check sees its file
//! changed while the window is in front.
//!
//! The browser can't look at files: nothing is watched there.

// The browser build has no worker and never checks.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::collections::HashMap;

use serde_json::json;

use crate::DesignApp;

/// The preference's label (Preferences › File Handling › Links).
pub const AUTO_UPDATE_LINKS: &str = "Update Modified Links When DesignCraft Comes Back to the Front";

/// How a linked graphic's file compares with the copy in the document, as the last check found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    Ok,
    /// The file differs from the copy.
    Modified,
    /// The file isn't there (or the link isn't followed).
    Missing,
}

impl LinkState {
    /// Its `links.list` status name.
    pub fn name(self) -> &'static str {
        match self {
            LinkState::Ok => "ok",
            LinkState::Modified => "modified",
            LinkState::Missing => "missing",
        }
    }

    /// A `links.list` row's status; `None` for one that isn't a linked file's (embedded).
    fn from_status(status: &str) -> Option<Self> {
        match status {
            "modified" => Some(LinkState::Modified),
            "missing" => Some(LinkState::Missing),
            "ok" => Some(LinkState::Ok),
            _ => None,
        }
    }
}

/// What the watch remembers between frames.
#[derive(Default)]
pub(crate) struct State {
    /// The document the states are of.
    doc: u64,
    /// Linked graphics of that document by asset id, as the last check found them.
    states: HashMap<u64, LinkState>,
    /// When the check `states` came from ended (a newer one replaces them).
    seen: Option<std::time::Instant>,
    /// Whether the window had focus on the last frame (`None` before the host said).
    focused: Option<bool>,
    /// Update the modified graphics found by the first check of the document as it is that
    /// started after this time (focus regained with the preference on).
    update_from: Option<std::time::Instant>,
    /// Whether the document, in the state last looked at, links any file: one that doesn't isn't
    /// checked (nor woken every few seconds for it).
    #[cfg(not(target_arch = "wasm32"))]
    links: Option<(crate::links_worker::Key, bool)>,
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

/// Once per frame: hand the active document to the link checks, and take in what they found.
pub(crate) fn on_frame(app: &mut DesignApp, ctx: &egui::Context) {
    let focused = ctx.input(|i| i.viewport().focused);
    tick(app, ctx, focused);
}

/// [`on_frame`] with the window's focus as the host reports it.
pub(crate) fn tick(app: &mut DesignApp, ctx: &egui::Context, focused: Option<bool>) {
    let was = std::mem::replace(&mut app.link_watch.focused, focused);
    let regained = focused == Some(true) && was == Some(false);
    let Some(uid) = app.session.active().map(|st| st.uid) else {
        app.link_watch.states.clear();
        return;
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        if regained {
            app.links_worker.recheck(uid);
            if app.session.prefs.auto_update_links {
                app.link_watch.update_from = Some(std::time::Instant::now());
            }
        }
        // No check during a gesture: each of its frames is another state of the document, and
        // an update would be undone by its next preview anyway.
        if interacting(app) {
            return;
        }
        let Some((snapshot, links)) = snapshot(app) else { return };
        if !links {
            app.link_watch.states.clear();
            app.link_watch.doc = uid;
            return;
        }
        let (verified, _) = app.links_worker.refresh(ctx, &snapshot);
        // A file the check read and found unchanged isn't read again, in this session or the next.
        if !verified.is_empty() {
            designcraft_engine::cmd::remember_link_files(&mut app.session, &snapshot.doc, &verified);
        }
        take_in(app, snapshot.key);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (ctx, uid, regained);
}

/// The active document as a check sees it, and whether it links any file.
#[cfg(not(target_arch = "wasm32"))]
fn snapshot(app: &mut DesignApp) -> Option<(crate::links_worker::Snapshot, bool)> {
    let st = app.session.active()?;
    let doc = std::sync::Arc::clone(&st.doc);
    let key = crate::links_worker::Key::new(st.uid, &doc, st.path.as_deref());
    let document = st.path.as_ref().map(std::path::PathBuf::from);
    let links = match app.link_watch.links {
        Some((at, links)) if at == key => links,
        _ => {
            let used = doc.used_assets();
            let links = doc.assets.values().any(|a| a.link.is_some() && used.contains(&a.id)) || doc.stories.values().any(|s| s.file_link.is_some());
            app.link_watch.links = Some((key, links));
            links
        }
    };
    let snapshot = crate::links_worker::Snapshot { key, doc, cache: std::sync::Arc::clone(&app.session.cache), document };
    Some((snapshot, links))
}

/// The statuses of the latest check of the active document (shown as `key` now), then the
/// updates they call for: only from a check of the document as it is now, since updating from an
/// older one could reload a graphic an edit (or an undo) has put back since.
#[cfg(not(target_arch = "wasm32"))]
fn take_in(app: &mut DesignApp, key: crate::links_worker::Key) {
    let w = &mut app.link_watch;
    let Some(latest) = app.links_worker.latest(key.uid) else {
        if w.doc != key.uid {
            w.states.clear();
            w.doc = key.uid;
        }
        return;
    };
    if w.doc == key.uid && w.seen == Some(latest.finished) {
        return;
    }
    w.doc = key.uid;
    w.seen = Some(latest.finished);
    w.states = latest.rows.iter().filter_map(|r| Some((r["asset"].as_u64()?, LinkState::from_status(r["status"].as_str()?)?))).collect();
    if latest.key != key {
        return;
    }
    let update = w.update_from.is_some_and(|at| latest.started >= at);
    if update {
        w.update_from = None;
    }
    let mut modified: Vec<u64> = w.states.iter().filter(|(_, s)| **s == LinkState::Modified).map(|(id, _)| *id).collect();
    if modified.is_empty() {
        return;
    }
    modified.sort_unstable();
    if update {
        update_all(app, &modified);
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
fn update_all(app: &mut DesignApp, assets: &[u64]) {
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
