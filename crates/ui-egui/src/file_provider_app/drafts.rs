//! Document ownership for numeric drafts. The original widget IDs stay in use until opt-in.
//! Once opted in, IDs remain stable across provider suspension and disconnection.
use std::collections::HashMap;

type Owner = (u64, u64);
const MAX_FIELDS: usize = 1024;
const STATE: &str = "file-provider-numeric-drafts";

#[derive(Clone, Copy)]
struct Field {
    owner: Owner,
    viewport: egui::ViewportId,
    dirty: bool,
}

#[derive(Clone, Default)]
struct Drafts {
    armed: bool,
    owners: HashMap<egui::ViewportId, Owner>,
    fields: HashMap<egui::Id, Field>,
}

fn state_id() -> egui::Id {
    egui::Id::unique(STATE)
}

fn state(ctx: &egui::Context) -> Drafts {
    ctx.data(|data| data.get_temp::<Drafts>(state_id())).unwrap_or_default()
}

pub(super) fn scope(ctx: &egui::Context, owner: Option<Owner>, connected: bool, live: &[Owner]) {
    let mut drafts = state(ctx);
    drafts.armed |= connected;
    let expired: Vec<_> = drafts.fields.iter().filter(|(_, field)| !live.contains(&field.owner)).map(|(key, _)| *key).collect();
    drafts.fields.retain(|_, field| live.contains(&field.owner));
    if drafts.armed {
        for key in expired {
            ctx.data_mut(|data| data.remove::<String>(key));
            ctx.memory_mut(|memory| memory.surrender_focus(key.with("te")));
        }
    }
    drafts.owners.retain(|_, owner| live.contains(owner));
    if let Some(owner) = owner {
        drafts.owners.insert(ctx.viewport_id(), owner);
    } else {
        drafts.owners.remove(&ctx.viewport_id());
    }
    ctx.data_mut(|data| data.insert_temp(state_id(), drafts));
}

pub(crate) fn field_key(ui: &egui::Ui, original: egui::Id) -> Option<egui::Id> {
    let mut drafts = state(ui.ctx());
    let Some(owner) = drafts.owners.get(&ui.ctx().viewport_id()).copied() else { return Some(original) };
    let legacy = drafts.fields.get(&original).is_some_and(|field| field.owner == owner && field.viewport == ui.ctx().viewport_id())
        || (!drafts.fields.contains_key(&original)
            && (ui.memory(|memory| memory.has_focus(original.with("te"))) || ui.data(|data| data.get_temp::<String>(original).is_some())));
    let key = if !drafts.armed || legacy { original } else { original.with(("provider-document", owner, ui.ctx().viewport_id())) };
    let dirty = ui.data(|data| data.get_temp::<String>(key).is_some());
    let admitted = admit_field(&mut drafts, key, Field { owner, viewport: ui.ctx().viewport_id(), dirty }, dirty);
    ui.ctx().data_mut(|data| data.insert_temp(state_id(), drafts));
    admitted.then_some(key)
}

fn admit_field(drafts: &mut Drafts, key: egui::Id, field: Field, existing_buffer: bool) -> bool {
    if drafts.armed && drafts.fields.len() >= MAX_FIELDS && !drafts.fields.contains_key(&key) && !existing_buffer {
        return false;
    }
    // Never discard an already-resident String to enforce an admission limit. Its owner is
    // recorded conservatively; this temporary excess retires as those drafts commit/cancel.
    drafts.fields.entry(key).or_insert(field);
    true
}

pub(crate) fn parked(ui: &egui::Ui, key: egui::Id) -> bool {
    let drafts = state(ui.ctx());
    drafts.armed && drafts.fields.get(&key).is_some_and(|field| field.dirty)
}

/// A committed value is applied by the caller before the settled-pass continuation runs.
pub(crate) fn field_result(ui: &egui::Ui, key: egui::Id, focused: bool, changed: bool, committed: bool, keep: bool) {
    let mut drafts = state(ui.ctx());
    let owner = drafts.owners.get(&ui.ctx().viewport_id()).copied();
    let armed = drafts.armed;
    if !armed
        && let Some(owner) = owner
        && let Some(field) = drafts.fields.get_mut(&key)
    {
        field.owner = owner;
    }
    record_result(&mut drafts, key, focused, changed, committed, keep);
    ui.ctx().data_mut(|data| data.insert_temp(state_id(), drafts));
}

fn record_result(drafts: &mut Drafts, key: egui::Id, focused: bool, changed: bool, committed: bool, keep: bool) {
    if let Some(field) = drafts.fields.get_mut(&key) {
        field.dirty = !committed && (field.dirty || changed);
        if !focused && !keep {
            drafts.fields.remove(&key);
        }
    }
}

pub(super) fn armed(ctx: &egui::Context) -> bool {
    state(ctx).armed
}

pub(super) fn any_pending(ctx: &egui::Context) -> bool {
    let drafts = state(ctx);
    drafts.armed && drafts.fields.values().any(|field| field.dirty)
}

pub(super) fn pending(ctx: &egui::Context, owner: Owner) -> bool {
    let drafts = state(ctx);
    drafts.armed && drafts.fields.values().any(|field| field.owner == owner && field.dirty)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn draft_ownership_survives_opt_in_disconnect_and_other_document_focus() {
        let ctx = egui::Context::default();
        let a = (11, 21);
        let b = (12, 22);
        let key = egui::Id::unique("legacy-field");
        scope(&ctx, Some(a), false, &[a, b]);
        let mut drafts = state(&ctx);
        drafts.fields.insert(key, Field { owner: a, viewport: ctx.viewport_id(), dirty: true });
        ctx.data_mut(|data| data.insert_temp(state_id(), drafts));
        scope(&ctx, Some(a), true, &[a, b]);
        assert!(pending(&ctx, a));
        scope(&ctx, Some(b), true, &[a, b]);
        assert!(pending(&ctx, a));
        assert!(!pending(&ctx, b));
        scope(&ctx, Some(b), false, &[a, b]);
        assert!(state(&ctx).armed);
        assert!(pending(&ctx, a));
        scope(&ctx, Some(b), false, &[b]);
        assert!(!pending(&ctx, a));
    }

    #[test]
    fn capacity_refuses_new_buffers_but_keeps_existing_drafts_scoped_and_recovers_without_restart() {
        let ctx = egui::Context::default();
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &serde_json::json!({})).unwrap();
        let doc = session.active().unwrap();
        let a = (doc.uid, doc.identity_epoch);
        session.execute("file.new", &serde_json::json!({})).unwrap();
        let doc = session.active().unwrap();
        let b = (doc.uid, doc.identity_epoch);
        let mut app = crate::DesignApp::new(session, crate::Services::default());
        app.egui_ctx = Some(ctx.clone());
        let viewport = ctx.viewport_id();
        scope(&ctx, Some(a), true, &[a, b]);
        let mut drafts = state(&ctx);
        for ordinal in 0..MAX_FIELDS {
            assert!(admit_field(&mut drafts, egui::Id::unique(("filled", ordinal)), Field { owner: a, viewport, dirty: true }, false,));
        }
        let fresh = egui::Id::unique("fresh");
        assert!(!admit_field(&mut drafts, fresh, Field { owner: b, viewport, dirty: false }, false));
        let orphan = egui::Id::unique("orphan");
        ctx.data_mut(|data| data.insert_temp(orphan, "123".to_string()));
        let existing = ctx.data(|data| data.get_temp::<String>(orphan).is_some());
        assert!(admit_field(&mut drafts, orphan, Field { owner: a, viewport, dirty: existing }, existing));
        assert!(drafts.fields.get(&orphan).is_some_and(|field| field.dirty));
        ctx.data_mut(|data| data.insert_temp(state_id(), drafts));
        assert!(pending(&ctx, a));
        assert!(!pending(&ctx, b));
        scope(&ctx, Some(b), true, &[a, b]);
        assert!(pending(&ctx, a));
        assert_eq!(ctx.data(|data| data.get_temp::<String>(orphan)), Some("123".to_string()));
        assert!(app.run("file.close", serde_json::json!({"index":0})).is_err());
        app.run("file.close", serde_json::json!({"index":1})).unwrap();
        assert_eq!(app.session.documents().len(), 1);
        assert_eq!(app.session.active().unwrap().uid, a.0);
        // Apply the same completion path as explicit numeric commit/cancel, preserving every
        // buffer until that resolution. Reclaimed capacity does not need a restart.
        let mut drafts = state(&ctx);
        let keys: Vec<_> = drafts.fields.keys().copied().collect();
        for key in keys {
            record_result(&mut drafts, key, false, false, true, false);
        }
        ctx.data_mut(|data| {
            data.insert_temp(state_id(), drafts);
            data.remove::<String>(orphan);
        });
        assert!(!pending(&ctx, a));
        assert!(!pending(&ctx, b));
        app.file_providers.set_session("0123456789abcdef0123456789abcdef".into()).unwrap();
        app.file_providers.connect("test".into(), "Test".into(), "account".into()).unwrap();
        assert!(crate::file_provider_app::save(&mut app, false, None).is_ok());
    }

    #[test]
    fn the_admission_limit_does_not_change_local_fields_before_the_first_opt_in() {
        let mut drafts = Drafts::default();
        let owner = (11, 21);
        for ordinal in 0..=MAX_FIELDS {
            assert!(admit_field(
                &mut drafts,
                egui::Id::unique(("local", ordinal)),
                Field { owner, viewport: egui::ViewportId::ROOT, dirty: false },
                false,
            ));
        }
        assert!(!drafts.armed);
        assert_eq!(drafts.fields.len(), MAX_FIELDS + 1);
    }
}
