//! Native DesignCraft projects over an optional same-origin storage provider.
use crate::{
    DesignApp,
    file_providers::{Action, Capture, Host},
};
use serde_json::{Value, json};

pub(crate) mod drafts;

const MAX_CONTINUATIONS: usize = 16;

pub(crate) enum After {
    Close(u64),
    Quit(u64),
    #[cfg(test)]
    Test(Box<dyn FnOnce(&mut DesignApp) -> Result<Value, String>>),
}

impl After {
    pub(crate) fn local(self, app: &mut DesignApp) -> Result<Value, String> {
        match self {
            Self::Close(uid) => {
                let index = app.session.documents().iter().position(|doc| doc.uid == uid && !doc.is_dirty());
                match index {
                    Some(index) => app.run("file.close", json!({"index":index})),
                    None => Ok(Value::Null),
                }
            }
            Self::Quit(uid) => app.quit_document_saved(uid),
            #[cfg(test)]
            Self::Test(callback) => callback(app),
        }
    }

    fn provider(self, app: &mut DesignApp, identity: (u64, u64)) -> Result<Value, String> {
        match self {
            Self::Close(uid) if uid == identity.0 => {
                // The generic command preamble commits the active document's IME/preview.
                // Close this already-validated identity directly, preserving the active UID.
                if !app.session.close_document_by_uid(identity.0, identity.1) {
                    return Err("captured document closed".into());
                }
                app.after_engine();
                Ok(Value::Null)
            }
            Self::Quit(uid) if uid == identity.0 => app.quit_document_saved(uid),
            #[cfg(test)]
            Self::Test(callback) => callback(app),
            _ => Err("save continuation belongs to another document".into()),
        }
    }
}

/// A UI continuation belongs to one captured identity and connection, never a tab ordinal.
pub(crate) struct PendingAfter {
    uid: u64,
    identity_epoch: u64,
    revision: u64,
    connection: String,
    ready: bool,
    callback: After,
}

pub(crate) fn busy(app: &DesignApp) -> bool {
    app.file_providers.busy() || !app.provider_after.is_empty() || app.egui_ctx.as_ref().is_some_and(drafts::any_pending)
}

fn pending_for(app: &DesignApp, uid: u64) -> bool {
    app.file_providers.pending_for(uid)
        || app.provider_after.values().any(|after| after.uid == uid)
        || app.egui_ctx.as_ref().is_some_and(|ctx| {
            app.session.documents().iter().find(|doc| doc.uid == uid).is_some_and(|doc| drafts::pending(ctx, (uid, doc.identity_epoch)))
        })
}

/// Revert ignores command parameters in the engine; Close alone accepts an explicit index.
pub(crate) fn guard_command(app: &DesignApp, id: &str, params: &Value) -> Result<(), String> {
    let ui_armed = app.egui_ctx.as_ref().is_some_and(drafts::armed);
    if !busy(app) && !app.file_providers.connected() && !ui_armed {
        return Ok(());
    }
    if matches!(id, "app.close" | "app.revert") && (busy(app) || app.egui_ctx.as_ref().is_some_and(drafts::any_pending)) {
        return Err("finish or cancel pending provider operations before closing or reverting the application".into());
    }
    let index = match id {
        "file.close" => match params.get("index") {
            Some(index) => Some(index.as_u64().and_then(|value| usize::try_from(value).ok()).ok_or("invalid document index")?),
            None => app.session.active_index(),
        },
        "file.revert" => app.session.active_index(),
        _ => return Ok(()),
    };
    let doc = index.and_then(|index| app.session.documents().get(index)).ok_or("document is no longer open")?;
    if pending_for(app, doc.uid) || doc.interaction.is_some() || doc.preview_stash.is_some() || ime_pending(app, (doc.uid, doc.identity_epoch)) {
        return Err("finish or cancel the pending provider operation for this document before closing or reverting".into());
    }
    Ok(())
}

pub(crate) fn save(app: &mut DesignApp, save_as: bool, after: Option<After>) -> Result<Value, String> {
    if after.is_some() && app.provider_after.len() >= MAX_CONTINUATIONS {
        return Err("finish or cancel pending save continuations before starting another".into());
    }
    let host = app.file_providers.clone();
    let connection = host.connection().ok_or("file provider is not connected")?;
    let doc = app.session.active().ok_or("no document")?;
    if pending_for(app, doc.uid) {
        return Err("a provider operation is already pending for this document".into());
    }
    if doc.preview_stash.is_some() {
        return Err("leave Data Merge Preview before saving with a file provider".into());
    }
    let (uid, revision, identity_epoch, snapshot) = (doc.uid, doc.revision, doc.identity_epoch, doc.clone());
    let name = host.bound_filename(uid).unwrap_or_else(|| format!("{}.designcraft", doc.doc.title));
    let serialized = app.session.execute("file.serialize", &json!({})).map_err(|e| e.to_string())?;
    let base64 = serialized["base64"].as_str().ok_or("native encoder returned no project")?;
    let bytes = designcraft_engine::cmd::base64_decode(base64);
    let request = host.begin(vec![snapshot], uid, revision, name, bytes, save_as)?;
    if let Some(callback) = after {
        app.provider_after.insert(request.clone(), PendingAfter { uid, identity_epoch, revision, connection, ready: false, callback });
    }
    app.status("Choose where to save the native project");
    Ok(json!({"pending":"fileProvider","requestId":request}))
}

pub(crate) fn observe_ui(app: &DesignApp, ctx: &egui::Context) {
    let live: Vec<_> = app.session.documents().iter().map(|doc| (doc.uid, doc.identity_epoch)).collect();
    drafts::scope(ctx, active_identity(app), app.file_providers.connected(), &live);
}

fn ime_pending(app: &DesignApp, identity: (u64, u64)) -> bool {
    app.session.ime_text_context().is_some_and(|context| {
        (context.document_uid, context.document_epoch) == identity
            && (app.session.tool_composing() || app.ime_marked.is_some() || app.pending_ime_delete.is_some())
    })
}

fn active_identity(app: &DesignApp) -> Option<(u64, u64)> {
    app.session.active().map(|doc| (doc.uid, doc.identity_epoch))
}

fn activate_identity(app: &mut DesignApp, identity: Option<(u64, u64)>) {
    if active_identity(app) == identity {
        return;
    }
    if let Some(index) =
        identity.and_then(|(uid, epoch)| app.session.documents().iter().position(|doc| doc.uid == uid && doc.identity_epoch == epoch))
    {
        let _ = app.session.execute("file.activate", &json!({"index":index}));
    }
}

fn apply_save(app: &mut DesignApp, host: &Host, request: String, capture: Capture, status: String, resource: String) {
    let identity = capture.snapshots.iter().find(|snapshot| snapshot.uid == capture.doc).map(|snapshot| (snapshot.uid, snapshot.identity_epoch));
    let alive = identity.is_some_and(|(uid, epoch)| app.session.documents().iter().any(|doc| doc.uid == uid && doc.identity_epoch == epoch));
    if status == "saved" && alive {
        for snapshot in &capture.snapshots {
            if let Some(doc) = app.session.document_mut_by_uid(snapshot.uid).filter(|doc| doc.identity_epoch == snapshot.identity_epoch) {
                doc.saved_doc = snapshot.doc.clone();
                doc.saved_revision = snapshot.revision;
                doc.path = None;
            }
        }
    }
    // Release the captured owner first. A full event queue retains it until a later drain.
    host.finish_save(&request, &capture, &status, &resource, alive);
    if status == "saved" && alive {
        if let Some(after) = app.provider_after.get_mut(&request) {
            after.ready = true;
        }
        app.status("Saved with file provider");
    } else {
        if app.provider_after.remove(&request).is_some() {
            app.quitting = None;
        }
        if status == "failed" {
            app.status("File provider save failed; the document is still unsaved");
        }
    }
}

fn apply_open(app: &mut DesignApp, host: &Host, request: String, resource: String, name: String, bytes: Vec<u8>) {
    let result = if let Some(uid) = host.bound_document(&resource).filter(|uid| app.session.documents().iter().any(|doc| doc.uid == *uid)) {
        match app.session.documents().iter().position(|doc| doc.uid == uid) {
            Some(index) => app.run("file.activate", json!({"index":index})).map(|_| uid),
            None => Err("provider document closed".into()),
        }
    } else {
        app.open_file("file.openBytes", json!({"name":name,"base64":designcraft_engine::cmd::base64_encode(&bytes)}))
            .and_then(|_| app.session.active().map(|doc| doc.uid).ok_or_else(|| "native open returned no document".into()))
    };
    if let Err(error) = &result {
        app.status(error);
    }
    host.finish_open(&request, &resource, &name, result);
}

fn continuations(app: &mut DesignApp, host: &Host, ctx: &egui::Context) {
    let connection = host.connection();
    let requests: Vec<_> = app.provider_after.keys().cloned().collect();
    for request in requests {
        let Some(after) = app.provider_after.get(&request) else { continue };
        let current = app.session.documents().iter().find(|doc| doc.uid == after.uid && doc.identity_epoch == after.identity_epoch);
        let valid = connection.as_deref() == Some(after.connection.as_str())
            && current.is_some_and(|doc| !after.ready || (doc.saved_revision == after.revision && !doc.is_dirty()));
        if !valid {
            app.provider_after.remove(&request);
            app.quitting = None;
            continue;
        }
        if !after.ready
            || host.pending_for(after.uid)
            || drafts::pending(ctx, (after.uid, after.identity_epoch))
            || ime_pending(app, (after.uid, after.identity_epoch))
            || current.is_some_and(|doc| doc.interaction.is_some() || doc.preview_stash.is_some())
        {
            continue;
        }
        // Remove our own guard only after all other operations for this identity have finished.
        let Some(after) = app.provider_after.remove(&request) else { continue };
        let previous = active_identity(app);
        // All UI callbacks carry their own UID. Activating A here would commit B's IME/preview.
        if let Err(error) = after.callback.provider(app, (after.uid, after.identity_epoch)) {
            app.status(error);
            app.quitting = None;
        }
        activate_identity(app, previous);
    }
}

pub(crate) fn tick(app: &mut DesignApp) {
    let host = app.file_providers.clone();
    for action in host.take_actions() {
        match action {
            Action::Save { request, capture, status, resource } => apply_save(app, &host, request, capture, status, resource),
            Action::Open { request, resource, name, bytes } => apply_open(app, &host, request, resource, name, bytes),
        }
    }
    if host.connected() {
        host.refresh(app.session.documents().iter().map(|doc| (doc.uid, doc.revision, doc.doc.title.clone(), doc.is_dirty())).collect());
    }
}

/// UI callbacks run only after all viewports, text widgets and IME consumed a settled pass.
/// Hidden logic ticks may apply a durable checkpoint, but cannot discard unprocessed input.
pub(crate) fn settled(app: &mut DesignApp, ctx: &egui::Context) {
    let host = app.file_providers.clone();
    continuations(app, &host, ctx);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;

#[cfg(test)]
mod ui_tests;
