//! Native DesignCraft projects over an optional same-origin storage provider.
use crate::{DesignApp, file_providers::Action};
use serde_json::{Value, json};
pub(crate) type After = Box<dyn FnOnce(&mut DesignApp) -> Result<Value, String>>;

pub(crate) fn save(app: &mut DesignApp, save_as: bool, after: Option<After>) -> Result<Value, String> {
    let host = app.file_providers.clone();
    let doc = app.session.active().ok_or("no document")?;
    if host.pending_for(doc.uid) {
        return Err("a provider save is already pending for this document".into());
    }
    if doc.preview_stash.is_some() {
        return Err("leave Data Merge Preview before saving with a file provider".into());
    }
    let (uid, revision, snapshot) = (doc.uid, doc.revision, doc.clone());
    let name = host.bound_filename(uid).unwrap_or_else(|| format!("{}.designcraft", doc.doc.title));
    let serialized = app.session.execute("file.serialize", &json!({})).map_err(|e| e.to_string())?;
    let base64 = serialized["base64"].as_str().ok_or("native encoder returned no project")?;
    let bytes = designcraft_engine::cmd::base64_decode(base64);
    let request = host.begin(vec![snapshot], uid, revision, name, bytes, save_as)?;
    if let Some(after) = after {
        app.provider_after.insert(request.clone(), after);
    }
    app.status("Choose where to save the native project");
    Ok(json!({"pending":"fileProvider","requestId":request}))
}
pub(crate) fn tick(app: &mut DesignApp) {
    let host = app.file_providers.clone();
    if !host.connected() {
        return;
    }
    for action in host.take_actions() {
        match action {
            Action::Save { request, capture, status, resource } => {
                let previous = app.session.active_index();
                let index = app.session.documents().iter().position(|d| d.uid == capture.doc);
                let alive = index
                    .is_some_and(|i| capture.snapshots.iter().any(|snapshot| app.session.documents()[i].identity_epoch == snapshot.identity_epoch));
                if status == "saved" {
                    if let Some(index) = index {
                        let _ = app.session.execute("file.activate", &json!({"index":index}));
                        for snapshot in &capture.snapshots {
                            if let Some(doc) =
                                app.session.active_mut().filter(|d| d.uid == snapshot.uid && d.identity_epoch == snapshot.identity_epoch)
                            {
                                doc.saved_doc = snapshot.doc.clone();
                                doc.saved_revision = snapshot.revision;
                                if status == "saved" {
                                    doc.path = None;
                                }
                            }
                        }
                    }
                    if let Some(after) = app.provider_after.remove(&request) {
                        if alive {
                            if let Err(error) = after(app) {
                                app.status(error);
                            }
                        } else {
                            app.cancel_quit();
                        }
                    }
                    app.status("Saved with file provider");
                } else {
                    if app.provider_after.remove(&request).is_some() {
                        app.cancel_quit();
                    }
                    if status == "failed" {
                        app.status("File provider save failed; the document is still unsaved");
                    }
                }
                if let Some(previous) = previous.filter(|i| *i < app.session.documents().len()) {
                    let _ = app.session.execute("file.activate", &json!({"index":previous}));
                }
                host.finish_save(&request, &capture, &status, &resource, alive);
            }
            Action::Open { request, resource, name, bytes } => {
                let result = if let Some(uid) = host.bound_document(&resource).filter(|uid| app.session.documents().iter().any(|d| d.uid == *uid)) {
                    match app.session.documents().iter().position(|d| d.uid == uid) {
                        Some(index) => app.run("file.activate", json!({"index":index})).map(|_| uid),
                        None => Err("provider document closed".into()),
                    }
                } else {
                    app.open_file("file.openBytes", json!({"name":name,"base64":designcraft_engine::cmd::base64_encode(&bytes)}))
                        .and_then(|_| app.session.active().map(|d| d.uid).ok_or_else(|| "native open returned no document".into()))
                };
                if let Err(error) = &result {
                    app.status(error);
                }
                host.finish_open(&request, &resource, &name, result);
            }
        }
    }
    host.refresh(app.session.documents().iter().map(|d| (d.uid, d.revision, d.doc.title.clone(), d.is_dirty())).collect());
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use designcraft_engine::Session;
    fn app() -> DesignApp {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let app = DesignApp::new(session, crate::Services::default());
        app.file_providers.set_session("0123456789abcdef0123456789abcdef".into()).unwrap();
        app.file_providers.connect("test".into(), "Test storage".into(), "account".into()).unwrap();
        app
    }
    fn request(app: &mut DesignApp) -> String {
        save(app, false, None).unwrap();
        let events = app.file_providers.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].metadata["type"], "saveDialog");
        events[0].metadata["requestId"].as_str().unwrap().to_string()
    }
    #[test]
    fn provider_ack_cannot_clear_edits_made_after_capture() {
        let mut app = app();
        std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "Before capture".into();
        let request = request(&mut app);
        app.file_providers.resolve_dialog(&request, "provider", "saved.designcraft").unwrap();
        let events = app.file_providers.take_events();
        assert!(!events[0].bytes.as_ref().unwrap().is_empty());
        std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "After capture".into();
        app.file_providers.result(request, "saved".into(), "file-1".into()).unwrap();
        tick(&mut app);
        assert!(app.session.active().unwrap().is_dirty());
    }
    #[test]
    fn successful_ack_updates_the_captured_document_not_the_current_tab() {
        let mut app = app();
        std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "Before capture".into();
        let uid = app.session.active().unwrap().uid;
        let request = request(&mut app);
        app.file_providers.resolve_dialog(&request, "provider", "saved.designcraft").unwrap();
        app.session.execute("file.new", &json!({})).unwrap();
        assert_ne!(app.session.active().unwrap().uid, uid);
        app.file_providers.result(request, "saved".into(), "file-1".into()).unwrap();
        tick(&mut app);
        assert!(!app.session.documents().iter().find(|d| d.uid == uid).unwrap().is_dirty());
        assert_eq!(app.file_providers.bound_document("file-1"), Some(uid));
    }
    #[test]
    fn download_result_does_not_claim_durable_provider_storage() {
        let mut app = app();
        std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "Before capture".into();
        let request = request(&mut app);
        app.file_providers.resolve_dialog(&request, "device", "local.designcraft").unwrap();
        app.file_providers.result(request, "download".into(), String::new()).unwrap();
        tick(&mut app);
        assert!(app.session.active().unwrap().is_dirty());
        assert!(app.file_providers.snapshot()["resources"].as_array().unwrap().is_empty());
    }
}
