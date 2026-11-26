//! The craft framework's dialog host. A dialog with a framework form (`form`) shows in the framework's own modal
//! `Dialog`: its title bar, its footer buttons in the platform's order, Enter for the accept button and the one-Escape
//! rule (Escape over a field's draft cancels the dialog with it, as in InDesign). The form edits the same `Dialog` draft
//! the native window did, and accepting or cancelling goes through the same `confirm` and `cancel`, so every command,
//! document change and undo step is the existing controller's.
//!
//! Ported dialogs register their form in [`form`]; dialogs without one keep the native window until they are ported.

use super::Dialog;
use crate::DesignApp;
use craft_ui_panels::dialog::ButtonPlace;
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
    /// The dialog opens `width` wide, or as wide as the window lets a dialog be where that is narrower, instead of as
    /// wide as its content (New Document's presets fill what they are given).
    pub(crate) opens_at_width: bool,
    /// Where the buttons stand: the foot by default; `ButtonPlace::Body` for a form whose body keeps their cell with a
    /// `FooterSlot` (the Color Picker's column at the top right).
    pub(crate) button_place: ButtonPlace,
    /// Buttons after the extra ones that act and keep the dialog open: their labels (i18n keys) and the field each sets
    /// to `true` for the form to act on in its next pass (the Color Picker's Add CMYK Swatch).
    pub(crate) actions: &'static [(&'static str, &'static str)],
}

impl Form {
    pub(crate) const fn new(show: fn(&mut DesignApp, &mut Ui, &mut Dialog) -> Option<bool>) -> Self {
        Form { show, width: 640.0, extra: &[], footer: None, opens_at_width: false, button_place: ButtonPlace::Foot, actions: &[] }
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

    pub(crate) const fn opens_at_width(mut self) -> Self {
        self.opens_at_width = true;
        self
    }
}

/// The captions of a section list, one row each, keyed by position.
struct SectionCaptions(Vec<String>);

impl craft_behavior::DisplayOrder<usize> for SectionCaptions {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn key(&self, at: usize) -> usize {
        at
    }
    fn index_of(&self, key: &usize) -> Option<usize> {
        (*key < self.0.len()).then_some(*key)
    }
    fn text(&self, at: usize) -> Option<std::borrow::Cow<'_, str>> {
        self.0.get(at).map(|text| std::borrow::Cow::Borrowed(text.as_str()))
    }
}
impl craft_behavior::ItemSource<usize> for SectionCaptions {}

/// The section list of a dialog with pages (Preferences, Paragraph and Character Style Options, Text Frame Options, the
/// Format Settings of Find/Change, Find/Replace Font's fonts): a navigation list named `label`, one row per section of
/// `sections` (id and caption in the interface language), with the row of `current` highlighted (the first row when
/// `current` is none of them), as tall as its rows (a body that scrolls around it scrolls it). The arrow keys move the
/// highlight. Returns the id of the section picked in this pass when it is another one.
pub(crate) fn section_list(ui: &mut Ui, id_salt: &str, label: String, sections: &[(&str, String)], current: &str) -> Option<String> {
    use craft_behavior::Selection;
    use craft_ui_widgets::contract::{Bind, Props as _, SelectMode, ShowCollection as _, Sizing};
    use craft_ui_widgets::list::{List, ListRow};

    let captions = SectionCaptions(sections.iter().map(|(_, caption)| caption.clone()).collect());
    let at = sections.iter().position(|(id, _)| *id == current).unwrap_or(0);
    let mut selected = Selection::single(at);
    let _ = List::<usize>::new(label)
        .id_salt(("dialog_section_list", id_salt.to_owned()))
        .select(SelectMode::Single)
        .arrows_select(true)
        .allow_empty(false)
        .header(false)
        .natural_height(true)
        .sizing(Sizing::FillWidth)
        .selection(Bind::bound(&mut selected))
        .show_items(ui, &captions, &mut |key, _| ListRow::new(captions.0.get(*key).cloned().unwrap_or_default()));
    // Only a row picked in this pass reports: a `current` the list doesn't have shows the first row without picking it.
    selected.items.first().filter(|picked| **picked != at).and_then(|picked| sections.get(*picked)).map(|(id, _)| (*id).to_owned())
}

/// The share of the window's width a framework dialog may take before its body scrolls (`Overlay::dialog`'s cap).
const DIALOG_WIDTH_SHARE: f32 = 0.8;

/// How many times a dialog has opened in this context, and the dialog the host showed last: its id and the pass.
#[derive(Clone, Default)]
struct Opening {
    count: u64,
    id: String,
    pass: Option<u64>,
}

fn opening_key() -> egui::Id {
    egui::Id::unique("designcraft-framework-dialog-opening")
}

/// The opening of dialog `id` the host shows in this pass. It counts one more when the dialog was not shown in the
/// pass before (it opened, or opened again), or another dialog was. The dialog's id takes it, so each opening starts
/// in the middle of the window: the framework keeps a dialog's placement under its id, and a dialog dragged by its
/// title, closed and opened again would otherwise come back where it was dragged.
fn opening(ctx: &egui::Context, id: &str) -> u64 {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| {
        let held = data.get_temp_mut_or_default::<Opening>(opening_key());
        let shown_before = held.id == id && held.pass.is_some_and(|last| last == pass || last + 1 == pass);
        if !shown_before {
            held.count += 1;
            held.id = id.to_owned();
        }
        held.pass = Some(pass);
        held.count
    })
}

/// Forgets the dialog the host showed last, once it closed: the next dialog to show is a new opening, even in the next
/// pass and with the same id.
fn closed(ctx: &egui::Context) {
    ctx.data_mut(|data| data.get_temp_mut_or_default::<Opening>(opening_key()).pass = None);
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
        super::STEP_AND_REPEAT => Some(super::special_forms::STEP_AND_REPEAT),
        super::CORNER_OPTIONS => Some(super::special_forms::CORNER_OPTIONS),
        super::TOC => Some(super::special_forms::TOC),
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
    for (label, _) in form.extra.iter().chain(form.actions) {
        buttons.push(FooterButton::new(tr(label), DialogRole::Other).enabled(enabled).stays_open(true));
    }
    let mut opened = true;
    let mut from_form = None;
    let opening = opening(ctx, &dialog_id);
    // The width the window lets a dialog take: a dialog that asks for more opens this wide, and a wide dialog's least
    // width gives way to it, so the footer's buttons stay in a small window.
    let room = ctx.content_rect().width() * DIALOG_WIDTH_SHARE;
    let tokens = craft_ui_widgets::runtime::env::Env::of_root(ctx).tokens.clone();
    // The dialog chrome's own least width is `LeastWidth::Dialog`, which a container cannot drop: in a window too narrow
    // for it, a smaller size role stands in, so the dialog, its footer buttons included, stays inside the window.
    let least = [LeastWidth::WideDialog, LeastWidth::Dialog, LeastWidth::Popup, LeastWidth::DialogButton]
        .into_iter()
        .filter(|least| form.width >= 700.0 || *least != LeastWidth::WideDialog)
        .find(|least| least.points(tokens.tokens()) <= room);
    // The body and the footer content both edit the app and the draft; the dialog runs them one after the other.
    let state = RefCell::new((app, d));
    let cell = &state;
    // The area only gives the dialog a parent: the framework mounts its own modal layer.
    let shown = egui::Area::new(egui::Id::unique(("framework_dialog_host", dialog_id.as_str()))).order(egui::Order::Foreground).show(ctx, |ui| {
        let dialog = FrameworkDialog::new(title)
            .id_salt(("framework_dialog", dialog_id.clone(), opening))
            .opened(Bind::bound(&mut opened))
            .enabled(enabled)
            // No dimmed backdrop: the page behind a dialog is what its settings and colours are judged against.
            .backdrop(Backdrop::None)
            // As the old app's dialogs: a dragged dialog may go past the window's edges, keeping its title strip inside.
            .drag_bounds(craft_ui::contract::overlay::DragBounds::TitleVisible)
            .closable(false)
            .outer_gesture(false)
            .close_on_enter(enabled)
            .button_place(form.button_place)
            .buttons(buttons);
        let dialog = match least {
            Some(least) => dialog.least_width(least),
            None => dialog,
        };
        let dialog = if form.opens_at_width {
            dialog.overlay(|overlay| overlay.min_width(craft_behavior::overlay::MinWidth::Points(form.width.min(room))))
        } else {
            dialog
        };
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
                // The body can run more than once in a pass (a measuring run), and only the run that saw the click reports it.
                from_form = (form.show)(app, ui, d).or(from_form);
            })
            .output
    });
    let (app, mut d) = state.into_inner();
    let out = shown.inner;
    let picked_after = out.picked.and_then(|picked| picked.checked_sub(if d.id == "alert" { 1 } else { 2 }));
    let picked_extra = picked_after.filter(|at| *at < form.extra.len());
    if let Some((_, field)) = picked_extra.and_then(|at| form.extra.get(at)) {
        d.fields.insert((*field).into(), serde_json::json!(true));
    }
    // An action keeps the dialog open: the form acts on its field in the next pass.
    if let Some((_, field)) = picked_after.and_then(|at| at.checked_sub(form.extra.len())).and_then(|at| form.actions.get(at)) {
        d.fields.insert((*field).into(), serde_json::json!(true));
        ctx.request_repaint();
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
    if app.ui.dialog.is_none() {
        closed(ctx);
    }
}
