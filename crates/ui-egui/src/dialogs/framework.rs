//! The craft framework's dialog host. A dialog with a framework form (`form`) shows in the framework's own modal
//! `Dialog`: its title bar, its footer buttons in the platform's order, Enter for the accept button and the one-Escape
//! rule (Escape over a field's draft cancels the dialog with it, as in InDesign). The form edits the same `Dialog` draft
//! the native window did, and accepting or cancelling goes through the same `confirm` and `cancel`, so every command,
//! document change and undo step is the existing controller's.
//!
//! Ported dialogs register their form in [`form`]; dialogs without one keep the native window until they are ported.

use super::Dialog;
use crate::DesignApp;
use egui::Ui;

#[path = "simple_forms.rs"]
mod simple_forms;

/// A dialog's framework form.
#[derive(Clone, Copy)]
pub(crate) struct Form {
    /// Draws the form over the draft. `Some(true)` when the form itself accepts (a double-clicked preset, Adjust Layout),
    /// `Some(false)` when it cancels; `None` otherwise.
    pub(crate) show: fn(&mut DesignApp, &mut Ui, &mut Dialog) -> Option<bool>,
    /// The width the dialog asks for, in points.
    pub(crate) width: f32,
    /// Extra footer buttons after accept and cancel: their labels (i18n keys) and the field each sets to `true` before
    /// accepting (Close Document's Don't Save sets `discard`).
    pub(crate) extra: &'static [(&'static str, &'static str)],
}

impl Form {
    pub(crate) const fn new(show: fn(&mut DesignApp, &mut Ui, &mut Dialog) -> Option<bool>) -> Self {
        Form { show, width: 640.0, extra: &[] }
    }

    pub(crate) const fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub(crate) const fn extra(mut self, extra: &'static [(&'static str, &'static str)]) -> Self {
        self.extra = extra;
        self
    }
}

/// The framework form of the dialog `id`, once that dialog is ported.
pub(crate) fn form(id: &str) -> Option<Form> {
    #[allow(clippy::match_single_binding)]
    match id {
        // Ported dialogs: one arm each, `"id" => Some(module::FORM),`.
        "alert" => Some(simple_forms::ALERT),
        "closeDocument" => Some(simple_forms::CLOSE_DOCUMENT),
        "deletePages" => Some(simple_forms::DELETE_PAGES),
        "updateLink" => Some(simple_forms::UPDATE_LINK),
        "newWorkspace" => Some(simple_forms::NEW_WORKSPACE),
        "frameSize" => Some(simple_forms::FRAME_SIZE),
        "goToRow" => Some(simple_forms::GO_TO_ROW),
        "goToPage" => Some(simple_forms::GO_TO_PAGE),
        "insertPages" => Some(simple_forms::INSERT_PAGES),
        "pagesPanelOptions" => Some(simple_forms::PAGES_PANEL_OPTIONS),
        "parentOptions" => Some(simple_forms::PARENT_OPTIONS),
        "insertTable" => Some(super::table_forms::INSERT),
        "convertTextToTable" | "convertTableToText" => Some(super::table_forms::CONVERT),
        "createGuides" => Some(super::layout_forms::CREATE_GUIDES),
        "marginsAndColumns" => Some(super::layout_forms::MARGINS_AND_COLUMNS),
        "textFrameOptions" => Some(super::frame_forms::TEXT_FRAME_OPTIONS),
        "fittingOptions" => Some(super::frame_forms::FITTING_OPTIONS),
        "polygonSettings" => Some(super::frame_forms::POLYGON_SETTINGS),
        "qrCode" => Some(super::frame_forms::QR_CODE),
        "layerOptions" => Some(super::frame_forms::LAYER_OPTIONS),
        crate::effects_dialog::ID => Some(crate::effects_dialog::FORM),
        crate::object_style_dialog::ID => Some(crate::object_style_dialog::FORM),
        "cellOptions" | "diagonalLines" => Some(super::cell_options_form::FORM),
        _ => None,
    }
}

/// Shows the active dialog in the framework's modal with `form` as its body, then confirms or cancels it as the native
/// window does.
pub(crate) fn show(app: &mut DesignApp, ctx: &egui::Context, form: Form) {
    use craft_ui_panels::dialog::Dialog as FrameworkDialog;
    use craft_ui_panels::sections::{DialogRole, FooterButton};
    use craft_ui_widgets::contract::{Bind, LeastWidth, Props as _, ShowContainer as _};
    use craft_ui_widgets::runtime::overlay::Backdrop;

    let Some(mut d) = app.ui.dialog.clone() else { return };
    let title = super::dialog_title(app, &d);
    let (ok, cancel) = super::footer_labels(app, &d);
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).trim().to_owned();
    let mut buttons = vec![FooterButton::new(tr(&ok), DialogRole::Accept).stays_open(true)];
    if d.id != "alert" {
        buttons.push(FooterButton::new(tr(cancel), DialogRole::Cancel));
    }
    for (label, _) in form.extra {
        buttons.push(FooterButton::new(tr(label), DialogRole::Other).stays_open(true));
    }
    let mut opened = true;
    let mut from_form = None;
    // The area only gives the dialog a parent: the framework mounts its own modal layer.
    let shown = egui::Area::new(egui::Id::unique(("framework_dialog_host", d.id.as_str())))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            FrameworkDialog::new(title)
                .id_salt(("framework_dialog", d.id.clone()))
                .opened(Bind::bound(&mut opened))
                .enabled(ui.is_enabled())
                // No dimmed backdrop: the page behind a dialog is what its settings and colours are judged against.
                .backdrop(Backdrop::None)
                .closable(false)
                .outer_gesture(false)
                .close_on_enter(true)
                .least_width(if form.width >= 700.0 { LeastWidth::WideDialog } else { LeastWidth::Dialog })
                .buttons(buttons)
                .show(ui, |ui, _| from_form = (form.show)(app, ui, &mut d))
                .output
        });
    let out = shown.inner;
    let picked_extra = out.picked.and_then(|picked| picked.checked_sub(if d.id == "alert" { 1 } else { 2 }));
    if let Some((_, field)) = picked_extra.and_then(|at| form.extra.get(at)) {
        d.fields.insert((*field).into(), serde_json::json!(true));
    }
    app.ui.dialog = Some(d);
    let result = match (out.picked, from_form) {
        (_, Some(result)) => Some(result),
        (Some(0), _) => Some(true),
        (Some(_), _) if picked_extra.is_some() => Some(true),
        _ if out.closed.is_some() => Some(false),
        _ => None,
    };
    match result {
        Some(true) => {
            let _ = super::confirm(app);
        }
        Some(false) => super::cancel(app),
        None => {}
    }
}
