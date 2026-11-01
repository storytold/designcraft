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
    overflow: bool,
}

fn state_id() -> egui::Id {
    egui::Id::new(STATE)
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
    if drafts.fields.len() < MAX_FIELDS {
        drafts.overflow = false;
    }
    ctx.data_mut(|data| data.insert_temp(state_id(), drafts));
}

pub(crate) fn field_key(ui: &egui::Ui, original: egui::Id) -> egui::Id {
    let mut drafts = state(ui.ctx());
    let Some(owner) = drafts.owners.get(&ui.ctx().viewport_id()).copied() else { return original };
    let legacy = drafts.fields.get(&original).is_some_and(|field| field.owner == owner && field.viewport == ui.ctx().viewport_id())
        || (!drafts.fields.contains_key(&original)
            && (ui.memory(|memory| memory.has_focus(original.with("te"))) || ui.data(|data| data.get_temp::<String>(original).is_some())));
    let key = if !drafts.armed || legacy { original } else { original.with(("provider-document", owner, ui.ctx().viewport_id())) };
    if drafts.fields.len() < MAX_FIELDS || drafts.fields.contains_key(&key) {
        drafts.fields.entry(key).or_insert(Field { owner, viewport: ui.ctx().viewport_id(), dirty: false });
    } else {
        drafts.overflow = true;
    }
    ui.ctx().data_mut(|data| data.insert_temp(state_id(), drafts));
    key
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
    if let Some(field) = drafts.fields.get_mut(&key) {
        if !armed && let Some(owner) = owner {
            field.owner = owner;
        }
        field.dirty = !committed && (field.dirty || changed);
        if !focused && !keep {
            drafts.fields.remove(&key);
        }
    }
    ui.ctx().data_mut(|data| data.insert_temp(state_id(), drafts));
}

pub(super) fn armed(ctx: &egui::Context) -> bool {
    state(ctx).armed
}

pub(super) fn any_pending(ctx: &egui::Context) -> bool {
    let drafts = state(ctx);
    drafts.armed && (drafts.overflow || drafts.fields.values().any(|field| field.dirty))
}

pub(super) fn pending(ctx: &egui::Context, owner: Owner) -> bool {
    let drafts = state(ctx);
    drafts.armed && (drafts.overflow || drafts.fields.values().any(|field| field.owner == owner && field.dirty))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_ownership_survives_opt_in_disconnect_and_other_document_focus() {
        let ctx = egui::Context::default();
        let a = (11, 21);
        let b = (12, 22);
        let key = egui::Id::new("legacy-field");
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
}
