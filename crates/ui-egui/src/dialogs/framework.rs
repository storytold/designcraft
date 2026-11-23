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
#[path = "style_forms.rs"]
pub(crate) mod style_forms;

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
    /// Draws the footer's own content before the buttons over the draft (New Document's Preview and status line).
    pub(crate) footer: Option<fn(&mut DesignApp, &mut Ui, &mut Dialog)>,
}

impl Form {
    pub(crate) const fn new(show: fn(&mut DesignApp, &mut Ui, &mut Dialog) -> Option<bool>) -> Self {
        Form { show, width: 640.0, extra: &[], footer: None }
    }

    pub(crate) const fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub(crate) const fn extra(mut self, extra: &'static [(&'static str, &'static str)]) -> Self {
        self.extra = extra;
        self
    }

    pub(crate) const fn footer(mut self, footer: fn(&mut DesignApp, &mut Ui, &mut Dialog)) -> Self {
        self.footer = Some(footer);
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
        "footnoteOptions" => Some(super::text_forms::FOOTNOTE_OPTIONS),
        "paragraphRules" => Some(super::text_forms::PARAGRAPH_RULES),
        "justification" => Some(super::text_forms::JUSTIFICATION),
        "hyphenation" => Some(super::text_forms::HYPHENATION),
        "insertXref" => Some(super::text_forms::INSERT_XREF),
        "paragraphLocal" => Some(super::paragraph_forms::PARAGRAPH_LOCAL),
        "textFrameOptions" => Some(super::frame_forms::TEXT_FRAME_OPTIONS),
        "fittingOptions" => Some(super::frame_forms::FITTING_OPTIONS),
        "polygonSettings" => Some(super::frame_forms::POLYGON_SETTINGS),
        "qrCode" => Some(super::frame_forms::QR_CODE),
        "layerOptions" => Some(super::frame_forms::LAYER_OPTIONS),
        "pdfExport" => Some(super::export_forms::PDF_EXPORT),
        "epubExport" => Some(super::export_forms::EPUB_EXPORT),
        "print" => Some(super::export_forms::PRINT),
        "colorSettings" => Some(super::document_forms::COLOR_SETTINGS),
        "documentSetup" => Some(super::document_forms::DOCUMENT_SETUP),
        "fileInfo" => Some(super::document_forms::FILE_INFO),
        "findChange" => Some(super::find_forms::FIND_CHANGE),
        "findFont" => Some(super::find_forms::FIND_FONT),
        "userDictionary" => Some(super::find_forms::USER_DICTIONARY),
        "menus" => Some(super::import_forms::MENUS),
        "importOptions" => Some(super::import_forms::IMPORT_OPTIONS),
        "contactSheet" => Some(super::import_forms::CONTACT_SHEET),
        "pdfImport" => Some(super::import_forms::PDF_IMPORT),
        crate::effects_dialog::ID => Some(crate::effects_dialog::FORM),
        crate::object_style_dialog::ID => Some(crate::object_style_dialog::FORM),
        "cellOptions" | "diagonalLines" => Some(super::cell_options_form::FORM),
        "colorPicker" => Some(super::color_forms::COLOR_PICKER),
        "swatchOptions" => Some(super::color_forms::SWATCH_OPTIONS),
        "colorGroupOptions" => Some(super::color_forms::COLOR_GROUP_OPTIONS),
        "layerSettings" => Some(super::color_forms::LAYER_SETTINGS),
        "colorTheme" => Some(super::color_forms::COLOR_THEME),
        "paragraphStyleOptions" => Some(style_forms::PARAGRAPH_STYLE_OPTIONS),
        "characterStyleOptions" => Some(style_forms::CHARACTER_STYLE_OPTIONS),
        "deleteParagraphStyle" => Some(style_forms::DELETE_PARAGRAPH_STYLE),
        "deleteCharacterStyle" => Some(style_forms::DELETE_CHARACTER_STYLE),
        "loadStyles" => Some(style_forms::LOAD_STYLES),
        "ruby" => Some(style_forms::RUBY),
        id if super::command_form::is_generic(id) => Some(super::command_form::COMMAND_FORM),
        crate::package_dialog::ID => Some(crate::package_dialog::FORM),
        crate::export_batch_dialog::ID => Some(crate::export_batch_dialog::FORM),
        "newDocument" => Some(crate::new_document::FORM),
        crate::package_dialog::INSTRUCTIONS => Some(crate::package_dialog::INSTRUCTIONS_FORM),
        "preferences" => Some(super::preferences::FORM),
        _ => None,
    }
}

/// Shows the active dialog in the framework's modal with `form` as its body, then confirms or cancels it as the native
/// window does. `enabled` is the enabled state of the app's Ui the dialog belongs to: a disabled app takes no accept,
/// cancel or edit through its dialog.
pub(crate) fn show(app: &mut DesignApp, ctx: &egui::Context, form: Form, enabled: bool) {
    use craft_ui_panels::dialog::Dialog as FrameworkDialog;
    use craft_ui_panels::sections::{DialogRole, FooterButton};
    use craft_ui_widgets::contract::{Bind, LeastWidth, Props as _, ShowContainer as _};
    use craft_ui_widgets::runtime::overlay::Backdrop;
    use std::cell::RefCell;

    let Some(d) = app.ui.dialog.clone() else { return };
    let dialog_id = d.id.clone();
    let title = super::dialog_title(app, &d);
    let (ok, cancel) = super::footer_labels(app, &d);
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).trim().to_owned();
    let mut buttons = vec![FooterButton::new(tr(&ok), DialogRole::Accept).enabled(enabled).stays_open(true)];
    if d.id != "alert" {
        buttons.push(FooterButton::new(tr(cancel), DialogRole::Cancel).enabled(enabled));
    }
    for (label, _) in form.extra {
        buttons.push(FooterButton::new(tr(label), DialogRole::Other).enabled(enabled).stays_open(true));
    }
    let mut opened = true;
    let mut from_form = None;
    // The body and the footer content both edit the app and the draft; the dialog runs them one after the other.
    let state = RefCell::new((app, d));
    let cell = &state;
    // The area only gives the dialog a parent: the framework mounts its own modal layer.
    let shown = egui::Area::new(egui::Id::unique(("framework_dialog_host", dialog_id.as_str())))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let dialog = FrameworkDialog::new(title)
                .id_salt(("framework_dialog", dialog_id.clone()))
                .opened(Bind::bound(&mut opened))
                .enabled(enabled)
                // No dimmed backdrop: the page behind a dialog is what its settings and colours are judged against.
                .backdrop(Backdrop::None)
                .closable(false)
                .outer_gesture(false)
                .close_on_enter(enabled)
                .least_width(if form.width >= 700.0 { LeastWidth::WideDialog } else { LeastWidth::Dialog })
                .buttons(buttons);
            let dialog = match form.footer {
                Some(footer) => dialog.footer(move |ui, _| {
                    let mut held = cell.borrow_mut();
                    let (app, d) = &mut *held;
                    footer(app, ui, d);
                }),
                None => dialog,
            };
            dialog
                .show(ui, |ui, _| {
                    let mut held = cell.borrow_mut();
                    let (app, d) = &mut *held;
                    from_form = (form.show)(app, ui, d);
                })
                .output
        });
    let (app, mut d) = state.into_inner();
    let out = shown.inner;
    let picked_extra = out.picked.and_then(|picked| picked.checked_sub(if d.id == "alert" { 1 } else { 2 }));
    if let Some((_, field)) = picked_extra.and_then(|at| form.extra.get(at)) {
        d.fields.insert((*field).into(), serde_json::json!(true));
    }
    app.ui.dialog = Some(d);
    let result = match (out.picked, from_form) {
        _ if !enabled => None,
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
